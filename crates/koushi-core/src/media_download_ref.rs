//! Opaque references for downloaded timeline media (#1619).
//!
//! A download writes a managed cache file under
//! `<account data dir>/media-downloads/<room-hash>/<event-hash>.bin`. Core
//! state and events name that entry with `download/<room-hash>/<event-hash>`,
//! never with a filesystem path. The platform adapter resolves the ref against
//! the account cache roots it already owns; paths stay out of Core state,
//! events, commands, diagnostics, and the webview.

use std::path::{Path, PathBuf};

pub const MEDIA_DOWNLOAD_REF_PREFIX: &str = "download";

/// Build the opaque ref for a downloaded timeline media entry.
pub fn media_download_ref(room_hash: &str, event_hash: &str) -> String {
    format!("{MEDIA_DOWNLOAD_REF_PREFIX}/{room_hash}/{event_hash}")
}

/// Resolve a ref inside one account's media cache root.
///
/// Refuses anything that is not exactly `download/<hash>/<hash>`, so a
/// renderer-supplied value cannot escape the managed cache directory.
pub fn media_download_cache_path(cache_root: &Path, source_ref: &str) -> Option<PathBuf> {
    let mut parts = source_ref.split('/');
    if parts.next()? != MEDIA_DOWNLOAD_REF_PREFIX {
        return None;
    }
    let room_hash = parts.next()?;
    let event_hash = parts.next()?;
    if parts.next().is_some() || !is_path_hash(room_hash) || !is_path_hash(event_hash) {
        return None;
    }
    Some(cache_root.join(room_hash).join(format!("{event_hash}.bin")))
}

/// `sanitize_matrix_id_for_path` emits up to 16 decimal digits.
fn is_path_hash(value: &str) -> bool {
    !value.is_empty() && value.len() <= 16 && value.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_and_resolves_a_ref_inside_the_cache_root() {
        let root = Path::new("/cache/media-downloads");
        let source_ref = media_download_ref("1234567890123456", "6543210987654321");
        assert_eq!(source_ref, "download/1234567890123456/6543210987654321");
        assert_eq!(
            media_download_cache_path(root, &source_ref),
            Some(root.join("1234567890123456").join("6543210987654321.bin"))
        );
    }

    #[test]
    fn rejects_refs_that_could_escape_the_cache_root() {
        let root = Path::new("/cache/media-downloads");
        for source_ref in [
            "",
            "download",
            "download/1",
            "download/1/2/3",
            "download/../../etc/passwd/.bin",
            "download/../1",
            "download/1/..",
            "download/12345678901234567/1",
            "download/abcdef/123",
            "download/12 3/456",
            "asset://download/1/2",
            "/download/1/2",
            "download/1/2\u{0}",
        ] {
            assert_eq!(
                media_download_cache_path(root, source_ref),
                None,
                "{source_ref:?} must not resolve"
            );
        }
    }
}
