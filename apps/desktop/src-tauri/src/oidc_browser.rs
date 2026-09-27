use url::Url;

#[cfg(target_os = "linux")]
use std::{
    env,
    process::{Command, Stdio},
};

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
pub(crate) fn launch_linux_default_browser(authorization_url: &str) -> std::io::Result<()> {
    Command::new("xdg-open")
        .arg(authorization_url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .and_then(|status| {
            status
                .success()
                .then_some(())
                .ok_or_else(|| std::io::Error::other("xdg-open exited unsuccessfully"))
        })
}

#[cfg(target_os = "linux")]
pub(crate) fn running_under_wsl() -> bool {
    env::var_os("WSL_INTEROP").is_some() || env::var_os("WSL_DISTRO_NAME").is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

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
