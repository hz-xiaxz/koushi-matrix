use super::*;
use std::fs;
use std::sync::{Mutex as TestMutex, MutexGuard, OnceLock as TestOnceLock};

fn cache_test_lock() -> MutexGuard<'static, ()> {
    static LOCK: TestOnceLock<TestMutex<()>> = TestOnceLock::new();
    LOCK.get_or_init(|| TestMutex::new(()))
        .lock()
        .expect("renderable thumbnail cache test lock should not be poisoned")
}

#[test]
fn stores_avatar_and_link_preview_thumbnails_with_opaque_refs() {
    let _guard = cache_test_lock();
    clear_renderable_thumbnail_cache();

    let avatar = store_renderable_thumbnail(
        RenderableThumbnailKind::Avatar,
        "mxc://example.test/avatar",
        b"avatar-bytes".to_vec(),
    )
    .expect("avatar bytes are within the cache bound");
    let link_preview = store_renderable_thumbnail(
        RenderableThumbnailKind::LinkPreview,
        "https://example.test/page",
        b"preview-bytes".to_vec(),
    )
    .expect("link-preview bytes are within the cache bound");

    let AvatarThumbnailState::Ready {
        source_ref,
        mime_type,
        ..
    } = avatar
    else {
        panic!("avatar thumbnail should be ready");
    };
    assert!(source_ref.starts_with("avatar/"));
    assert!(!source_ref.contains("://"));
    assert_eq!(mime_type.as_deref(), Some("application/octet-stream"));

    let AvatarThumbnailState::Ready {
        source_ref,
        mime_type,
        ..
    } = link_preview
    else {
        panic!("link-preview thumbnail should be ready");
    };
    assert!(source_ref.starts_with("link-preview/"));
    assert!(!source_ref.contains("://"));
    assert_eq!(mime_type.as_deref(), Some("application/octet-stream"));
}

#[test]
fn lookup_renderable_thumbnail_returns_bytes_for_opaque_ref() {
    let _guard = cache_test_lock();
    clear_renderable_thumbnail_cache();

    let ready = store_renderable_thumbnail(
        RenderableThumbnailKind::Avatar,
        "mxc://example.test/lookup",
        b"lookup-bytes".to_vec(),
    )
    .expect("thumbnail bytes are within the cache bound");
    let AvatarThumbnailState::Ready { source_ref, .. } = ready else {
        panic!("thumbnail should be ready");
    };

    let content = lookup_renderable_thumbnail(&source_ref).expect("thumbnail should be cached");
    assert_eq!(content.bytes, b"lookup-bytes");
    assert_eq!(
        content.mime_type.as_deref(),
        Some("application/octet-stream")
    );
}

#[test]
fn ready_thumbnail_refs_survive_session_cache_churn() {
    let _guard = cache_test_lock();
    clear_renderable_thumbnail_cache();

    let ready = store_renderable_thumbnail(
        RenderableThumbnailKind::Avatar,
        "mxc://example.test/pinned",
        b"pinned-bytes".to_vec(),
    )
    .expect("pinned thumbnail bytes are within the cache bound");
    let AvatarThumbnailState::Ready { source_ref, .. } = ready else {
        panic!("thumbnail should be ready");
    };
    for index in 0..=128 {
        let source = format!("mxc://example.test/churn/{index}");
        let bytes = format!("bytes-{index}").into_bytes();
        store_renderable_thumbnail(RenderableThumbnailKind::Avatar, &source, bytes)
            .expect("churn thumbnail bytes are within the cache bound");
    }

    let content = lookup_renderable_thumbnail(&source_ref)
        .expect("Ready thumbnail ref must remain available until session clear");
    assert_eq!(content.bytes, b"pinned-bytes");
}

#[test]
fn lookup_rejects_uri_and_traversal_instead_of_parsing_adapter_schemes() {
    assert!(lookup_renderable_thumbnail("../avatar/0123456789abcdef").is_none());
    assert!(lookup_renderable_thumbnail("avatar/not-hex").is_none());
}

