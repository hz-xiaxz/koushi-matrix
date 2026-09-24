//! Pure renderers for the exported pages.
//!
//! Every page is a function of files already in the export folder. Pages load
//! scripts and styles only from `assets/` in the same folder and never reach
//! the network; the CSP below enforces that.

use jiff::{Timestamp, tz::TimeZone};

pub(crate) mod index_page;
pub(crate) mod room_page;
mod style;

#[cfg(test)]
mod index_page_tests;
#[cfg(test)]
mod room_page_tests;
#[cfg(test)]
mod test_support;

#[cfg(test)]
pub(crate) fn test_labels() -> koushi_protocol::HistoryExportLabels {
    test_support::labels()
}

/// The math bootstrap written to `assets/koushi-math.js`.
pub(crate) const MATH_BOOTSTRAP: &[u8] = include_bytes!("../../../assets/koushi-math.js");

const CSP: &str = "default-src 'none'; script-src 'self' file:; style-src 'self' file: 'unsafe-inline'; font-src 'self' file:; img-src 'self' file:";

/// `<head>` shared by all pages; `assets` is the relative path to `assets/`.
fn head(out: &mut String, lang: &str, title: &str, assets: &str, math: bool) {
    out.push_str("<!DOCTYPE html>\n<html lang=\"");
    out.push_str(&escape(if lang.trim().is_empty() { "en" } else { lang }));
    out.push_str("\">\n<head>\n<meta charset=\"utf-8\">\n");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    out.push_str("<meta http-equiv=\"Content-Security-Policy\" content=\"");
    out.push_str(CSP);
    out.push_str("\">\n<title>");
    out.push_str(&escape(title));
    out.push_str("</title>\n");
    if math {
        out.push_str(&format!(
            "<link rel=\"stylesheet\" href=\"{assets}katex/katex.min.css\">\n\
             <script defer src=\"{assets}katex/katex.min.js\"></script>\n\
             <script defer src=\"{assets}koushi-math.js\"></script>\n"
        ));
    }
    out.push_str("<style>");
    out.push_str(style::STYLE);
    out.push_str("</style>\n</head>\n");
}

pub(crate) fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            other => escaped.push(other),
        }
    }
    escaped
}

/// Fill `{name}` placeholders, then HTML-escape the whole result.
fn label(template: &str, values: &[(&str, &str)]) -> String {
    let mut text = template.to_owned();
    for (name, value) in values {
        text = text.replace(&format!("{{{name}}}"), value);
    }
    escape(&text)
}

/// Percent-encode a relative path for an `href`/`src`, keeping `/`.
fn href_path(path: &str) -> String {
    let mut encoded = String::with_capacity(path.len());
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                encoded.push(char::from(byte));
            }
            other => encoded.push_str(&format!("%{other:02X}")),
        }
    }
    encoded
}

fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

fn zone(name: &str) -> TimeZone {
    TimeZone::get(name).unwrap_or(TimeZone::UTC)
}

/// `(YYYY-MM-DD, HH:MM, RFC 3339)` of a millisecond timestamp in `zone`.
fn civil(timestamp_ms: u64, zone: &TimeZone) -> (String, String, String) {
    let timestamp = i64::try_from(timestamp_ms)
        .ok()
        .and_then(|ms| Timestamp::from_millisecond(ms).ok())
        .unwrap_or(Timestamp::UNIX_EPOCH);
    let zoned = timestamp.to_zoned(zone.clone());
    (
        zoned.strftime("%Y-%m-%d").to_string(),
        zoned.strftime("%H:%M").to_string(),
        zoned.strftime("%Y-%m-%dT%H:%M:%S%:z").to_string(),
    )
}
