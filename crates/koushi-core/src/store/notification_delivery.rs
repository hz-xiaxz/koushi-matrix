//! #1427: the credential seam for device-local notification sinks.
//!
//! Each credential belongs to one full Matrix session identity and one validated sink ID.
//! No account-scoped operation falls back to the retired global namespace.
//!
//! No function here returns the secret to a caller that could log it: `load` hands back a
//! `NotificationSinkSecret`, which is zeroizing, redacts its own `Debug`, and is neither
//! `Serialize` nor `Display`. The product only ever learns whether a secret is *present*.

use koushi_key::{
    LocalSecretError, NotificationSinkSecret, NotificationSinkSecretId, is_missing_credential_error,
};

use super::{CoreFailure, StoreActor};
use koushi_protocol::SessionKeyId;

/// Why a delivery secret could not be written, read, or forgotten.
///
/// The caller renders this, so it is a typed value rather than a message: a sink id the
/// credential store could never key on, a secret whose shape the store would refuse, and
/// an unreachable store are three different things to say to the user.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotificationDeliverySecretError {
    /// The id is not the bounded lowercase charset a credential name may use.
    InvalidSinkId,
    /// The value is empty, oversized, or carries control characters.
    InvalidSecret,
    /// The credential store itself could not be reached.
    StoreUnavailable,
}

impl From<LocalSecretError> for NotificationDeliverySecretError {
    fn from(error: LocalSecretError) -> Self {
        match error {
            LocalSecretError::InvalidNotificationSinkSecretId => Self::InvalidSinkId,
            LocalSecretError::InvalidNotificationSinkSecret => Self::InvalidSecret,
            _ => Self::StoreUnavailable,
        }
    }
}

impl NotificationDeliverySecretError {
    /// The public failure to answer the command with. A refusal the user can fix is a
    /// delivery rejection; an unreachable store is the store failure the rest of the app
    /// already reports.
    pub fn to_core_failure(self) -> CoreFailure {
        use koushi_protocol::NotificationDeliveryFailureKind;
        match self {
            Self::InvalidSinkId => CoreFailure::NotificationDeliveryRejected {
                kind: NotificationDeliveryFailureKind::InvalidSinkId,
            },
            Self::InvalidSecret => CoreFailure::NotificationDeliveryRejected {
                kind: NotificationDeliveryFailureKind::InvalidSecret,
            },
            Self::StoreUnavailable => CoreFailure::StoreUnavailable,
        }
    }
}

impl StoreActor {
    /// Whether the credential store holds a secret for this sink.
    ///
    /// A missing secret is `Ok(false)`, not an error: "not configured yet" is the ordinary
    /// state of a sink the user has just added. Any other failure is a real store problem
    /// and is reported as such, so the product never reads a sink as configured because the
    /// keychain was locked.
    pub fn notification_sink_secret_configured(
        &self,
        key_id: &SessionKeyId,
        sink_id: &str,
    ) -> Result<bool, NotificationDeliverySecretError> {
        self.load_notification_sink_secret(key_id, sink_id)
            .map(|secret| secret.is_some())
    }

    /// Store one sink's secret, replacing any previous value.
    pub fn save_notification_sink_secret(
        &self,
        key_id: &SessionKeyId,
        sink_id: &str,
        secret: &str,
    ) -> Result<(), NotificationDeliverySecretError> {
        let id = NotificationSinkSecretId::for_account(key_id, sink_id)?;
        let secret = NotificationSinkSecret::new(secret)?;
        self.credential_store
            .save_notification_sink_secret(&id, &secret)
            .map_err(NotificationDeliverySecretError::from)
    }

    /// Read one sink's secret for one send.
    ///
    /// `Ok(None)` means this device holds no secret for the sink; the value, when there is
    /// one, is returned to the caller and never written to state, a log, or an event.
    pub fn load_notification_sink_secret(
        &self,
        key_id: &SessionKeyId,
        sink_id: &str,
    ) -> Result<Option<NotificationSinkSecret>, NotificationDeliverySecretError> {
        let id = NotificationSinkSecretId::for_account(key_id, sink_id)?;
        match self.credential_store.load_notification_sink_secret(&id) {
            Ok(secret) => Ok(Some(secret)),
            Err(error) if is_missing_credential_error(&error) => Ok(None),
            Err(error) => Err(NotificationDeliverySecretError::from(error)),
        }
    }

    /// Forget one sink's secret. Forgetting a secret that is not there succeeds, so the
    /// product can clear a sink without first asking whether it was ever configured.
    pub fn delete_notification_sink_secret(
        &self,
        key_id: &SessionKeyId,
        sink_id: &str,
    ) -> Result<(), NotificationDeliverySecretError> {
        let id = NotificationSinkSecretId::for_account(key_id, sink_id)?;
        match self.credential_store.delete_notification_sink_secret(&id) {
            Ok(()) => Ok(()),
            Err(error) if is_missing_credential_error(&error) => Ok(()),
            Err(error) => Err(NotificationDeliverySecretError::from(error)),
        }
    }
}

#[cfg(test)]
#[path = "notification_delivery/tests.rs"]
mod tests;
