use koushi_state::{HistoryExportRange, HistoryExportRoomCounts};

use super::manifest::*;

fn scope() -> ManifestScope {
    ManifestScope::Space {
        id: "!space:example.org".to_owned(),
    }
}

fn manifest() -> ExportManifest {
    let mut manifest = ExportManifest::new(scope(), HistoryExportRange::AllAvailable, "Lab".to_owned());
    manifest.upsert_room("!a:example.org", "A", "A (00000001)", ManifestRoomStatus::Completed, HistoryExportRoomCounts {
        exported_events: 3,
        ..HistoryExportRoomCounts::default()
    });
    manifest
}

#[test]
fn fresh_when_no_manifest() {
    assert_eq!(match_manifest(None, &scope(), &HistoryExportRange::AllAvailable), ManifestMatch::Fresh);
}

#[test]
fn resume_when_scope_and_range_match() {
    let bytes = manifest().to_bytes();
    assert_eq!(
        match_manifest(Some(&bytes), &scope(), &HistoryExportRange::AllAvailable),
        ManifestMatch::Resume(manifest())
    );
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["format"], "koushi-history-export");
    assert_eq!(value["version"], 1);
    assert_eq!(value["rooms"][0]["status"], "completed");
}

#[test]
fn mismatch_on_other_space_or_range() {
    let bytes = manifest().to_bytes();
    let other = ManifestScope::Space {
        id: "!other:example.org".to_owned(),
    };
    assert_eq!(match_manifest(Some(&bytes), &other, &HistoryExportRange::AllAvailable), ManifestMatch::Mismatch);
    let period = HistoryExportRange::Period {
        start_ms: 1,
        end_exclusive_ms: 2,
        time_zone: "UTC".to_owned(),
    };
    assert_eq!(match_manifest(Some(&bytes), &scope(), &period), ManifestMatch::Mismatch);
}

#[test]
fn mismatch_on_unknown_format_version_or_corrupt_json() {
    let mut value: serde_json::Value = serde_json::from_slice(&manifest().to_bytes()).unwrap();
    value["version"] = 2.into();
    let newer = serde_json::to_vec(&value).unwrap();
    assert_eq!(match_manifest(Some(&newer), &scope(), &HistoryExportRange::AllAvailable), ManifestMatch::Mismatch);
    value["version"] = 1.into();
    value["format"] = "other".into();
    let foreign = serde_json::to_vec(&value).unwrap();
    assert_eq!(match_manifest(Some(&foreign), &scope(), &HistoryExportRange::AllAvailable), ManifestMatch::Mismatch);
    assert_eq!(match_manifest(Some(b"{not json"), &scope(), &HistoryExportRange::AllAvailable), ManifestMatch::Mismatch);
}

#[test]
fn upsert_updates_in_place_and_keeps_order() {
    let mut manifest = manifest();
    manifest.upsert_room("!b:example.org", "B", "B (00000002)", ManifestRoomStatus::Pending, HistoryExportRoomCounts::default());
    manifest.upsert_room("!a:example.org", "A", "A (00000001)", ManifestRoomStatus::Failed, HistoryExportRoomCounts::default());
    let ids: Vec<_> = manifest.rooms.iter().map(|room| (room.room_id.as_str(), room.status)).collect();
    assert_eq!(ids, vec![("!a:example.org", ManifestRoomStatus::Failed), ("!b:example.org", ManifestRoomStatus::Pending)]);
    assert_eq!(manifest.room("!b:example.org").unwrap().folder, "B (00000002)");
}
