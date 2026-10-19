//! Where the bundled `koushi-mcp` launcher is, as a path an assistant can keep
//! in its configuration.
//!
//! Release packages place the launcher next to the app executable. That path
//! is stable for an installed app, but not for an AppImage (mounted at a new
//! temporary path each run) or a macOS app running from a disk image or a
//! translocated copy; those cases get a stable copy or an explanation instead.

use std::path::{Path, PathBuf};

use koushi_mcp_host::consent::{LauncherStatus, LauncherUnavailable};

const LAUNCHER: &str = if cfg!(windows) {
    "koushi-mcp.exe"
} else {
    "koushi-mcp"
};

pub(super) fn resolve(data_dir: &Path) -> LauncherStatus {
    let Ok(executable) = tauri::utils::platform::current_exe() else {
        return unavailable(LauncherUnavailable::NotBundled);
    };
    let Some(directory) = executable.parent() else {
        return unavailable(LauncherUnavailable::NotBundled);
    };
    let bundled = directory.join(LAUNCHER);
    if !bundled.is_file() {
        return unavailable(LauncherUnavailable::NotBundled);
    }
    if cfg!(target_os = "linux") && std::env::var_os("APPIMAGE").is_some() {
        return match stable_copy(&bundled, data_dir) {
            Ok(path) => available(path),
            Err(_) => unavailable(LauncherUnavailable::Ephemeral),
        };
    }
    if cfg!(target_os = "macos") && is_transient_macos_location(&bundled) {
        return unavailable(LauncherUnavailable::Translocated);
    }
    available(bundled)
}

fn available(path: PathBuf) -> LauncherStatus {
    LauncherStatus::Available {
        path: path.to_string_lossy().into_owned(),
    }
}

fn unavailable(reason: LauncherUnavailable) -> LauncherStatus {
    LauncherStatus::Unavailable { reason }
}

/// A disk image mount or an App Translocation copy disappears when the image
/// is ejected or the app is moved.
fn is_transient_macos_location(path: &Path) -> bool {
    let text = path.to_string_lossy();
    text.starts_with("/Volumes/") || text.contains("/AppTranslocation/")
}

/// Copies the launcher to `<data_dir>/mcp/bin/` when the copy is missing or
/// differs. The copy goes through a temporary file and a rename, so a launcher
/// an assistant is running right now is never overwritten in place.
fn stable_copy(source: &Path, data_dir: &Path) -> std::io::Result<PathBuf> {
    let directory = data_dir.join("mcp").join("bin");
    std::fs::create_dir_all(&directory)?;
    let target = directory.join(LAUNCHER);
    let bytes = std::fs::read(source)?;
    if std::fs::read(&target).is_ok_and(|existing| existing == bytes) {
        return Ok(target);
    }
    let temporary = tempfile::NamedTempFile::new_in(&directory)?;
    std::fs::write(temporary.path(), &bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o755))?;
    }
    temporary.persist(&target).map_err(|error| error.error)?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_images_and_translocation_are_transient() {
        assert!(is_transient_macos_location(Path::new(
            "/Volumes/Koushi/Koushi.app/Contents/MacOS/koushi-mcp"
        )));
        assert!(is_transient_macos_location(Path::new(
            "/private/var/folders/x/AppTranslocation/ABC/d/Koushi.app/Contents/MacOS/koushi-mcp"
        )));
        assert!(!is_transient_macos_location(Path::new(
            "/Applications/Koushi.app/Contents/MacOS/koushi-mcp"
        )));
    }

    #[test]
    fn the_stable_copy_is_refreshed_only_when_it_differs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source = dir.path().join("mount").join(LAUNCHER);
        std::fs::create_dir_all(source.parent().expect("parent")).expect("mkdir");
        std::fs::write(&source, b"launcher v1").expect("write");
        let data_dir = dir.path().join("data");

        let target = stable_copy(&source, &data_dir).expect("copy");
        assert_eq!(std::fs::read(&target).expect("read"), b"launcher v1");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&target)
                .expect("meta")
                .permissions()
                .mode();
            assert_eq!(mode & 0o111, 0o111, "executable");
        }

        std::fs::write(&source, b"launcher v2").expect("write");
        let again = stable_copy(&source, &data_dir).expect("copy");
        assert_eq!(again, target);
        assert_eq!(std::fs::read(&target).expect("read"), b"launcher v2");
    }
}
