use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum E2eeRecoveryState {
    Unknown,
    Enabled,
    Disabled,
    Incomplete,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct E2eeTrustState {
    pub verification: VerificationFlowState,
    pub cross_signing: CrossSigningStatus,
    pub key_backup: KeyBackupStatus,
    pub identity_reset: IdentityResetState,
    #[serde(default)]
    pub key_management: E2eeKeyManagementState,
    pub devices: Vec<DeviceTrustSummary>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct E2eeKeyManagementState {
    pub room_key_export: RoomKeyExportState,
    pub room_key_import: RoomKeyImportState,
    pub secure_backup_setup: SecureBackupSetupState,
    pub passphrase_change: SecureBackupPassphraseChangeState,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RoomKeyExportState {
    #[default]
    Idle,
    Exporting {
        request_id: u64,
    },
    Exported {
        request_id: u64,
        exported_sessions: Option<u64>,
    },
    Failed {
        request_id: u64,
        #[serde(rename = "failureKind")]
        kind: TrustOperationFailureKind,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RoomKeyImportState {
    #[default]
    Idle,
    Importing {
        request_id: u64,
    },
    Imported {
        request_id: u64,
        imported_count: u64,
        total_count: u64,
    },
    Failed {
        request_id: u64,
        #[serde(rename = "failureKind")]
        kind: TrustOperationFailureKind,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SecureBackupSetupState {
    #[default]
    Idle,
    SettingUp {
        request_id: u64,
    },
    /// The generated recovery key is revealed on screen until the user
    /// explicitly confirms it was saved (#927). Together with
    /// `SecureBackupPassphraseChangeState::Changed`, this is the only state
    /// that carries recovery-key material.
    RecoveryKeyReady {
        request_id: u64,
        recovery_key: RecoveryKeyMaterial,
        delivery: RecoveryKeyDeliveryState,
        /// The saved confirmation could not be persisted and the reveal was
        /// restored; the user is asked to confirm again.
        #[serde(default)]
        confirmation_failed: bool,
    },
    Enabled {
        request_id: u64,
    },
    Failed {
        request_id: u64,
        #[serde(rename = "failureKind")]
        kind: TrustOperationFailureKind,
    },
}

/// Outcome of the optional "Save to file" action for a revealed key.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RecoveryKeyDeliveryState {
    #[default]
    NotWritten,
    Written,
    WriteFailed,
}

/// A recovery key generated (or reset) by the SDK for on-screen reveal.
///
/// Privacy contract (#927): the value may cross to the WebView only through
/// the live `RecoveryKeyReady`/`Changed` snapshot projection so the user can
/// read or copy it. `Debug` is redacted, the allocation is zeroized on drop,
/// and it must never enter diagnostics, logs, QA tokens, or persisted state.
#[derive(Clone, Eq, PartialEq)]
pub struct RecoveryKeyMaterial(zeroize::Zeroizing<String>);

impl RecoveryKeyMaterial {
    pub fn new(value: impl Into<String>) -> Self {
        Self(zeroize::Zeroizing::new(value.into()))
    }

    pub fn expose_secret(&self) -> &str {
        self.0.as_str()
    }
}

impl std::fmt::Debug for RecoveryKeyMaterial {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RecoveryKeyMaterial(..)")
    }
}

impl Serialize for RecoveryKeyMaterial {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.expose_secret())
    }
}

impl<'de> Deserialize<'de> for RecoveryKeyMaterial {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(Self::new)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SecureBackupPassphraseChangeState {
    #[default]
    Idle,
    Changing {
        request_id: u64,
    },
    /// The new recovery key is revealed until the user confirms it was saved.
    Changed {
        request_id: u64,
        recovery_key: RecoveryKeyMaterial,
        delivery: RecoveryKeyDeliveryState,
    },
    Failed {
        request_id: u64,
        #[serde(rename = "failureKind")]
        kind: TrustOperationFailureKind,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum VerificationFlowState {
    #[default]
    Idle,
    Requested {
        request_id: u64,
        target: VerificationTarget,
        /// Who sent the request. Only an incoming request (`Them`) can be
        /// accepted; our own request waits for the other side (#1024).
        #[serde(default)]
        initiator: VerificationInitiator,
    },
    Accepted {
        request_id: u64,
        target: VerificationTarget,
        #[serde(default)]
        initiator: VerificationInitiator,
    },
    SasPresented {
        request_id: u64,
        target: VerificationTarget,
        emojis: Vec<SasEmoji>,
    },
    Confirming {
        request_id: u64,
        target: VerificationTarget,
        emojis: Vec<SasEmoji>,
    },
    Done {
        request_id: u64,
        target: VerificationTarget,
    },
    Failed {
        request_id: u64,
        target: VerificationTarget,
        #[serde(rename = "failureKind")]
        kind: TrustOperationFailureKind,
    },
}

impl VerificationFlowState {
    /// A flow is in progress: another request cannot start until it settles
    /// (`Done`/`Failed`) or is cancelled (`Idle`).
    pub fn is_in_progress(&self) -> bool {
        matches!(
            self,
            Self::Requested { .. }
                | Self::Accepted { .. }
                | Self::SasPresented { .. }
                | Self::Confirming { .. }
        )
    }
}

/// Which side started a verification request.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VerificationInitiator {
    /// This device sent the request (for example **Verify user**).
    Us,
    /// Another device or user sent the request to us.
    #[default]
    Them,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct VerificationTarget {
    pub user_id: String,
    pub device_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SasEmoji {
    pub symbol: String,
    pub description: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CrossSigningStatus {
    #[default]
    Unknown,
    Missing,
    Bootstrapping {
        request_id: u64,
    },
    Trusted,
    NotTrusted,
    Failed {
        request_id: u64,
        #[serde(rename = "failureKind")]
        kind: TrustOperationFailureKind,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum KeyBackupStatus {
    #[default]
    Unknown,
    Disabled,
    Enabling {
        request_id: u64,
    },
    Enabled {
        version: String,
    },
    Restoring {
        request_id: u64,
        version: Option<String>,
        restored_rooms: u64,
        total_rooms: Option<u64>,
    },
    Failed {
        request_id: u64,
        #[serde(rename = "failureKind")]
        kind: TrustOperationFailureKind,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum IdentityResetState {
    #[default]
    Idle,
    Resetting {
        request_id: u64,
    },
    AwaitingAuth {
        request_id: u64,
        auth_type: IdentityResetAuthType,
    },
    Failed {
        request_id: u64,
        #[serde(rename = "failureKind")]
        kind: TrustOperationFailureKind,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IdentityResetAuthType {
    Uiaa,
    #[serde(rename = "oauth")]
    OAuth,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DeviceTrustSummary {
    pub user_id: String,
    pub device_id: String,
    pub trust_level: DeviceTrustLevel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DeviceTrustLevel {
    Unknown,
    Unverified,
    Verified,
    Blocked,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TrustOperationFailureKind {
    Cancelled,
    Mismatch,
    InvalidPassphrase,
    Network,
    Forbidden,
    Timeout,
    Sdk,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VerificationCancelReason {
    User,
    Mismatch,
}
