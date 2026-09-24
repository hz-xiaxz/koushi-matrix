//! Names inside an export folder.
//!
//! Every name is derived from untrusted text (room names, attachment file
//! names), so each one is reduced to a single portable path component:
//! separators and Windows-forbidden characters become `_`, leading dots and
//! trailing dots or spaces are removed, `..` never survives, and stems are
//! capped so nested paths stay within platform limits.

/// Longest stem kept from a room name or attachment file name: 120
/// characters and at most 150 UTF-8 bytes, so a name plus its prefix, suffix,
/// or extension stays under the common 255-byte file-name limit even for
/// CJK text.
pub(crate) const NAME_STEM_MAX_CHARS: usize = 120;
const NAME_STEM_MAX_BYTES: usize = 150;

const EXTENSION_MAX_CHARS: usize = 10;

/// `<room name> (<8 hex digits>)`. The suffix is a stable hash of the room id
/// so rooms with the same name do not collide.
pub(crate) fn room_folder_name(display_name: &str, room_id: &str) -> String {
    let stem = portable_stem(display_name).unwrap_or_else(|| "room".to_owned());
    format!("{stem} ({})", &fnv1a_hex(room_id)[..8])
}

/// The hidden folder a room is built in before it is renamed into place.
pub(crate) fn partial_folder_name(final_name: &str) -> String {
    format!(".{final_name}.partial")
}

/// `<stem> - Export <YYYY-MM-DD>`, the folder created inside the chosen
/// parent directory for a new export.
pub(crate) fn export_folder_name(stem: &str, civil_date: &str) -> String {
    let stem = portable_stem(stem).unwrap_or_else(|| "Koushi".to_owned());
    format!("{stem} - Export {civil_date}")
}

/// `<sequence>_<name>.<ext>`: the chronological sequence keeps names unique
/// within a room. The extension comes from the original name, or from the
/// MIME type when the name has none.
pub(crate) fn attachment_file_name(
    sequence: u32,
    original: Option<&str>,
    mimetype: Option<&str>,
) -> String {
    let cleaned = original.map(clean_component).unwrap_or_default();
    let (stem, extension) = match cleaned.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() && is_extension(extension) => {
            (stem.to_owned(), Some(extension.to_ascii_lowercase()))
        }
        _ => (cleaned.clone(), None),
    };
    let stem = cap_stem(&stem).unwrap_or_else(|| "file".to_owned());
    let extension = extension.or_else(|| mimetype.and_then(extension_for_mime).map(str::to_owned));
    match extension {
        Some(extension) => format!("{sequence:04}_{stem}.{extension}"),
        None => format!("{sequence:04}_{stem}"),
    }
}

/// `<sequence>.jpg` in the room's `thumbs/` folder.
pub(crate) fn thumbnail_file_name(sequence: u32) -> String {
    format!("{sequence:04}.jpg")
}

/// 64-bit FNV-1a as 16 lowercase hex digits. Unlike `DefaultHasher`, the value
/// is fixed across Rust releases, so folder names survive app updates.
pub(crate) fn fnv1a_hex(input: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in input.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn portable_stem(input: &str) -> Option<String> {
    cap_stem(&clean_component(input))
}

/// Replace unsafe characters and remove every `..`.
fn clean_component(input: &str) -> String {
    let replaced: String = input
        .trim()
        .chars()
        .map(|character| match character {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            character if character.is_control() => '_',
            other => other,
        })
        .collect();
    let mut cleaned = replaced.trim_start_matches(['.', ' ']).to_owned();
    while cleaned.contains("..") {
        cleaned = cleaned.replace("..", "_");
    }
    cleaned
}

fn cap_stem(stem: &str) -> Option<String> {
    let mut capped = String::new();
    for character in stem.chars().take(NAME_STEM_MAX_CHARS) {
        if capped.len() + character.len_utf8() > NAME_STEM_MAX_BYTES {
            break;
        }
        capped.push(character);
    }
    let trimmed = capped.trim_end_matches(['.', ' ']);
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

fn is_extension(candidate: &str) -> bool {
    (1..=EXTENSION_MAX_CHARS).contains(&candidate.chars().count())
        && candidate
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
}

fn extension_for_mime(mimetype: &str) -> Option<&'static str> {
    let essence = mimetype.split(';').next()?.trim().to_ascii_lowercase();
    Some(match essence.as_str() {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/svg+xml" => "svg",
        "image/heic" => "heic",
        "application/pdf" => "pdf",
        "application/zip" => "zip",
        "text/plain" => "txt",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "video/quicktime" => "mov",
        "audio/ogg" => "ogg",
        "audio/mpeg" => "mp3",
        "audio/mp4" => "m4a",
        _ => return None,
    })
}
