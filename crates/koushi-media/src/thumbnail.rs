//! Reduced JPEG previews of raster images (history export thumbnails).

use std::io::Cursor;

use image::{DynamicImage, ImageDecoder, ImageReader, Limits, codecs::jpeg::JpegEncoder};

const JPEG_QUALITY: u8 = 80;
/// Decoding cap so a hostile image cannot exhaust memory.
const MAX_DECODE_ALLOC: u64 = 512 * 1024 * 1024;

/// A JPEG no larger than `max_edge` on either side, upright per the image's
/// EXIF orientation. `None` when the bytes are not a decodable raster image
/// (unsupported formats such as GIF or SVG, or corrupt data).
pub fn thumbnail_jpeg(bytes: &[u8], max_edge: u32) -> Option<Vec<u8>> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = Limits::default();
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().ok()?;
    let orientation = decoder.orientation().ok();
    let mut image = DynamicImage::from_decoder(decoder).ok()?;
    if let Some(orientation) = orientation {
        image.apply_orientation(orientation);
    }
    if image.width() > max_edge || image.height() > max_edge {
        image = image.thumbnail(max_edge, max_edge);
    }
    let rgb = image.to_rgb8();
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY)
        .encode_image(&rgb)
        .ok()?;
    Some(out)
}
