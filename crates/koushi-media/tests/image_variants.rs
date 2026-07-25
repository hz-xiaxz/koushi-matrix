use std::io::Cursor;

use image::{DynamicImage, GenericImageView, ImageFormat, Rgba, RgbaImage};
use koushi_media::{
    ImageOutputFormat, ImageOutputRequest, ImagePreparationError, ImagePreparationPolicy,
    ImageResizeScale, PreparedImageFormat, prepare_image_output, prepare_image_variants,
};

fn synthetic_png(width: u32, height: u32) -> Vec<u8> {
    let image = RgbaImage::from_fn(width, height, |x, y| {
        Rgba([(x % 251) as u8, (y % 239) as u8, 127, ((x + y) % 256) as u8])
    });
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image)
        .write_to(&mut bytes, ImageFormat::Png)
        .expect("encode fixture");
    bytes.into_inner()
}

fn synthetic_jpeg(width: u32, height: u32) -> Vec<u8> {
    let image = RgbaImage::from_fn(width, height, |x, y| {
        Rgba([(x % 251) as u8, (y % 239) as u8, 127, 255])
    });
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image)
        .write_to(&mut bytes, ImageFormat::Jpeg)
        .expect("encode fixture");
    bytes.into_inner()
}

fn synthetic_apng() -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 2, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_animated(2, 0).expect("enable APNG");
        encoder.validate_sequence(true);
        let mut writer = encoder.write_header().expect("write APNG header");
        writer
            .write_image_data(&[255, 0, 0, 255, 0, 255, 0, 255])
            .expect("write APNG frame one");
        writer
            .write_image_data(&[0, 0, 255, 255, 255, 255, 0, 255])
            .expect("write APNG frame two");
        writer.finish().expect("finish APNG");
    }
    bytes
}

#[test]
fn png_offers_original_resized_png_and_alpha_preserving_webp() {
    let source = synthetic_png(96, 64);
    let variants = prepare_image_variants(
        &source,
        "sample.png",
        "image/png",
        &ImagePreparationPolicy {
            target_long_edge: 48,
            quality_percent: 82,
        },
    )
    .expect("prepare PNG");

    assert_eq!(
        variants
            .iter()
            .map(|variant| variant.format)
            .collect::<Vec<_>>(),
        vec![
            PreparedImageFormat::Png,
            PreparedImageFormat::Png,
            PreparedImageFormat::WebP
        ]
    );
    assert_eq!(variants[1].mime_type, "image/png");
    assert_eq!(variants[1].dimensions, (48, 32));
    assert!(variants[1].metadata_stripped);
    assert_eq!(variants[2].mime_type, "image/webp");
    assert_eq!(
        image::load_from_memory(&variants[2].bytes)
            .unwrap()
            .color()
            .has_alpha(),
        true
    );
}

#[test]
fn jpeg_offers_real_jpeg_and_webp_outputs_and_recommends_no_larger_candidate() {
    let source = synthetic_jpeg(96, 64);
    let variants = prepare_image_variants(
        &source,
        "sample.jpg",
        "image/jpeg",
        &ImagePreparationPolicy {
            target_long_edge: 48,
            quality_percent: 75,
        },
    )
    .expect("prepare JPEG");

    assert_eq!(variants[0].format, PreparedImageFormat::Jpeg);
    assert_eq!(variants[1].mime_type, "image/jpeg");
    assert_eq!(variants[2].mime_type, "image/webp");
    let recommended = variants.iter().find(|variant| variant.recommended).unwrap();
    assert!(recommended.bytes.len() <= source.len());
}

#[test]
fn unsupported_or_animated_input_remains_original_only() {
    let gif = b"GIF89a synthetic animated fixture";
    let variants = prepare_image_variants(
        gif,
        "animation.gif",
        "image/gif",
        &ImagePreparationPolicy::default(),
    )
    .expect("original fallback");

    assert_eq!(variants.len(), 1);
    assert_eq!(variants[0].bytes, gif);
    assert_eq!(variants[0].mime_type, "image/gif");
    assert!(variants[0].recommended);
}

#[test]
fn animated_png_remains_original_only() {
    let apng = synthetic_apng();
    let variants = prepare_image_variants(
        &apng,
        "animation.png",
        "image/png",
        &ImagePreparationPolicy::default(),
    )
    .expect("retain APNG original");

    assert_eq!(variants.len(), 1);
    assert_eq!(variants[0].bytes, apng);
    assert_eq!(variants[0].mime_type, "image/png");
    assert!(variants[0].recommended);
}

#[test]
fn spoofed_image_declaration_uses_binary_mime_and_extension() {
    let source = b"not actually a png";
    let variants = prepare_image_variants(
        source,
        "spoofed.png",
        "image/png",
        &ImagePreparationPolicy::default(),
    )
    .expect("retain unknown original safely");

    assert_eq!(variants.len(), 1);
    assert_eq!(variants[0].mime_type, "application/octet-stream");
    assert_eq!(variants[0].filename, "spoofed.bin");
    assert_eq!(variants[0].format, PreparedImageFormat::Other);
}

// --- #305: independent resize and format output selection ---

fn decoded_dimensions(bytes: &[u8]) -> (u32, u32) {
    image::load_from_memory(bytes)
        .expect("prepared output must decode")
        .dimensions()
}

