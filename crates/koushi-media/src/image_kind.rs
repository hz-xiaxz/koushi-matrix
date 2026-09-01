#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageKind {
    pub extension: &'static str,
    pub mime_type: &'static str,
}

pub fn image_kind(bytes: &[u8]) -> Option<ImageKind> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some(ImageKind {
            extension: "png",
            mime_type: "image/png",
        });
    }
    if bytes.len() >= 3 && bytes[0..3] == [0xff, 0xd8, 0xff] {
        return Some(ImageKind {
            extension: "jpg",
            mime_type: "image/jpeg",
        });
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some(ImageKind {
            extension: "gif",
            mime_type: "image/gif",
        });
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some(ImageKind {
            extension: "webp",
            mime_type: "image/webp",
        });
    }
    if bytes.len() >= 12
        && &bytes[4..8] == b"ftyp"
        && (&bytes[8..12] == b"avif" || &bytes[8..12] == b"avis")
    {
        return Some(ImageKind {
            extension: "avif",
            mime_type: "image/avif",
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_kind_detects_png_for_webview_content_type() {
        let kind = image_kind(b"\x89PNG\r\n\x1a\nrest").expect("png image kind");
        assert_eq!(kind.extension, "png");
        assert_eq!(kind.mime_type, "image/png");
    }

    #[test]
    fn image_kind_detects_jpeg_for_webview_content_type() {
        let kind = image_kind(b"\xff\xd8\xff\xe0rest").expect("jpeg image kind");
        assert_eq!(kind.extension, "jpg");
        assert_eq!(kind.mime_type, "image/jpeg");
    }

    #[test]
    fn image_kind_rejects_unknown_bytes() {
        assert_eq!(image_kind(b"not an image"), None);
    }

    #[test]
    fn image_kind_detects_gif_webp_and_avif() {
        for (bytes, extension, mime_type) in [
            (&b"GIF87arest"[..], "gif", "image/gif"),
            (&b"GIF89arest"[..], "gif", "image/gif"),
            (&b"RIFFxxxxWEBPrest"[..], "webp", "image/webp"),
            (&b"\0\0\0\0ftypavifrest"[..], "avif", "image/avif"),
            (&b"\0\0\0\0ftypavisrest"[..], "avif", "image/avif"),
        ] {
            let kind = image_kind(bytes).expect("image kind");
            assert_eq!((kind.extension, kind.mime_type), (extension, mime_type));
        }
    }

    #[test]
    fn image_kind_rejects_truncated_signatures() {
        assert_eq!(image_kind(b"\xff\xd8"), None);
        assert_eq!(image_kind(b"RIFFWEBP"), None);
        assert_eq!(image_kind(b"ftypavif"), None);
    }
}
