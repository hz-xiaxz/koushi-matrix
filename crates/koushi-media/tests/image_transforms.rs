use std::io::Cursor;

use image::{
    DynamicImage, ExtendedColorType, ImageDecoder, ImageEncoder, ImageFormat, ImageReader, Rgba,
    RgbaImage,
    codecs::{jpeg::JpegEncoder, png::PngEncoder, webp::WebPEncoder},
};
use koushi_media::{
    DecodeLimits, DecodedRgbaImage, ImageOutputFormat, ImageOutputRequest, ImagePreparationPolicy,
    ImageResizeScale, prepare_decoded_image_output, prepare_image_output,
    prepare_image_output_fitted, prepare_image_variants, probe_image_dimensions,
};

const SCALES: [ImageResizeScale; 4] = [
    ImageResizeScale::Original,
    ImageResizeScale::Half,
    ImageResizeScale::Quarter,
    ImageResizeScale::Eighth,
];
const FORMATS: [ImageOutputFormat; 4] = [
    ImageOutputFormat::Keep,
    ImageOutputFormat::Png,
    ImageOutputFormat::Jpeg,
    ImageOutputFormat::WebP,
];

fn source(hidden: [u8; 3]) -> RgbaImage {
    RgbaImage::from_fn(64, 40, |x, y| {
        if (16..48).contains(&x) && (8..32).contains(&y) {
            Rgba([255, 255, 255, 255])
        } else {
            Rgba([hidden[0], hidden[1], hidden[2], 0])
        }
    })
}