#[test]
fn resize_scales_both_dimensions_linearly() {
    let source = synthetic_png(1284, 918);
    for (scale, expected) in [
        (ImageResizeScale::Original, (1284, 918)),
        (ImageResizeScale::Half, (642, 459)),
        (ImageResizeScale::Quarter, (321, 229)),
        (ImageResizeScale::Eighth, (160, 114)),
    ] {
        let output = prepare_image_output(
            &source,
            "shot.png",
            ImageOutputRequest {
                resize: scale,
                format: ImageOutputFormat::Keep,
            },
            &ImagePreparationPolicy::default(),
        )
        .expect("resize must succeed");
        assert_eq!(output.dimensions, expected, "{:?} scale", scale);
        assert_eq!(
            decoded_dimensions(&output.bytes),
            expected,
            "reported dimensions must describe the encoded bytes"
        );
    }
}

#[test]
fn resize_floors_tiny_sources_at_one_pixel() {
    let source = synthetic_png(3, 1);
    let output = prepare_image_output(
        &source,
        "tiny.png",
        ImageOutputRequest {
            resize: ImageResizeScale::Eighth,
            format: ImageOutputFormat::Keep,
        },
        &ImagePreparationPolicy::default(),
    )
    .expect("a tiny source must still produce output");
    assert_eq!(output.dimensions, (1, 1));
    assert_eq!(decoded_dimensions(&output.bytes), (1, 1));
}

#[test]
fn keep_preserves_the_source_encoding_while_format_overrides_it() {
    let png = synthetic_png(64, 32);
    let kept = prepare_image_output(
        &png,
        "shot.png",
        ImageOutputRequest {
            resize: ImageResizeScale::Original,
            format: ImageOutputFormat::Keep,
        },
        &ImagePreparationPolicy::default(),
    )
    .expect("keep must succeed");
    assert_eq!(kept.format, PreparedImageFormat::Png);
    assert!(kept.filename.ends_with(".png"));

    for (format, expected, extension) in [
        (ImageOutputFormat::Jpeg, PreparedImageFormat::Jpeg, ".jpg"),
        (ImageOutputFormat::WebP, PreparedImageFormat::WebP, ".webp"),
        (ImageOutputFormat::Png, PreparedImageFormat::Png, ".png"),
    ] {
        let output = prepare_image_output(
            &png,
            "shot.png",
            ImageOutputRequest {
                resize: ImageResizeScale::Half,
                format,
            },
            &ImagePreparationPolicy::default(),
        )
        .expect("format override must succeed");
        assert_eq!(output.format, expected);
        assert!(
            output.filename.ends_with(extension),
            "filename must follow the chosen format"
        );
        assert_eq!(output.dimensions, (32, 16));
        assert_eq!(decoded_dimensions(&output.bytes), (32, 16));
    }
}

#[test]
fn jpeg_output_flattens_alpha_deterministically() {
    let source = synthetic_png(16, 16);
    let first = prepare_image_output(
        &source,
        "alpha.png",
        ImageOutputRequest {
            resize: ImageResizeScale::Original,
            format: ImageOutputFormat::Jpeg,
        },
        &ImagePreparationPolicy::default(),
    )
    .expect("jpeg output must succeed for a source with alpha");
    let second = prepare_image_output(
        &source,
        "alpha.png",
        ImageOutputRequest {
            resize: ImageResizeScale::Original,
            format: ImageOutputFormat::Jpeg,
        },
        &ImagePreparationPolicy::default(),
    )
    .expect("jpeg output must succeed on repeat");
    assert_eq!(
        first.bytes, second.bytes,
        "the same request must encode identical bytes so cache identity holds"
    );
    assert!(first.metadata_stripped);
}

#[test]
fn output_identity_is_the_resize_and_format_pair() {
    let source = synthetic_jpeg(40, 20);
    let mut ids = Vec::new();
    for resize in [
        ImageResizeScale::Original,
        ImageResizeScale::Half,
        ImageResizeScale::Quarter,
        ImageResizeScale::Eighth,
    ] {
        for format in [
            ImageOutputFormat::Keep,
            ImageOutputFormat::Png,
            ImageOutputFormat::Jpeg,
            ImageOutputFormat::WebP,
        ] {
            let output = prepare_image_output(
                &source,
                "photo.jpg",
                ImageOutputRequest { resize, format },
                &ImagePreparationPolicy::default(),
            )
            .expect("every combination must encode");
            ids.push(output.id);
        }
    }
    let unique = ids.iter().collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        unique.len(),
        ids.len(),
        "each resize/format pair needs a distinct cache identity"
    );
}

#[test]
fn undecodable_sources_report_a_decode_failure() {
    assert_eq!(
        prepare_image_output(
            &[],
            "empty.png",
            ImageOutputRequest {
                resize: ImageResizeScale::Original,
                format: ImageOutputFormat::Keep,
            },
            &ImagePreparationPolicy::default(),
        ),
        Err(ImagePreparationError::Empty)
    );
    assert_eq!(
        prepare_image_output(
            b"not an image",
            "junk.png",
            ImageOutputRequest {
                resize: ImageResizeScale::Half,
                format: ImageOutputFormat::WebP,
            },
            &ImagePreparationPolicy::default(),
        ),
        Err(ImagePreparationError::Decode)
    );
}
