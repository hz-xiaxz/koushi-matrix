#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SendFailureDiagnostic {
    pub(crate) reason: &'static str,
    pub(crate) recoverable: bool,
}

pub(crate) fn classify_send_failure(
    error: &matrix_sdk::Error,
    recoverable: bool,
) -> SendFailureDiagnostic {
    use matrix_sdk::Error;

    let reason = match error {
        Error::SecureBackupRequired | Error::SecureBackupSendAdmissionClosed => {
            "secure_backup_required"
        }
        Error::Http(error) => classify_http_failure(error),
        Error::ConcurrentRequestFailed => "concurrent_request_failed",
        Error::BadCryptoStoreState
        | Error::NoOlmMachine
        | Error::CryptoStoreError(_)
        | Error::OlmError(_)
        | Error::MegolmError(_)
        | Error::DecryptorError(_)
        | Error::BackupNotEnabled => "crypto",
        Error::StateStore(_)
        | Error::EventCacheStore(_)
        | Error::MediaStore(_)
        | Error::CrossProcessLockError(_) => "store",
        _ => "other",
    };

    SendFailureDiagnostic {
        reason,
        recoverable,
    }
}

fn classify_http_failure(error: &matrix_sdk::HttpError) -> &'static str {
    let is_timeout = matches!(error, matrix_sdk::HttpError::Reqwest(error) if error.is_timeout());
    http_failure_token(is_timeout)
}

fn http_failure_token(is_timeout: bool) -> &'static str {
    if is_timeout { "timeout" } else { "http" }
}

#[cfg(test)]
mod tests {
    use super::{classify_send_failure, http_failure_token};

    #[test]
    fn classifies_secure_backup_without_exposing_raw_error() {
        let diagnostic = classify_send_failure(&matrix_sdk::Error::SecureBackupRequired, true);
        assert_eq!(diagnostic.reason, "secure_backup_required");
        assert!(diagnostic.recoverable);
    }

    #[test]
    fn classifies_concurrent_and_fallback_errors() {
        let concurrent = classify_send_failure(&matrix_sdk::Error::ConcurrentRequestFailed, true);
        assert_eq!(concurrent.reason, "concurrent_request_failed");

        let fallback = classify_send_failure(&matrix_sdk::Error::InsufficientData, false);
        assert_eq!(fallback.reason, "other");
        assert!(!fallback.recoverable);
    }

    #[test]
    fn distinguishes_http_timeouts_without_exposing_transport_details() {
        assert_eq!(http_failure_token(true), "timeout");
        assert_eq!(http_failure_token(false), "http");
    }
}
