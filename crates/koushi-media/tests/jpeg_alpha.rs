use std::io::Cursor;

use image::{DynamicImage, GenericImageView, ImageFormat, Rgba, RgbaImage};
use koushi_media::{
    DecodeLimits, DecodedRgbaImage, ImageOutputFormat, ImageOutputRequest, ImagePreparationPolicy,
    ImageResizeScale, prepare_decoded_image_output, prepare_image_output,
    prepare_image_output_fitted, thumbnail_jpeg,
};

const SCALES: [ImageResizeScale; 4] = [
    ImageResizeScale::Original,
    ImageResizeScale::Half,
    ImageResizeScale::Quarter,
    ImageResizeScale::Eighth,
];

// A synthetic window with transparent margins, a translucent shadow and opaque
// content. RGB values under zero alpha must never become visible in a JPEG.
fn window(hidden_rgb: [u8; 3]) -> RgbaImage {
    RgbaImage::from_fn(128, 128, |x, y| {
        if (48..80).contains(&x) && (48..80).contains(&y) {
            Rgba([0, 0, 0, 255])
        } else if (32..96).contains(&x) && (32..96).contains(&y) {
            Rgba([240, 240, 240, 255])
        } else if (16..112).contains(&x) && (16..112).contains(&y) {
            Rgba([0, 0, 0, 64])
        } else {
            Rgba([hidden_rgb[0], hidden_rgb[1], hidden_rgb[2], 0])
        }
    })
}

fn encoded_window(hidden_rgb: [u8; 3], format: ImageFormat) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(window(hidden_rgb))
        .write_to(&mut bytes, format)
        .expect("encode synthetic window");
    bytes.into_inner()
}

fn jpeg(source: &[u8], resize: ImageResizeScale) -> Vec<u8> {
    prepare_image_output(
        source,
        "window.png",
        ImageOutputRequest {
            resize,
            format: ImageOutputFormat::Jpeg,
        },
        &ImagePreparationPolicy::default(),
    )
    .expect("prepare JPEG")
    .bytes
}

fn decoded_jpeg(bytes: &[u8], edge: u32) -> image::RgbImage {
    let image = image::load_from_memory_with_format(bytes, ImageFormat::Jpeg)
        .expect("decode generated JPEG");
    assert_eq!(image.dimensions(), (edge, edge), "no extra border or crop");
    let rgb = image.to_rgb8();
    assert!(
        rgb.get_pixel(0, 0).0.iter().all(|channel| *channel >= 245),
        "transparent margin must be white, got {:?}",
        rgb.get_pixel(0, 0)
    );
    rgb
}

#[test]
fn jpeg_conversion_makes_transparent_png_and_webp_margins_white_at_every_scale() {
    for format in [ImageFormat::Png, ImageFormat::WebP] {
        let source = encoded_window([0, 0, 0], format);
        for resize in SCALES {
            decoded_jpeg(&jpeg(&source, resize), 128 / resize.divisor());
        }
    }
}

#[test]
fn jpeg_conversion_blends_shadows_and_keeps_opaque_black_content() {
    let source = encoded_window([0, 0, 0], ImageFormat::Png);
    let rgb = decoded_jpeg(&jpeg(&source, ImageResizeScale::Original), 128);
    for (position, expected) in [((24, 64), 191i16), ((40, 64), 240), ((64, 64), 0)] {
        let pixel = rgb.get_pixel(position.0, position.1);
        assert!(
            pixel
                .0
                .iter()
                .all(|channel| (i16::from(*channel) - expected).abs() <= 5),
            "pixel at {position:?} must be near {expected}, got {pixel:?}"
        );
    }
}

#[test]
fn jpeg_resize_does_not_bleed_hidden_rgb_into_visible_edges() {
    let black = encoded_window([0, 0, 0], ImageFormat::Png);
    let magenta = encoded_window([255, 0, 255], ImageFormat::Png);
    for resize in SCALES {
        assert_eq!(jpeg(&black, resize), jpeg(&magenta, resize), "{resize:?}");
    }
}

#[test]
fn fitted_jpeg_composites_before_resizing() {
    for edge in [128, 48] {
        let render = |hidden_rgb| {
            prepare_image_output_fitted(
                &encoded_window(hidden_rgb, ImageFormat::Png),
                "window.png",
                edge,
                ImageOutputFormat::Jpeg,
                &ImagePreparationPolicy::default(),
            )
            .expect("fit JPEG")
            .bytes
        };
        let bytes = render([0, 0, 0]);
        decoded_jpeg(&bytes, edge);
        assert_eq!(bytes, render([255, 0, 255]));
    }
}

#[test]
fn native_decoded_jpeg_composites_before_resizing() {
    for resize in SCALES {
        let render = |hidden_rgb| {
            let decoded = DecodedRgbaImage::from_straight_rgba(
                128,
                128,
                128 * 4,
                window(hidden_rgb).into_raw(),
                DecodeLimits::default(),
            )
            .expect("decoded synthetic window");
            prepare_decoded_image_output(
                decoded,
                "window.heic",
                ImageOutputRequest {
                    resize,
                    format: ImageOutputFormat::Jpeg,
                },
                &ImagePreparationPolicy::default(),
            )
            .expect("encode native pixels")
            .bytes
        };
        let bytes = render([0, 0, 0]);
        decoded_jpeg(&bytes, 128 / resize.divisor());
        assert_eq!(bytes, render([255, 0, 255]));
    }
}

#[test]
fn jpeg_thumbnails_composite_before_resizing() {
    for edge in [128, 48] {
        let render = |hidden_rgb| {
            thumbnail_jpeg(&encoded_window(hidden_rgb, ImageFormat::Png), edge)
                .expect("JPEG thumbnail")
        };
        let bytes = render([0, 0, 0]);
        decoded_jpeg(&bytes, edge);
        assert_eq!(bytes, render([255, 0, 255]));
    }
}

#[test]
fn alpha_preserving_outputs_and_original_keep_retain_transparency() {
    let source = encoded_window([0, 0, 0], ImageFormat::Png);
    for format in [
        ImageOutputFormat::Keep,
        ImageOutputFormat::Png,
        ImageOutputFormat::WebP,
    ] {
        let output = prepare_image_output(
            &source,
            "window.png",
            ImageOutputRequest {
                resize: ImageResizeScale::Original,
                format,
            },
            &ImagePreparationPolicy::default(),
        )
        .expect("alpha-preserving output");
        let decoded = image::load_from_memory(&output.bytes).expect("decode output");
        assert_eq!(decoded.get_pixel(0, 0), Rgba([0, 0, 0, 0]));
        assert_eq!(decoded.get_pixel(24, 64), Rgba([0, 0, 0, 64]));
        if format == ImageOutputFormat::Keep {
            assert_eq!(output.bytes, source);
        }
    }
}