fn png(image: RgbaImage) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image)
        .write_to(&mut bytes, ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

fn request(resize: ImageResizeScale, format: ImageOutputFormat) -> ImageOutputRequest {
    ImageOutputRequest { resize, format }
}

fn rgba(bytes: &[u8]) -> RgbaImage {
    image::load_from_memory(bytes).unwrap().to_rgba8()
}

fn visible_pixel(pixel: &Rgba<u8>, background: u32) -> [u32; 3] {
    let alpha = u32::from(pixel[3]);
    std::array::from_fn(|c| (u32::from(pixel[c]) * alpha + background * (255 - alpha) + 127) / 255)
}

fn assert_same_visible(a: &[u8], b: &[u8]) {
    let (a, b) = (rgba(a), rgba(b));
    assert_eq!(a.dimensions(), b.dimensions());
    for (a, b) in a.pixels().zip(b.pixels()) {
        assert_eq!(a[3], b[3]);
        for background in [0, 255] {
            assert_eq!(visible_pixel(a, background), visible_pixel(b, background));
        }
    }
}

#[test]
fn png_webp_and_keep_resize_ignore_hidden_rgb_on_light_and_dark_backgrounds() {
    let (black, magenta) = (png(source([0, 0, 0])), png(source([255, 0, 255])));
    for format in [
        ImageOutputFormat::Png,
        ImageOutputFormat::WebP,
        ImageOutputFormat::Keep,
    ] {
        for resize in SCALES {
            let render = |input: &[u8]| {
                prepare_image_output(
                    input,
                    "window.png",
                    request(resize, format),
                    &Default::default(),
                )
                .unwrap()
                .bytes
            };
            assert_same_visible(&render(&black), &render(&magenta));
        }
    }
}

#[test]
fn fitted_and_native_alpha_resizes_ignore_hidden_rgb() {
    for format in [ImageOutputFormat::Png, ImageOutputFormat::WebP] {
        let fitted = |hidden| {
            prepare_image_output_fitted(
                &png(source(hidden)),
                "window.png",
                24,
                format,
                &Default::default(),
            )
            .unwrap()
            .bytes
        };
        assert_same_visible(&fitted([0, 0, 0]), &fitted([255, 0, 255]));
        let native = |hidden| {
            let decoded = DecodedRgbaImage::from_straight_rgba(
                64,
                40,
                64 * 4,
                source(hidden).into_raw(),
                DecodeLimits::default(),
            )
            .unwrap();
            prepare_decoded_image_output(
                decoded,
                "window.heic",
                request(ImageResizeScale::Half, format),
                &Default::default(),
            )
            .unwrap()
            .bytes
        };
        assert_same_visible(&native([0, 0, 0]), &native([255, 0, 255]));
    }
}

fn exif(orientation: u8) -> Vec<u8> {
    vec![
        b'I',
        b'I',
        42,
        0,
        8,
        0,
        0,
        0,
        1,
        0,
        0x12,
        1,
        3,
        0,
        1,
        0,
        0,
        0,
        orientation,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
    ]
}

fn encode<E: ImageEncoder>(mut encoder: E, image: &DynamicImage, orientation: u8) {
    encoder.set_exif_metadata(exif(orientation)).unwrap();
    encoder
        .write_image(
            image.as_bytes(),
            image.width(),
            image.height(),
            image.color().into(),
        )
        .unwrap();
}

fn quadrants() -> RgbaImage {
    RgbaImage::from_fn(64, 40, |x, y| {
        Rgba(match (x < 32, y < 20) {
            (true, true) => [220, 20, 20, 255],
            (false, true) => [20, 220, 20, 255],
            (true, false) => [20, 20, 220, 255],
            (false, false) => [220, 220, 20, 255],
        })
    })
}

fn oriented(format: ImageFormat, orientation: u8) -> Vec<u8> {
    let image = DynamicImage::ImageRgba8(quadrants());
    let mut bytes = Vec::new();
    match format {
        ImageFormat::Png => encode(PngEncoder::new(&mut bytes), &image, orientation),
        ImageFormat::WebP => encode(WebPEncoder::new_lossless(&mut bytes), &image, orientation),
        ImageFormat::Jpeg => encode(
            JpegEncoder::new_with_quality(&mut bytes, 95),
            &DynamicImage::ImageRgb8(image.to_rgb8()),
            orientation,
        ),
        _ => unreachable!(),
    }
    bytes
}

#[test]
fn all_eight_exif_orientations_survive_all_format_and_scale_choices() {
    let corners = [
        [220i16, 20, 20],
        [20, 220, 20],
        [20, 20, 220],
        [220, 220, 20],
    ];
    let expected = [
        [0, 1, 2, 3],
        [1, 0, 3, 2],
        [3, 2, 1, 0],
        [2, 3, 0, 1],
        [0, 2, 1, 3],
        [2, 0, 3, 1],
        [3, 1, 2, 0],
        [1, 3, 0, 2],
    ];
    for input in [ImageFormat::Png, ImageFormat::WebP, ImageFormat::Jpeg] {
        for orientation in 1..=8 {
            let source = oriented(input, orientation);
            let (w, h) = if orientation >= 5 { (40, 64) } else { (64, 40) };
            for format in FORMATS {
                for resize in SCALES {
                    let output = prepare_image_output(
                        &source,
                        "quadrants.image",
                        request(resize, format),
                        &ImagePreparationPolicy {
                            target_long_edge: 2048,
                            quality_percent: 95,
                        },
                    )
                    .unwrap();
                    let expected_size = (w / resize.divisor(), h / resize.divisor());
                    assert_eq!(
                        output.dimensions, expected_size,
                        "{input:?} EXIF={orientation} {format:?} {resize:?}"
                    );
                    if format == ImageOutputFormat::Keep && resize == ImageResizeScale::Original {
                        assert_eq!(source, output.bytes);
                        continue;
                    }
                    let pixels = rgba(&output.bytes);
                    assert_eq!(pixels.dimensions(), expected_size);
                    let (w, h) = pixels.dimensions();
                    for (index, (x, y)) in [
                        (w / 4, h / 4),
                        (3 * w / 4, h / 4),
                        (w / 4, 3 * h / 4),
                        (3 * w / 4, 3 * h / 4),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        let expected = corners[expected[usize::from(orientation - 1)][index]];
                        let actual = pixels.get_pixel(x, y);
                        assert!(
                            (0..3).all(|c| (i16::from(actual[c]) - expected[c]).abs() <= 50),
                            "{input:?} EXIF={orientation} {format:?} {resize:?} corner={index}: {actual:?}"
                        );
                    }
                    let mut decoder = ImageReader::new(Cursor::new(&output.bytes))
                        .with_guessed_format()
                        .unwrap()
                        .into_decoder()
                        .unwrap();
                    assert_eq!(
                        decoder.orientation().unwrap(),
                        image::metadata::Orientation::NoTransforms
                    );
                    assert!(decoder.exif_metadata().unwrap().is_none());
                }
            }
            assert_eq!(probe_image_dimensions(&source), Some((w, h)));
            let variants = prepare_image_variants(
                &source,
                "quadrants.image",
                "image/invalid",
                &Default::default(),
            )
            .unwrap();
            assert!(variants.iter().all(|variant| variant.dimensions == (w, h)));
            let fitted = prepare_image_output_fitted(
                &source,
                "quadrants.image",
                32,
                ImageOutputFormat::Png,
                &Default::default(),
            )
            .unwrap();
            assert_eq!(
                fitted.dimensions,
                if orientation >= 5 { (20, 32) } else { (32, 20) }
            );
        }
    }
}

#[test]
fn lossless_format_conversion_retains_pixels_and_original_keep_retains_bytes() {
    let pixels = source([12, 34, 56]);
    let mut webp = Vec::new();
    WebPEncoder::new_lossless(&mut webp)
        .write_image(&pixels, 64, 40, ExtendedColorType::Rgba8)
        .unwrap();
    for source in [png(pixels.clone()), webp] {
        for format in [
            ImageOutputFormat::Keep,
            ImageOutputFormat::Png,
            ImageOutputFormat::WebP,
        ] {
            let output = prepare_image_output(
                &source,
                "window.image",
                request(ImageResizeScale::Original, format),
                &Default::default(),
            )
            .unwrap();
            assert_eq!(rgba(&output.bytes), rgba(&source));
            if format == ImageOutputFormat::Keep {
                assert_eq!(output.bytes, source);
            }
        }
    }
}

#[test]
fn animated_webp_is_retained_without_silently_converting_its_first_frame() {
    let source = include_bytes!("fixtures/animated.webp");
    assert_eq!(rgba(source).dimensions(), (32, 24));
    for format in FORMATS {
        assert!(
            prepare_image_output(
                source,
                "animation.webp",
                request(ImageResizeScale::Half, format),
                &Default::default()
            )
            .is_err()
        );
    }
    assert!(
        prepare_image_output_fitted(
            source,
            "animation.webp",
            16,
            ImageOutputFormat::Png,
            &Default::default()
        )
        .is_err()
    );
    let variants =
        prepare_image_variants(source, "animation.webp", "image/webp", &Default::default())
            .unwrap();
    assert_eq!(variants.len(), 1);
    assert_eq!(variants[0].bytes, source);
}
