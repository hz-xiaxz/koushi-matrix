use koushi_state::{HistoryExportRange, HistoryExportRoomCounts};

use super::index_page::render_index_page;
use super::test_support::labels;
use crate::room_history_export::manifest::{ExportManifest, ManifestRoomStatus, ManifestScope};

#[test]
fn index_lists_rooms_with_status_counts_and_links() {
    let mut manifest = ExportManifest::new(
        ManifestScope::Space {
            id: "!s:example.org".to_owned(),
        },
        HistoryExportRange::AllAvailable,
        "Lab & Friends".to_owned(),
    );
    let counts = HistoryExportRoomCounts {
        exported_events: 12,
        attachments_total: 3,
        attachments_failed: 1,
        ..Default::default()
    };
    manifest.upsert_room(
        "!a:x",
        "General",
        "General (0000000a)",
        ManifestRoomStatus::Completed,
        counts,
    );
    manifest.upsert_room(
        "!b:x",
        "Secret <b>",
        "Secret _b_ (0000000b)",
        ManifestRoomStatus::Skipped,
        HistoryExportRoomCounts::default(),
    );
    manifest.upsert_room(
        "!c:x",
        "Broken",
        "Broken (0000000c)",
        ManifestRoomStatus::Failed,
        HistoryExportRoomCounts::default(),
    );
    let page = String::from_utf8(render_index_page(
        &manifest,
        &labels(),
        1_758_758_400_000,
        "UTC",
    ))
    .unwrap();
    assert!(page.contains("<title>Lab &amp; Friends</title>"), "{page}");
    assert!(
        page.contains("<a href=\"rooms/General%20%280000000a%29/index.html\">General</a>"),
        "{page}"
    );
    assert!(page.contains("12 events"));
    assert!(page.contains("3 attachments"));
    assert!(page.contains("1 not retrieved"));
    assert!(page.contains("Secret &lt;b&gt;"));
    assert!(!page.contains("rooms/Secret"), "skipped rooms have no link");
    assert!(!page.contains("rooms/Broken"), "failed rooms have no link");
    assert!(page.contains("Skipped (not joined)") && page.contains("Failed"));
    assert!(page.contains("All available history"));
    assert!(page.contains("Exported 2025-09-25 00:00"));
    assert_eq!(page.matches("<script").count(), 0);
}

#[test]
fn period_range_is_shown_with_inclusive_end_date() {
    let manifest = ExportManifest::new(
        ManifestScope::Room {
            id: "!a:x".to_owned(),
        },
        HistoryExportRange::Period {
            start_ms: 1_756_652_400_000,
            end_exclusive_ms: 1_759_244_400_000,
            time_zone: "Asia/Tokyo".to_owned(),
        },
        "General".to_owned(),
    );
    let page = String::from_utf8(render_index_page(
        &manifest,
        &labels(),
        1_758_758_400_000,
        "Asia/Tokyo",
    ))
    .unwrap();
    // 2025-09-01 00:00 JST to 2025-10-01 00:00 JST exclusive → 2025-09-01 to 2025-09-30.
    assert!(page.contains("2025-09-01 to 2025-09-30"), "{page}");
}
