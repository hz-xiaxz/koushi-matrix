use koushi_state::{AvatarImage, AvatarThumbnailFailureKind, AvatarThumbnailState, OwnProfile};

fn summary(thumbnail: Option<AvatarThumbnailState>) -> String {
    let profile = OwnProfile {
        display_name: Some("private display name".into()),
        avatar: thumbnail.map(|thumbnail| AvatarImage {
            mxc_uri: "mxc://synthetic.invalid/private-avatar".into(),
            thumbnail,
        }),
    };
    let event = super::own_avatar_event(&profile, 2);
    let formatted = koushi_diagnostics::format_event(&event);
    for private in [
        "private display name",
        "mxc://",
        "private-avatar",
        "private-source",
    ] {
        assert!(!formatted.contains(private));
    }
    formatted
}

#[test]
fn own_avatar_summary_separates_metadata_pending_failure_and_missing_bytes() {
    assert!(summary(None).contains("thumbnail_state=absent"));
    assert!(
        summary(Some(AvatarThumbnailState::NotRequested)).contains("thumbnail_state=not_requested")
    );
    assert!(
        summary(Some(AvatarThumbnailState::Loading { request_id: 1 }))
            .contains("thumbnail_state=loading")
    );
    for (kind, token) in [
        (AvatarThumbnailFailureKind::Network, "network"),
        (AvatarThumbnailFailureKind::Forbidden, "forbidden"),
        (AvatarThumbnailFailureKind::Unsupported, "unsupported"),
        (AvatarThumbnailFailureKind::Sdk, "sdk"),
        (AvatarThumbnailFailureKind::Capacity, "capacity"),
    ] {
        assert!(
            summary(Some(AvatarThumbnailState::Failed {
                request_id: 1,
                kind
            }))
            .contains(&format!("failure_kind={token}"))
        );
    }
    let missing = summary(Some(AvatarThumbnailState::Ready {
        source_ref: "private-source".into(),
        width: None,
        height: None,
        mime_type: None,
    }));
    assert!(missing.contains("thumbnail_state=ready"));
    assert!(missing.contains("retained_bytes=missing"));
}

#[tokio::test]
async fn ready_avatar_sampling_does_not_touch_cache_or_subscribe_to_events() {
    use crate::renderable_thumbnail::{
        RenderableThumbnailKind, renderable_thumbnail_cache_stats, store_renderable_thumbnail,
    };
    let _guard = crate::renderable_thumbnail::test_cache_lock_async().await;
    crate::renderable_thumbnail::clear_renderable_thumbnail_cache();
    let thumbnail = store_renderable_thumbnail(
        RenderableThumbnailKind::Avatar,
        "mxc://synthetic.invalid/private-avatar",
        vec![1],
    )
    .unwrap();
    let source_ref = match &thumbnail {
        AvatarThumbnailState::Ready { source_ref, .. } => source_ref.clone(),
        _ => panic!("stored thumbnail must be ready"),
    };
    for i in 1..crate::renderable_thumbnail::MAX_RENDERABLE_THUMBNAIL_ENTRIES {
        store_renderable_thumbnail(
            RenderableThumbnailKind::Avatar,
            &format!("mxc://synthetic.invalid/pressure-{i}"),
            vec![1],
        )
        .unwrap();
    }
    let before = renderable_thumbnail_cache_stats();
    assert!(summary(Some(thumbnail)).contains("retained_bytes=present"));
    assert_eq!(renderable_thumbnail_cache_stats(), before);
    store_renderable_thumbnail(
        RenderableThumbnailKind::Avatar,
        "mxc://synthetic.invalid/eviction",
        vec![1],
    )
    .unwrap();
    assert!(
        !crate::renderable_thumbnail::contains_renderable_thumbnail(&source_ref),
        "diagnostic observation must not promote the oldest cache entry"
    );
    let (runtime, _stores) = super::super::CoreRuntime::start_isolated_with_event_capacity(4);
    let connection = runtime.attach_without_events();
    let before = runtime.event_queue_diagnostics();
    connection.record_own_avatar_diagnostics(0);
    assert_eq!(runtime.event_queue_diagnostics(), before);
    drop(connection);
    runtime.shutdown().await;
}
