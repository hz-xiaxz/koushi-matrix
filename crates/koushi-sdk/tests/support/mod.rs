pub fn library_production_sources() -> &'static [(&'static str, &'static str)] {
    &[
        ("src/lib.rs", include_str!("../../src/lib.rs")),
        ("src/auth.rs", include_str!("../../src/auth.rs")),
        (
            "src/client_session.rs",
            include_str!("../../src/client_session.rs"),
        ),
        ("src/e2ee.rs", include_str!("../../src/e2ee.rs")),
        ("src/profile.rs", include_str!("../../src/profile.rs")),
        ("src/qa_reports.rs", include_str!("../../src/qa_reports.rs")),
        (
            "src/room_operations.rs",
            include_str!("../../src/room_operations.rs"),
        ),
        (
            "src/room_projection.rs",
            include_str!("../../src/room_projection.rs"),
        ),
        ("src/search.rs", include_str!("../../src/search.rs")),
        (
            "src/sliding_sync_discovery.rs",
            include_str!("../../src/sliding_sync_discovery.rs"),
        ),
        ("src/sync.rs", include_str!("../../src/sync.rs")),
        ("src/timeline.rs", include_str!("../../src/timeline.rs")),
    ]
}
