use super::layout::*;

#[test]
fn room_folder_is_sanitized_capped_and_hash_suffixed() {
    let hostile = "a/b\\c:d*".repeat(40);
    let name = room_folder_name(&hostile, "!room:example.org");
    assert!(!name.contains(['/', '\\', ':', '*']));
    let (stem, suffix) = name.rsplit_once(" (").unwrap();
    assert!(stem.chars().count() <= 120);
    assert_eq!(suffix.len(), 9, "8 hex digits and ')'");
    assert_eq!(name, room_folder_name(&hostile, "!room:example.org"));
    assert_ne!(name, room_folder_name(&hostile, "!other:example.org"));
}

#[test]
fn room_folder_hash_is_stable_across_builds() {
    // FNV-1a 64 of "!r:x", first 8 hex digits. Changing the hash would orphan
    // folders of exports that were started by an older build.
    assert_eq!(
        room_folder_name("Lab", "!r:x"),
        format!("Lab ({})", &fnv1a_hex("!r:x")[..8])
    );
    assert_eq!(fnv1a_hex(""), "cbf29ce484222325");
}

#[test]
fn attachment_names_are_sequenced_and_contained() {
    assert_eq!(
        attachment_file_name(1, Some("report.pdf"), None),
        "0001_report.pdf"
    );
    let hostile = attachment_file_name(12, Some("../../etc/passwd"), None);
    assert!(
        !hostile.contains('/') && !hostile.contains(".."),
        "{hostile}"
    );
    assert!(hostile.starts_with("0012_"));
    assert_eq!(
        attachment_file_name(3, None, Some("image/png")),
        "0003_file.png"
    );
    let long = attachment_file_name(4, Some(&format!("{}.jpg", "x".repeat(400))), None);
    assert!(long.ends_with(".jpg"));
    assert!(
        long.chars().count() <= 5 + 120 + 4,
        "{}",
        long.chars().count()
    );
    assert_eq!(
        attachment_file_name(12345, Some("a.txt"), None),
        "12345_a.txt"
    );
}

#[test]
fn empty_or_dot_names_fall_back() {
    assert!(room_folder_name("  ", "!r:x").starts_with("room ("));
    assert!(room_folder_name("...", "!r:x").starts_with("room ("));
    assert_eq!(attachment_file_name(2, Some(".."), None), "0002_file");
    assert_eq!(
        attachment_file_name(2, Some(".hidden"), None),
        "0002_hidden"
    );
}

#[test]
fn partial_and_export_folder_names() {
    assert_eq!(
        partial_folder_name("Lab (0123abcd)"),
        ".Lab (0123abcd).partial"
    );
    assert_eq!(
        export_folder_name("My: Space", "2026-09-25"),
        "My_ Space - Export 2026-09-25"
    );
    assert_eq!(thumbnail_file_name(7), "0007.jpg");
}

#[test]
fn long_cjk_names_stay_within_the_file_name_byte_limit() {
    let japanese = "設計".repeat(200);
    let room = room_folder_name(&japanese, "!r:x");
    assert!(room.len() <= 255, "room folder is {} bytes", room.len());
    let attachment = attachment_file_name(9999, Some(&format!("{japanese}.pdf")), None);
    assert!(
        attachment.len() <= 255,
        "attachment is {} bytes",
        attachment.len()
    );
    assert!(attachment.ends_with(".pdf"));
    assert!(export_folder_name(&japanese, "2025-09-25").len() <= 255);
    // Still whole characters.
    assert!(room.is_char_boundary(room.len()));
}

#[test]
fn windows_reserved_stems_are_never_a_bare_name() {
    for reserved in ["CON", "nul", "COM1", "LPT9", "AUX", "PRN"] {
        assert_ne!(
            room_folder_name(reserved, "!r:x").to_ascii_uppercase(),
            reserved.to_ascii_uppercase()
        );
        assert!(attachment_file_name(1, Some(reserved), None).starts_with("0001_"));
    }
}
