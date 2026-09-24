use std::io::Cursor;

use image::{DynamicImage, ImageFormat, RgbImage};

use super::thumbnail::{THUMB_MAX_EDGE, thumbnail_jpeg};

fn encode(width: u32, height: u32, format: ImageFormat) -> Vec<u8> {
    let image = DynamicImage::ImageRgb8(RgbImage::from_pixel(width, height, image::Rgb([200, 30, 30])));
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, format).unwrap();
    bytes.into_inner()
}

fn dimensions(jpeg: &[u8]) -> (u32, u32) {
    let image = image::load_from_memory_with_format(jpeg, ImageFormat::Jpeg).unwrap();
    (image.width(), image.height())
}

/// A JPEG with an Exif APP1 segment whose Orientation tag is `orientation`.
fn jpeg_with_orientation(width: u32, height: u32, orientation: u8) -> Vec<u8> {
    let plain = encode(width, height, ImageFormat::Jpeg);
    let mut app1 = vec![0xFF, 0xE1, 0x00, 0x22];
    app1.extend_from_slice(b"Exif\0\0");
    app1.extend_from_slice(&[b'I', b'I', 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00]);
    app1.extend_from_slice(&[0x01, 0x00]);
    app1.extend_from_slice(&[0x12, 0x01, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, orientation, 0x00, 0x00, 0x00]);
    app1.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    let mut bytes = plain[..2].to_vec();
    bytes.extend(app1);
    bytes.extend_from_slice(&plain[2..]);
    bytes
}

#[test]
fn large_images_are_reduced_to_the_max_edge() {
    let thumb = thumbnail_jpeg(&encode(1200, 600, ImageFormat::Png), THUMB_MAX_EDGE).unwrap();
    assert_eq!(dimensions(&thumb), (480, 240));
}

#[test]
fn small_images_keep_their_size_but_are_reencoded() {
    let thumb = thumbnail_jpeg(&encode(10, 10, ImageFormat::Png), THUMB_MAX_EDGE).unwrap();
    assert_eq!(dimensions(&thumb), (10, 10));
    assert_eq!(&thumb[..2], &[0xFF, 0xD8]);
}

#[test]
fn thumbnail_applies_exif_orientation() {
    let thumb = thumbnail_jpeg(&jpeg_with_orientation(40, 20, 6), THUMB_MAX_EDGE).unwrap();
    assert_eq!(dimensions(&thumb), (20, 40));
}

#[test]
fn corrupt_or_unsupported_data_has_no_thumbnail() {
    assert!(thumbnail_jpeg(b"not an image", THUMB_MAX_EDGE).is_none());
    assert!(thumbnail_jpeg(b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>", THUMB_MAX_EDGE).is_none());
    let mut truncated = encode(64, 64, ImageFormat::Png);
    truncated.truncate(40);
    assert!(thumbnail_jpeg(&truncated, THUMB_MAX_EDGE).is_none());
}
