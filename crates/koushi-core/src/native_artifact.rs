use std::{
    collections::{HashMap, hash_map::Entry},
    fmt,
    path::PathBuf,
    sync::Mutex,
};

use koushi_protocol::ids::RequestId;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum NativeArtifactKind {
    RoomKeyExportDestination,
    RoomKeyImportSource,
    RecoveryKeyDestination,
}

impl fmt::Display for NativeArtifactKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::RoomKeyExportDestination => "room_key_export_destination",
            Self::RoomKeyImportSource => "room_key_import_source",
            Self::RecoveryKeyDestination => "recovery_key_destination",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum NativeArtifactError {
    #[error("native artifact registration is unavailable")]
    Unavailable,
    #[error("native artifact registration is missing or mismatched")]
    Missing,
    #[error("native artifact registration already exists")]
    AlreadyRegistered,
}

pub trait NativeArtifactPort: Send + Sync {
    fn register(
        &self,
        request_id: RequestId,
        kind: NativeArtifactKind,
        path: PathBuf,
    ) -> Result<(), NativeArtifactError>;
    fn take(
        &self,
        request_id: RequestId,
        kind: NativeArtifactKind,
    ) -> Result<PathBuf, NativeArtifactError>;
    fn unregister(&self, request_id: RequestId, kind: NativeArtifactKind);
}

#[derive(Default)]
pub struct RejectingNativeArtifactPort;

impl NativeArtifactPort for RejectingNativeArtifactPort {
    fn register(
        &self,
        _request_id: RequestId,
        _kind: NativeArtifactKind,
        _path: PathBuf,
    ) -> Result<(), NativeArtifactError> {
        Err(NativeArtifactError::Unavailable)
    }

    fn take(
        &self,
        _request_id: RequestId,
        _kind: NativeArtifactKind,
    ) -> Result<PathBuf, NativeArtifactError> {
        Err(NativeArtifactError::Unavailable)
    }

    fn unregister(&self, _request_id: RequestId, _kind: NativeArtifactKind) {}
}

/// Narrow injectable registry used by the native adapter and Core test hooks.
/// Paths are only retained until the exact request/kind is consumed or removed.
pub struct NativeArtifactRegistry {
    entries: Mutex<HashMap<(RequestId, NativeArtifactKind), PathBuf>>,
}

impl fmt::Debug for NativeArtifactRegistry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NativeArtifactRegistry")
            .field(
                "registered_count",
                &self
                    .entries
                    .lock()
                    .map(|entries| entries.len())
                    .unwrap_or(0),
            )
            .finish()
    }
}

impl Default for NativeArtifactRegistry {
    fn default() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }
}

impl NativeArtifactRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.entries
            .lock()
            .expect("native artifact registry lock")
            .is_empty()
    }
}

impl NativeArtifactPort for NativeArtifactRegistry {
    fn register(
        &self,
        request_id: RequestId,
        kind: NativeArtifactKind,
        path: PathBuf,
    ) -> Result<(), NativeArtifactError> {
        let mut entries = self.entries.lock().expect("native artifact registry lock");
        match entries.entry((request_id, kind)) {
            Entry::Vacant(entry) => {
                entry.insert(path);
                Ok(())
            }
            Entry::Occupied(_) => Err(NativeArtifactError::AlreadyRegistered),
        }
    }

    fn take(
        &self,
        request_id: RequestId,
        kind: NativeArtifactKind,
    ) -> Result<PathBuf, NativeArtifactError> {
        self.entries
            .lock()
            .expect("native artifact registry lock")
            .remove(&(request_id, kind))
            .ok_or(NativeArtifactError::Missing)
    }

    fn unregister(&self, request_id: RequestId, kind: NativeArtifactKind) {
        self.entries
            .lock()
            .expect("native artifact registry lock")
            .remove(&(request_id, kind));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(sequence: u64) -> RequestId {
        RequestId {
            connection_id: koushi_protocol::ids::RuntimeConnectionId(1),
            sequence,
        }
    }

    #[test]
    fn registry_consumes_only_exact_request_and_kind() {
        let registry = NativeArtifactRegistry::new();
        let id = request(1);
        registry
            .register(
                id,
                NativeArtifactKind::RoomKeyExportDestination,
                PathBuf::from("synthetic"),
            )
            .unwrap();
        assert_eq!(
            registry.take(id, NativeArtifactKind::RoomKeyImportSource),
            Err(NativeArtifactError::Missing)
        );
        assert_eq!(
            registry.take(request(2), NativeArtifactKind::RoomKeyExportDestination),
            Err(NativeArtifactError::Missing)
        );
        assert_eq!(
            registry
                .take(id, NativeArtifactKind::RoomKeyExportDestination)
                .unwrap(),
            PathBuf::from("synthetic")
        );
        assert_eq!(
            registry.take(id, NativeArtifactKind::RoomKeyExportDestination),
            Err(NativeArtifactError::Missing)
        );
    }

    #[test]
    fn duplicate_registration_preserves_the_original_path() {
        let registry = NativeArtifactRegistry::new();
        let id = request(1);
        registry
            .register(
                id,
                NativeArtifactKind::RoomKeyExportDestination,
                PathBuf::from("first"),
            )
            .unwrap();
        assert_eq!(
            registry.register(
                id,
                NativeArtifactKind::RoomKeyExportDestination,
                PathBuf::from("second"),
            ),
            Err(NativeArtifactError::AlreadyRegistered)
        );
        assert_eq!(
            registry
                .take(id, NativeArtifactKind::RoomKeyExportDestination)
                .unwrap(),
            PathBuf::from("first")
        );
    }

    #[test]
    fn unregister_and_drop_clear_paths_without_printing_them() {
        let registry = NativeArtifactRegistry::new();
        let id = request(1);
        registry
            .register(
                id,
                NativeArtifactKind::RecoveryKeyDestination,
                PathBuf::from("private-synthetic-path"),
            )
            .unwrap();
        registry.unregister(id, NativeArtifactKind::RecoveryKeyDestination);
        assert!(registry.is_empty());
        let debug = format!("{registry:?}");
        assert!(!debug.contains("private-synthetic-path"));
    }
}
