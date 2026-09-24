//! Matrix `formatted_body` sanitizing shared by the timeline projection and
//! the history export pages.

use matrix_sdk::ruma::html::{Html, SanitizerConfig};

/// Elements removed with their content in every sanitized body.
const REMOVED_ELEMENTS: [&str; 2] = ["script", "style"];

/// Sanitize with ruma's compatibility allow-list, dropping reply fallbacks,
/// scripts and styles. `extra_removed` names further elements to drop with
/// their content.
pub(crate) fn sanitize_matrix_html(body: &str, extra_removed: &[&'static str]) -> String {
    let html = Html::parse(body);
    html.sanitize_with(
        &SanitizerConfig::compat()
            .remove_reply_fallback()
            .remove_elements(REMOVED_ELEMENTS.iter().chain(extra_removed).copied()),
    );
    html.to_string()
}
