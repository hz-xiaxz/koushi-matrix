//! Top-level `index.html`: the table of contents of an export folder.

use koushi_protocol::HistoryExportLabels;
use koushi_state::HistoryExportRange;

use super::{civil, escape, head, href_path, label, zone};
use crate::room_history_export::manifest::{ExportManifest, ManifestRoomStatus};

/// One day in milliseconds; a period's exclusive end is shown as the day
/// before it.
const DAY_MS: u64 = 24 * 60 * 60 * 1000;

pub(crate) fn render_index_page(
    manifest: &ExportManifest,
    labels: &HistoryExportLabels,
    exported_at_ms: u64,
    time_zone: &str,
) -> Vec<u8> {
    let page_zone = zone(time_zone);
    let mut out = String::new();
    head(&mut out, &labels.lang, &manifest.title, "assets/", false);
    let (date, time, _) = civil(exported_at_ms, &page_zone);
    out.push_str("<body>\n<header>\n<h1>");
    out.push_str(&escape(&manifest.title));
    out.push_str("</h1>\n<p class=\"meta\">");
    out.push_str(&label(
        &labels.exported_at,
        &[("date", &format!("{date} {time}"))],
    ));
    out.push_str(" · ");
    out.push_str(&range_text(&manifest.range, labels));
    out.push_str("</p>\n</header>\n<main>\n<h2>");
    out.push_str(&escape(&labels.rooms_heading));
    out.push_str("</h2>\n<table>\n<tbody>\n");
    for room in &manifest.rooms {
        let name = if room.display_name.trim().is_empty() {
            &room.room_id
        } else {
            &room.display_name
        };
        out.push_str("<tr><td>");
        if room.status == ManifestRoomStatus::Completed {
            out.push_str(&format!(
                "<a href=\"{}\">{}</a>",
                href_path(&format!("rooms/{}/index.html", room.folder)),
                escape(name)
            ));
        } else {
            out.push_str(&escape(name));
        }
        out.push_str("</td><td>");
        out.push_str(&escape(match room.status {
            ManifestRoomStatus::Completed => &labels.status_completed,
            ManifestRoomStatus::Skipped => &labels.status_skipped,
            ManifestRoomStatus::Failed => &labels.status_failed,
            ManifestRoomStatus::Pending => &labels.status_pending,
        }));
        out.push_str("</td><td>");
        if room.status == ManifestRoomStatus::Completed {
            let counts = room.counts;
            let mut parts = vec![
                label(
                    &labels.events_count,
                    &[("count", &counts.exported_events.to_string())],
                ),
                label(
                    &labels.attachments_count,
                    &[("count", &counts.attachments_total.to_string())],
                ),
            ];
            if counts.attachments_failed > 0 {
                parts.push(label(
                    &labels.failed_attachments_count,
                    &[("count", &counts.attachments_failed.to_string())],
                ));
            }
            out.push_str(&parts.join(" · "));
        }
        out.push_str("</td></tr>\n");
    }
    out.push_str("</tbody>\n</table>\n</main>\n</body>\n</html>\n");
    out.into_bytes()
}

fn range_text(range: &HistoryExportRange, labels: &HistoryExportLabels) -> String {
    match range {
        HistoryExportRange::AllAvailable => escape(&labels.range_all),
        HistoryExportRange::Period {
            start_ms,
            end_exclusive_ms,
            time_zone,
        } => {
            let range_zone = zone(time_zone);
            let (start, _, _) = civil(*start_ms, &range_zone);
            let (end, _, _) = civil(end_exclusive_ms.saturating_sub(DAY_MS / 2), &range_zone);
            label(&labels.range_period, &[("start", &start), ("end", &end)])
        }
    }
}
