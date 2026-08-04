use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, de};

use crate::composer_shortcuts::format_markdown_subset_html;
use crate::{ComposerFormattingOptions, MentionIntent, MentionTarget};

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum ComposerInline {
    Text {
        text: String,
    },
    Mention {
        target: MentionTarget,
        display_label: String,
    },
}

impl fmt::Debug for ComposerInline {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text { text } => formatter
                .debug_struct("Text")
                .field("bytes", &text.len())
                .finish(),
            Self::Mention { .. } => formatter.write_str("Mention(..)"),
        }
    }
}

#[derive(Clone, Eq, PartialEq, Serialize)]
pub struct ComposerDocument {
    pub version: u8,
    pub inlines: Vec<ComposerInline>,
}

impl ComposerDocument {
    pub const VERSION: u8 = 2;

    pub fn new(inlines: Vec<ComposerInline>) -> Self {
        Self {
            version: Self::VERSION,
            inlines: normalize_inlines(inlines),
        }
    }

    pub fn from_plain_text(text: impl Into<String>) -> Self {
        let text = text.into();
        Self::new(
            (!text.is_empty())
                .then_some(ComposerInline::Text { text })
                .into_iter()
                .collect(),
        )
    }

    pub fn plain_body(&self) -> String {
        let mut body = String::new();
        for inline in &self.inlines {
            match inline {
                ComposerInline::Text { text } => body.push_str(text),
                ComposerInline::Mention { display_label, .. } => {
                    body.push('@');
                    body.push_str(display_label);
                }
            }
        }
        body
    }

    pub fn formatted_body(&self) -> Option<String> {
        self.formatted_body_with_options(ComposerFormattingOptions::default())
    }

    pub fn formatted_body_with_options(
        &self,
        options: ComposerFormattingOptions,
    ) -> Option<String> {
        let marker_prefix = mention_marker_prefix(self);
        let mut source = String::new();
        let mut mentions = Vec::new();
        for inline in &self.inlines {
            match inline {
                ComposerInline::Text { text } => source.push_str(text),
                ComposerInline::Mention {
                    target,
                    display_label,
                } => {
                    let marker = format!("{marker_prefix}{};", mentions.len());
                    source.push_str(&marker);
                    mentions.push((marker, target, display_label));
                }
            }
        }
        let (mut html, markdown_changed) = format_markdown_subset_html(&source, options);
        for (marker, target, display_label) in &mentions {
            html = html.replace(marker, &mention_html(target, display_label));
        }
        (markdown_changed || !mentions.is_empty()).then_some(html)
    }

    pub fn mention_intent(&self) -> MentionIntent {
        let mut targets = Vec::new();
        for inline in &self.inlines {
            let ComposerInline::Mention { target, .. } = inline else {
                continue;
            };
            if !targets
                .iter()
                .any(|existing| same_mention_identity(existing, target))
            {
                targets.push(target.clone());
            }
        }
        MentionIntent { targets }
    }

    pub fn is_empty(&self) -> bool {
        self.inlines.is_empty()
    }

    pub fn len(&self) -> usize {
        self.inlines
            .iter()
            .map(|inline| match inline {
                ComposerInline::Text { text } => text.len(),
                ComposerInline::Mention { display_label, .. } => 1 + display_label.len(),
            })
            .sum()
    }

    pub fn truncated_to_plain_bytes(&self, maximum: usize) -> Self {
        let mut remaining = maximum;
        let mut inlines = Vec::new();
        for inline in &self.inlines {
            match inline {
                ComposerInline::Text { text } => {
                    let end = text
                        .char_indices()
                        .map(|(index, _)| index)
                        .take_while(|index| *index <= remaining)
                        .last()
                        .unwrap_or(0);
                    let end = if text.len() <= remaining {
                        text.len()
                    } else {
                        end
                    };
                    if end > 0 {
                        inlines.push(ComposerInline::Text {
                            text: text[..end].to_owned(),
                        });
                        remaining -= end;
                    }
                    if end < text.len() {
                        break;
                    }
                }
                mention @ ComposerInline::Mention { display_label, .. } => {
                    let bytes = 1 + display_label.len();
                    if bytes > remaining {
                        break;
                    }
                    inlines.push(mention.clone());
                    remaining -= bytes;
                }
            }
        }
        Self::new(inlines)
    }
}

fn mention_marker_prefix(document: &ComposerDocument) -> String {
    let mut prefix = "\u{fdd0}koushi-mention-".to_owned();
    while document.inlines.iter().any(|inline| match inline {
        ComposerInline::Text { text } => text.contains(&prefix),
        ComposerInline::Mention { display_label, .. } => display_label.contains(&prefix),
    }) {
        prefix.push('x');
    }
    prefix
}

fn mention_html(target: &MentionTarget, display_label: &str) -> String {
    let mut label = String::from("@");
    label.push_str(display_label);
    let escaped_label = escaped_html(&label);
    match target {
        MentionTarget::User { user_id, .. } => format!(
            "<a href=\"https://matrix.to/#/{}\">{escaped_label}</a>",
            percent_encode_matrix_identifier(user_id)
        ),
        MentionTarget::Room { room_id, .. } => format!(
            "<a href=\"https://matrix.to/#/{}\">{escaped_label}</a>",
            percent_encode_matrix_identifier(room_id)
        ),
        MentionTarget::RoomMention { .. } => {
            format!("<span data-mx-mention>{escaped_label}</span>")
        }
    }
}

fn percent_encode_matrix_identifier(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            use std::fmt::Write as _;
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

fn escaped_html(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

impl Default for ComposerDocument {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

impl From<String> for ComposerDocument {
    fn from(text: String) -> Self {
        Self::from_plain_text(text)
    }
}

impl From<&str> for ComposerDocument {
    fn from(text: &str) -> Self {
        Self::from_plain_text(text)
    }
}

impl fmt::Debug for ComposerDocument {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ComposerDocument")
            .field("version", &self.version)
            .field("inline_count", &self.inlines.len())
            .field("mention_count", &self.mention_intent().targets.len())
            .finish()
    }
}

impl<'de> Deserialize<'de> for ComposerDocument {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireDocument {
            version: u8,
            inlines: Vec<ComposerInline>,
        }

        let wire = WireDocument::deserialize(deserializer)?;
        if wire.version != Self::VERSION {
            return Err(de::Error::custom("unsupported composer document version"));
        }
        Ok(Self::new(wire.inlines))
    }
}

fn normalize_inlines(inlines: Vec<ComposerInline>) -> Vec<ComposerInline> {
    let mut normalized = Vec::with_capacity(inlines.len());
    for inline in inlines {
        match inline {
            ComposerInline::Text { text } if text.is_empty() => {}
            ComposerInline::Text { text } => match normalized.last_mut() {
                Some(ComposerInline::Text { text: previous }) => previous.push_str(&text),
                _ => normalized.push(ComposerInline::Text { text }),
            },
            mention @ ComposerInline::Mention { .. } => normalized.push(mention),
        }
    }
    normalized
}

fn same_mention_identity(left: &MentionTarget, right: &MentionTarget) -> bool {
    match (left, right) {
        (MentionTarget::User { user_id: left, .. }, MentionTarget::User { user_id: right, .. }) => {
            left == right
        }
        (MentionTarget::Room { room_id: left, .. }, MentionTarget::Room { room_id: right, .. }) => {
            left == right
        }
        (MentionTarget::RoomMention { .. }, MentionTarget::RoomMention { .. }) => true,
        _ => false,
    }
}
