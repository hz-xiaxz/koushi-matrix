use std::io::Cursor;

use image::{
    ExtendedColorType, ImageDecoder, ImageEncoder, ImageFormat, ImageReader,
    codecs::{jpeg::JpegEncoder, png::PngEncoder, webp::WebPEncoder},
};
use koushi_media::{
    ImageOutputFormat, ImageOutputRequest, ImageResizeScale, prepare_image_output,
    prepare_image_output_fitted, prepare_image_variants, thumbnail_jpeg,
};

// Generated with Pillow ImageCms.createProfile("sRGB") / LittleCMS, with the
// ICC header date normalized to 2000-01-01. Contains no external or user data.
const SRGB: &[u8] = include_bytes!("fixtures/srgb.icc");

fn encode<E: ImageEncoder>(mut encoder: E, icc: &[u8]) {
    encoder.set_icc_profile(icc.to_vec()).unwrap();
    encoder
        .write_image(
            &[73, 149, 211].repeat(64 * 40),
            64,
            40,
            ExtendedColorType::Rgb8,
        )
        .unwrap();
}

fn source(format: ImageFormat, icc: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    match format {
        ImageFormat::Png => encode(PngEncoder::new(&mut bytes), icc),
        ImageFormat::Jpeg => encode(JpegEncoder::new_with_quality(&mut bytes, 95), icc),
        ImageFormat::WebP => encode(WebPEncoder::new_lossless(&mut bytes), icc),
        _ => unreachable!(),
    }
    bytes
}

fn assert_color(bytes: &[u8]) {
    let mut decoder = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .unwrap()
        .into_decoder()
        .unwrap();
    assert_eq!(decoder.icc_profile().unwrap().as_deref(), Some(SRGB));
    assert!(decoder.exif_metadata().unwrap().is_none());
    let rgb = image::load_from_memory(bytes).unwrap().to_rgb8();
    let actual = rgb.get_pixel(rgb.width() / 2, rgb.height() / 2);
    for (actual, expected) in actual.0.into_iter().zip([73i16, 149, 211]) {
        assert!((i16::from(actual) - expected).abs() <= 6);
    }
}

#[test]
fn rgb_color_profile_survives_all_source_output_and_scale_combinations() {
    for input in [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::WebP] {
        let source = source(input, SRGB);
        for format in [
            ImageOutputFormat::Keep,
            ImageOutputFormat::Png,
            ImageOutputFormat::Jpeg,
            ImageOutputFormat::WebP,
        ] {
            for resize in [
                ImageResizeScale::Original,
                ImageResizeScale::Half,
                ImageResizeScale::Quarter,
                ImageResizeScale::Eighth,
            ] {
                let output = prepare_image_output(
                    &source,
                    "color.image",
                    ImageOutputRequest { resize, format },
                    &Default::default(),
                )
                .unwrap();
                assert_color(&output.bytes);
            }
        }
        for format in [
            ImageOutputFormat::Png,
            ImageOutputFormat::Jpeg,
            ImageOutputFormat::WebP,
        ] {
            let output = prepare_image_output_fitted(
                &source,
                "color.image",
                24,
                format,
                &Default::default(),
            )
            .unwrap();
            assert_color(&output.bytes);
        }
        for output in
            prepare_image_variants(&source, "color.image", "image/invalid", &Default::default())
                .unwrap()
        {
            assert_color(&output.bytes);
        }
        assert_color(&thumbnail_jpeg(&source, 24).unwrap());
    }
}

#[test]
fn non_rgb_profile_is_not_attached_to_rgb_pixels_or_silently_discarded() {
    let mut incompatible = SRGB.to_vec();
    incompatible[16..20].copy_from_slice(b"GRAY");
    let source = source(ImageFormat::Png, &incompatible);
    let original = prepare_image_output(
        &source,
        "color.png",
        ImageOutputRequest {
            resize: ImageResizeScale::Original,
            format: ImageOutputFormat::Keep,
        },
        &Default::default(),
    )
    .unwrap();
    assert_eq!(original.bytes, source);
    assert!(
        prepare_image_output(
            &source,
            "color.png",
            ImageOutputRequest {
                resize: ImageResizeScale::Half,
                format: ImageOutputFormat::Png
            },
            &Default::default()
        )
        .is_err()
    );
    assert!(thumbnail_jpeg(&source, 24).is_none());
}

#[test]
fn webp_profile_bytes_are_not_mistaken_for_an_animation_chunk() {
    let mut profile = SRGB.to_vec();
    // ICC device-manufacturer signature; these bytes are not a RIFF chunk.
    profile[48..52].copy_from_slice(b"ANIM");
    let source = source(ImageFormat::WebP, &profile);
    let output = prepare_image_output(
        &source,
        "color.webp",
        ImageOutputRequest {
            resize: ImageResizeScale::Half,
            format: ImageOutputFormat::Png,
        },
        &Default::default(),
    )
    .expect("a still WebP with an RGB profile remains convertible");
    let mut decoder = ImageReader::new(Cursor::new(&output.bytes))
        .with_guessed_format()
        .unwrap()
        .into_decoder()
        .unwrap();
    assert_eq!(decoder.icc_profile().unwrap(), Some(profile));
}
