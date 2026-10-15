//! Device-local notification delivery sinks (#1427).
//!
//! The core owns *whether* a notification is delivered, *when* (the dwell and the
//! minimum interval), and *what it says*; this crate owns *how* a sink puts that
//! message on the wire. A sink sends exactly the request it is given with the secret
//! it is given, once, and reports a typed outcome. It must not choose recipients,
//! coalesce, suppress, or build content (state-ownership.md, "Threads and attention").
//!
//! Three kinds of sink reach the wire today: Resend's HTTPS API, which needs no mailbox of
//! the user's own; SMTP against a mailbox the user already has — the cheap relays, and the
//! providers in [`presets`] (QQ Mail, Gmail, iCloud Mail) whose authorization code or app
//! password is the only credential a third-party client may use; and ServerChan Turbo,
//! which relays to the SendKey owner's WeChat (#1460).
//!
//! Crate ownership (engineering rules, "Crate Ownership And Projection Authority"):
//! `koushi-sdk` owns SDK-facing Matrix primitives, `koushi-store` owns credential and
//! encrypted-file mechanics, and the desktop app stays a thin platform adapter, so a
//! product-facing outbound sender that is neither Matrix nor platform lives here.

mod presets;
mod resend;
mod serverchan;
mod smtp;

use std::fmt;

#[cfg(test)]
mod smtp_tests;

#[cfg(test)]
mod tests;

pub use presets::{SMTP_PRESETS, SmtpPreset, preset};
pub use resend::{RESEND_ENDPOINT, ResendSink};
pub use serverchan::{
    SERVERCHAN_ENDPOINT, ServerChanSendKeyError, ServerChanSink, validate_serverchan_sendkey,
};
pub use smtp::{SmtpSettings, SmtpSink, SmtpTls};

/// One notification this device has already decided to deliver.
///
/// `subject` and `body` are built by the core from the attention projection: a safe
/// room label, the kind and the counts, and message text only when the user turned the
/// plaintext prefix on with a stored consent. A sink never adds content of its own.
#[derive(Clone, Eq, PartialEq)]
pub struct NotificationDeliveryRequest {
    pub subject: String,
    pub body: String,
    pub destination: String,
    pub from_address: String,
    /// Display name this device's client uses as the sender name.
    pub sender_name: String,
    /// Stable per-attempt id, so a retry of the same delivery cannot send twice.
    pub idempotency_key: String,
}

/// `Debug` redacts the subject, the body, and both addresses.
///
/// This is the same payload `koushi_core::notification_delivery::NotificationDeliveryIntent`
/// carries one layer up, and it earns the same treatment for the same reason: the body may
/// hold message text under a prefix consent, and the addresses are personal identifiers, so
/// a struct dump must not become a log line or a diagnostics entry. The idempotency key
/// stays visible because it names no person and no message, and it is what a retry is
/// correlated by.
impl fmt::Debug for NotificationDeliveryRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NotificationDeliveryRequest")
            .field("subject", &"Subject(..)")
            .field("body", &"Body(..)")
            .field("destination", &"Destination(..)")
            .field("from_address", &"FromAddress(..)")
            .field("sender_name", &"SenderName(..)")
            .field("idempotency_key", &self.idempotency_key)
            .finish()
    }
}

/// Why one delivery attempt did not succeed.
///
/// The variants are what the product can honestly say. A provider 403 covers both a
/// sender that is not permitted and an account under review, and the app must not
/// claim to know which, so it is one typed state rather than an invented distinction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotificationDeliveryFailure {
    /// The provider rejected the credential (401).
    InvalidSecret,
    /// The provider refused this sender or this account (403).
    NotPermitted,
    /// The provider refused the request itself (another 4xx, such as 422).
    Rejected,
    /// The provider asked us to slow down (429), with its own delay when it gave one.
    RateLimited { retry_after_secs: Option<u64> },
    /// No verdict was reached: a connect error, a TLS error, a timeout, a connection
    /// closed mid-delivery, a reply the sink cannot classify, or a provider 5xx that
    /// says nothing about the request itself.
    Transport,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotificationDeliveryOutcome {
    /// The provider accepted the request. No sink can prove the message reached the
    /// recipient, so this is never reported as received.
    Accepted,
    Failed(NotificationDeliveryFailure),
}

impl NotificationDeliveryOutcome {
    pub fn is_accepted(self) -> bool {
        matches!(self, Self::Accepted)
    }

    pub fn failure(self) -> Option<NotificationDeliveryFailure> {
        match self {
            Self::Accepted => None,
            Self::Failed(failure) => Some(failure),
        }
    }
}

/// The sinks this build can deliver through.
///
/// A sink is an enum rather than a trait object for the same reason the credential
/// backend is: the set is closed, and every variant must be implemented deliberately.
#[derive(Clone)]
pub enum NotificationSink {
    Resend(ResendSink),
    Smtp(SmtpSink),
    ServerChan(ServerChanSink),
}

impl NotificationSink {
    /// Deliver one notification through this sink.
    ///
    /// The typed outcome is the only thing that leaves. Raw provider bodies, the
    /// endpoint or relay host, and transport errors stay inside the sink, because a
    /// settings snapshot, a diagnostics export, or a log line must never carry them
    /// (engineering rules, "Logging and Diagnostics").
    pub async fn send(
        &self,
        secret: &koushi_key::NotificationSinkSecret,
        request: &NotificationDeliveryRequest,
    ) -> NotificationDeliveryOutcome {
        match self {
            Self::Resend(sink) => sink.send(secret, request).await,
            Self::Smtp(sink) => sink.send(secret, request).await,
            Self::ServerChan(sink) => sink.send(secret, request).await,
        }
    }
}
