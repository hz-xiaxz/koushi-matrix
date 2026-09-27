use url::Url;

#[cfg(target_os = "linux")]
use std::env;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OidcBrowserLaunchFailure {
    InvalidAuthorizationUrl,
    BrowserLaunchFailed,
}

pub(crate) fn launch_oidc_authorization_url<E>(
    authorization_url: &str,
    launch: impl FnOnce(&str) -> Result<(), E>,
) -> Result<(), OidcBrowserLaunchFailure> {
    let parsed = Url::parse(authorization_url)
        .map_err(|_| OidcBrowserLaunchFailure::InvalidAuthorizationUrl)?;
    let has_userinfo = authorization_url
        .split_once("://")
        .and_then(|(_, remainder)| remainder.split(['/', '?', '#']).next())
        .is_some_and(|authority| authority.contains('@'));
    if !matches!(parsed.scheme(), "http" | "https")
        || authorization_url.contains('\\')
        || has_userinfo
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(OidcBrowserLaunchFailure::InvalidAuthorizationUrl);
    }
    launch(authorization_url).map_err(|_| OidcBrowserLaunchFailure::BrowserLaunchFailed)
}

/// Launch through one native desktop opener and retry with a second native
/// opener when the first one cannot be started. WSL needs the Linux opener
/// first: the generic opener delegates to `powershell.exe` there, whose
/// detached process can report success even when no Windows browser is
/// configured for the WSL session.
pub(crate) fn launch_oidc_authorization_url_with_fallback<E>(
    authorization_url: &str,
    primary: impl FnOnce(&str) -> Result<(), E>,
    fallback: impl FnOnce(&str) -> Result<(), E>,
) -> Result<(), OidcBrowserLaunchFailure> {
    launch_oidc_authorization_url(authorization_url, |url| {
        primary(url).or_else(|_| fallback(url))
    })
}

#[cfg(target_os = "linux")]
pub(crate) fn launch_linux_default_browser(
    authorization_url: &str,
) -> Result<(), tauri_plugin_opener::Error> {
    launch_linux_browser_with(authorization_url, "xdg-open")
}

#[cfg(target_os = "linux")]
fn launch_linux_browser_with(
    authorization_url: &str,
    program: &str,
) -> Result<(), tauri_plugin_opener::Error> {
    // The native opener owns detached process launch/reaping. An xdg-open
    // process may live as long as the browser, so never wait for its exit.
    // Success means dispatch, not that the browser painted a window.
    tauri_plugin_opener::open_url(authorization_url, Some(program))
}

#[cfg(target_os = "linux")]
pub(crate) fn running_under_wsl() -> bool {
    env::var_os("WSL_INTEROP").is_some() || env::var_os("WSL_DISTRO_NAME").is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn oidc_browser_returns_while_launcher_is_still_running() {
        use std::{
            io::Write,
            os::unix::fs::PermissionsExt,
            process::Command,
            sync::mpsc,
            time::{Duration, Instant},
        };

        let temp = tempfile::tempdir().unwrap();
        let fifo = temp.path().join("release");
        let ack = temp.path().join("release.ack");
        assert!(
            Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .unwrap()
                .success()
        );
        // O_RDWR keeps the pipe open without blocking on the launcher. The
        // launcher cannot exit until the test explicitly releases it.
        let mut release = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&fifo)
            .unwrap();
        let program = temp.path().join("browser");
        std::fs::write(
            &program,
            "#!/bin/sh\nread release < \"$1\"\nprintf x > \"$1.ack\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
        let (tx, rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let result =
                launch_linux_browser_with(fifo.to_str().unwrap(), program.to_str().unwrap());
            tx.send(result).unwrap();
        });
        let outcome = rx.recv_timeout(Duration::from_secs(2));
        writeln!(release, "done").unwrap();
        worker.join().unwrap();
        assert!(
            matches!(outcome, Ok(Ok(()))),
            "launch must settle before browser exit"
        );
        // Keep the executable alive until the detached process consumes its
        // release signal. A regular-file marker cannot block in read(2), and
        // the deadline also covers a child that exits without acknowledging.
        let deadline = Instant::now() + Duration::from_secs(2);
        while std::fs::read(&ack).ok().as_deref() != Some(b"x") {
            assert!(
                Instant::now() < deadline,
                "launcher did not acknowledge release"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn oidc_browser_launches_original_http_and_https_urls() {
        for authorization_url in [
            "http://127.0.0.1/authorize?opaque=one%2Btwo",
            "https://identity.example.invalid/authorize?opaque=one%2Btwo",
        ] {
            let mut launched = None;
            assert_eq!(
                launch_oidc_authorization_url(authorization_url, |url| {
                    launched = Some(url.to_owned());
                    Ok::<(), ()>(())
                }),
                Ok(())
            );
            assert_eq!(launched.as_deref(), Some(authorization_url));
        }
    }

    #[test]
    fn oidc_browser_rejects_malformed_non_http_and_userinfo_urls_without_launching() {
        for authorization_url in [
            "not a url",
            "ftp://identity.example.invalid/authorize",
            "https://@identity.example.invalid/authorize",
            "https://user@identity.example.invalid/authorize",
            "https://user:password@identity.example.invalid/authorize",
            "https:user@identity.example.invalid/authorize",
            r"https:/\/@identity.example.invalid/authorize",
        ] {
            let mut launched = false;
            assert_eq!(
                launch_oidc_authorization_url(authorization_url, |_| {
                    launched = true;
                    Ok::<(), ()>(())
                }),
                Err(OidcBrowserLaunchFailure::InvalidAuthorizationUrl)
            );
            assert!(!launched);
        }
    }

    #[test]
    fn oidc_browser_maps_native_failure_without_retaining_error_text() {
        let result = launch_oidc_authorization_url(
            "https://identity.example.invalid/authorize?opaque=private",
            |_| Err("native error with private details"),
        );
        assert_eq!(result, Err(OidcBrowserLaunchFailure::BrowserLaunchFailed));
        let debug = format!("{result:?}");
        assert!(!debug.contains("identity.example.invalid"));
        assert!(!debug.contains("opaque"));
        assert!(!debug.contains("native error"));
    }

    #[test]
    fn oidc_browser_uses_primary_launcher_before_fallback() {
        let calls = std::cell::Cell::new(0);
        let result = launch_oidc_authorization_url_with_fallback(
            "https://identity.example.invalid/authorize",
            |_| {
                calls.set(1);
                Ok::<(), ()>(())
            },
            |_| {
                calls.set(2);
                Ok::<(), ()>(())
            },
        );

        assert_eq!(result, Ok(()));
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn oidc_browser_uses_native_fallback_when_primary_launcher_fails() {
        let calls = std::cell::Cell::new(0);
        let result = launch_oidc_authorization_url_with_fallback(
            "https://identity.example.invalid/authorize",
            |_| {
                calls.set(1);
                Err::<(), _>(())
            },
            |_| {
                calls.set(2);
                Ok::<(), ()>(())
            },
        );

        assert_eq!(result, Ok(()));
        assert_eq!(calls.get(), 2);
    }
}
