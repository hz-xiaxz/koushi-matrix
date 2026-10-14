//! Device-local notification delivery sinks (#1427).
//!
//! The core owns *whether* a notification is delivered, *when* (the dwell and the
//! minimum interval), and *what it says*; this crate owns *how* a sink puts that
//! message on the wire. A sink sends exactly the request it is given with the secret
//! it is given, once, and reports a typed outcome. It must not choose recipients,
//! coalesce, suppress, or build content (state-ownership.md, "Threads and attention").
//!
//! Crate ownership (engineering rules, "Crate Ownership And Projection Authority"):
//! `koushi-sdk` owns SDK-facing Matrix primitives, `koushi-store` owns credential and
//! encrypted-file mechanics, and the desktop app stays a thin platform adapter, so a
//! product-facing outbound sender that is neither Matrix nor platform lives here.

mod resend;

#[cfg(test)]
mod tests;

pub use resend::{RESEND_ENDPOINT, ResendSink};

/// One notification this device has already decided to deliver.
///
/// `subject` and `body` are built by the core from the attention projection: a safe
/// room label, the kind and the counts, and message text only when the user turned the
/// plaintext prefix on with a stored consent. A sink never adds content of its own.
#[derive(Clone, Debug, Eq, PartialEq)]
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
    /// No verdict was reached: connect error, TLS error, timeout, or a 5xx.
    Transport,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotificationDeliveryOutcome {
    Delivered,
    Failed(NotificationDeliveryFailure),
}

impl NotificationDeliveryOutcome {
    pub fn is_delivered(self) -> bool {
        matches!(self, Self::Delivered)
    }

    pub fn failure(self) -> Option<NotificationDeliveryFailure> {
        match self {
            Self::Delivered => None,
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
}

impl NotificationSink {
    /// Deliver one notification through this sink.
    ///
    /// The typed outcome is the only thing that leaves. Raw provider bodies, the
    /// endpoint, and transport errors stay inside the sink, because a settings
    /// snapshot, a diagnostics export, or a log line must never carry them
    /// (engineering rules, "Logging and Diagnostics").
    pub async fn send(
        &self,
        secret: &koushi_key::NotificationSinkSecret,
        request: &NotificationDeliveryRequest,
    ) -> NotificationDeliveryOutcome {
        match self {
            Self::Resend(sink) => sink.send(secret, request).await,
        }
    }
}