#[test]
fn thumbnail_cache_is_bounded_by_entry_count_and_retained_bytes() {
    let mut cache = RenderableThumbnailCache::default();
    for index in 0..=(MAX_RENDERABLE_THUMBNAIL_ENTRIES + 8) {
        cache
            .insert(
                format!("avatar/{index}"),
                vec![u8::try_from(index % 251).unwrap(); 1024],
                "image/png".to_owned(),
            )
            .expect("test entry is within the cache bound");
    }

    let stats = cache.stats();
    assert!(stats.entry_count <= MAX_RENDERABLE_THUMBNAIL_ENTRIES);
    assert!(stats.retained_bytes <= MAX_RENDERABLE_THUMBNAIL_BYTES);
    assert!(stats.eviction_count > 0);
    assert!(cache.get("avatar/0").is_none(), "oldest entry is evicted");
}

#[test]
fn oversized_thumbnail_is_rejected_without_publishing_a_ready_url() {
    let _guard = cache_test_lock();
    clear_renderable_thumbnail_cache();
    let rejection_count_before = renderable_thumbnail_cache_stats().oversize_rejection_count;

    let result = store_renderable_thumbnail(
        RenderableThumbnailKind::Avatar,
        "mxc://example.test/oversized",
        vec![0; MAX_RENDERABLE_THUMBNAIL_BYTES + 1],
    );

    assert_eq!(
        result,
        Err(RenderableThumbnailStoreError::TooLarge {
            byte_count: MAX_RENDERABLE_THUMBNAIL_BYTES + 1,
            max_bytes: MAX_RENDERABLE_THUMBNAIL_BYTES,
        })
    );
    let stats = renderable_thumbnail_cache_stats();
    assert_eq!(stats.entry_count, 0);
    assert_eq!(stats.retained_bytes, 0);
    assert_eq!(
        stats.oversize_rejection_count,
        rejection_count_before.saturating_add(1)
    );
}

#[test]
fn clear_renderable_thumbnail_cache_drops_previous_session_bytes() {
    let _guard = cache_test_lock();
    clear_renderable_thumbnail_cache();

    let ready = store_renderable_thumbnail(
        RenderableThumbnailKind::Avatar,
        "mxc://example.test/session-scoped",
        b"session-bytes".to_vec(),
    )
    .expect("thumbnail bytes are within the cache bound");
    let AvatarThumbnailState::Ready { source_ref, .. } = ready else {
        panic!("thumbnail should be ready");
    };
    assert!(lookup_renderable_thumbnail(&source_ref).is_some());

    clear_renderable_thumbnail_cache();

    assert!(lookup_renderable_thumbnail(&source_ref).is_none());
}

#[test]
fn cleanup_legacy_plaintext_thumbnail_dirs_preserves_media_downloads() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let data_dir = tempdir.path();

    fs::create_dir_all(data_dir.join("avatar_thumbnails")).expect("seed avatar dir");
    fs::write(
        data_dir.join("avatar_thumbnails").join("thumb.bin"),
        b"avatar",
    )
    .expect("seed avatar file");
    fs::create_dir_all(data_dir.join("link_preview_thumbnails")).expect("seed preview dir");
    fs::write(
        data_dir.join("link_preview_thumbnails").join("preview.bin"),
        b"preview",
    )
    .expect("seed preview file");
    fs::create_dir_all(data_dir.join("media_downloads")).expect("seed media dir");
    fs::write(
        data_dir.join("media_downloads").join("download.bin"),
        b"download",
    )
    .expect("seed download file");

    cleanup_legacy_plaintext_thumbnail_dirs(data_dir).expect("cleanup should succeed");

    assert!(!data_dir.join("avatar_thumbnails").exists());
    assert!(!data_dir.join("link_preview_thumbnails").exists());
    assert!(data_dir.join("media_downloads").exists());
    assert_eq!(
        fs::read(data_dir.join("media_downloads").join("download.bin")).expect("media file"),
        b"download"
    );
}
