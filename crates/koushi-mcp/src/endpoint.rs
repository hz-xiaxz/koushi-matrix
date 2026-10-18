//! Endpoint discovery and the `endpoint.json` contract.
//!
//! The app writes the file on startup (`0600`, directory `0700`) and removes it
//! on shutdown. The launcher only reads it. The token inside is per launch; it
//! never appears in argv, environment, or logs, which is why the file exists at
//! all instead of a command-line argument.

use std::path::{Path, PathBuf};

use crate::wire::EndpointFile;

/// Application data directory name used by the Tauri bundle identifier.
pub const APP_DIR: &str = "chat.koushi.desktop";

/// Default location of `endpoint.json` on this platform.
pub fn default_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME").map(|home| {
            PathBuf::from(home)
                .join("Library/Application Support")
                .join(APP_DIR)
                .join("mcp/endpoint.json")
        })
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("LOCALAPPDATA").map(|dir| {
            PathBuf::from(dir)
                .join(APP_DIR)
                .join("mcp")
                .join("endpoint.json")
        })
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
            return Some(PathBuf::from(runtime).join("koushi").join("endpoint.json"));
        }
        std::env::var_os("HOME").map(|home| {
            PathBuf::from(home)
                .join(".local/share")
                .join(APP_DIR)
                .join("mcp/endpoint.json")
        })
    }
}

/// Reads and parses the endpoint file.
///
/// On Unix the file must not be group- or world-accessible; a loosened mode is
/// treated as an error rather than silently trusted.
pub fn read(path: &Path) -> std::io::Result<EndpointFile> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = std::fs::metadata(path)?;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "endpoint file is accessible by other users",
            ));
        }
    }
    let bytes = std::fs::read(path)?;
    serde_json::from_slice(&bytes)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_endpoint(path: &Path, content: &str) {
        std::fs::write(path, content).expect("write");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).expect("chmod");
        }
    }

    #[test]
    fn reads_a_private_endpoint_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("endpoint.json");
        write_endpoint(
            &path,
            r#"{"schema":1,"endpoint":"/tmp/x.sock","token":"abc","pid":7,"app_version":"0.26.0"}"#,
        );
        let endpoint = read(&path).expect("read");
        assert_eq!(endpoint.schema, 1);
        assert_eq!(endpoint.token, "abc");
        assert_eq!(endpoint.pid, 7);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_a_world_readable_endpoint_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("endpoint.json");
        std::fs::write(
            &path,
            r#"{"schema":1,"endpoint":"/tmp/x.sock","token":"abc","pid":7,"app_version":"0.26.0"}"#,
        )
        .expect("write");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).expect("chmod");
        let error = read(&path).expect_err("must reject");
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    }
}
