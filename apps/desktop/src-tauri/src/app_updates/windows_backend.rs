//! Windows release-check backend.
//!
//! No-key builds read public GitHub Releases for notifications. Key-bearing
//! builds use signed NSIS updates; installation belongs to the shutdown coordinator.

use serde::Deserialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::{Update, UpdaterExt};
use url::Url;

use super::channel_policy::{candidate_version_is_newer, select_newer_candidate, update_endpoints};
use super::{
    Backend, Completion, DESKTOP_UPDATE_EVENT_NAME, DesktopUpdateState, PendingUpdate,
    UpdateFuture, Work,
};

const RELEASES_API: &str = "https://api.github.com/repos/shinaoka/koushi-matrix/releases";

pub(super) enum Candidate {
    Notification,
    Signed(Box<Update>),
}

#[derive(Clone, Debug)]
pub(super) struct GitHubRelease {
    pub(super) version: String,
    pub(super) release_url: String,
}

#[derive(Debug, Deserialize)]
struct ReleaseResponse {
    tag_name: String,
    html_url: String,
    draft: bool,
    prerelease: bool,
}

pub(super) struct WindowsBackend {
    app: AppHandle,
    current_version: String,
}

impl WindowsBackend {
    pub(super) fn for_app(app: &AppHandle) -> Option<Self> {
        Some(Self {
            app: app.clone(),
            current_version: app.package_info().version.to_string(),
        })
    }
}

impl Backend<Candidate> for WindowsBackend {
    fn start(&self, work: Work<Candidate>) -> UpdateFuture<'static, Completion<Candidate>> {
        let current_version = self.current_version.clone();
        let app = self.app.clone();
        match work {
            Work::Check(include_prereleases) => Box::pin(async move {
                if super::configured_updater_public_key().is_some() {
                    return Completion::Check(
                        check_signed_releases(&app, include_prereleases).await,
                    );
                }
                let result = check_releases(&current_version, include_prereleases).await;
                Completion::Check(result.map(|candidate| {
                    candidate.map(|candidate| PendingUpdate {
                        version: candidate.version.clone(),
                        notification_only: true,
                        release_url: Some(candidate.release_url.clone()),
                        update: Candidate::Notification,
                        bytes: None,
                    })
                }))
            }),
            Work::Download(mut pending) => Box::pin(async move {
                let Candidate::Signed(update) = &pending.update else {
                    return Completion::Download(Err(()));
                };
                match update.download(|_, _| {}, || {}).await {
                    Ok(bytes) => {
                        pending.bytes = Some(bytes);
                        Completion::Download(Ok(pending))
                    }
                    Err(_) => Completion::Download(Err(())),
                }
            }),
            Work::Install(_) => Box::pin(async { Completion::Install(Err(())) }),
        }
    }

    fn emit(&self, state: DesktopUpdateState) {
        let _ = self.app.emit(DESKTOP_UPDATE_EVENT_NAME, state);
    }

    fn current_version(&self) -> &str {
        &self.current_version
    }

    fn restart(&self) {
        // NSIS owns relaunch after the shutdown coordinator launches it.
    }
}

async fn check_signed_releases(
    app: &AppHandle,
    include_prereleases: bool,
) -> Result<Option<PendingUpdate<Candidate>>, ()> {
    let mut selected: Option<Update> = None;
    for endpoint in update_endpoints(include_prereleases) {
        let updater = app
            .updater_builder()
            .endpoints(vec![Url::parse(endpoint).map_err(|_| ())?])
            .map_err(|_| ())?
            .build()
            .map_err(|_| ())?;
        if let Some(candidate) = updater.check().await.map_err(|_| ())? {
            selected = Some(select_newer_candidate(selected, candidate, |update| {
                update.version.as_str()
            }));
        }
    }
    Ok(selected.map(|update| PendingUpdate {
        version: update.version.clone(),
        notification_only: false,
        release_url: None,
        update: Candidate::Signed(Box::new(update)),
        bytes: None,
    }))
}

pub(super) fn install_verified(pending: PendingUpdate<Candidate>) -> Result<(), ()> {
    let Candidate::Signed(update) = pending.update else {
        return Err(());
    };
    update.install(&pending.bytes.ok_or(())?).map_err(|_| ())
}

async fn check_releases(
    current_version: &str,
    include_prereleases: bool,
) -> Result<Option<GitHubRelease>, ()> {
    let client = reqwest::Client::builder()
        .user_agent("Koushi desktop update checker")
        .build()
        .map_err(|_| ())?;
    let releases = client
        .get(RELEASES_API)
        .query(&[("per_page", "30")])
        .send()
        .await
        .map_err(|_| ())?
        .error_for_status()
        .map_err(|_| ())?
        .json::<Vec<ReleaseResponse>>()
        .await
        .map_err(|_| ())?;

    let mut selected = None;
    for release in releases {
        if release.draft || (!include_prereleases && release.prerelease) {
            continue;
        }
        let Some(version) = release.tag_name.strip_prefix('v') else {
            continue;
        };
        if !is_valid_semver(version) || !candidate_version_is_newer(current_version, version) {
            continue;
        }
        let candidate = GitHubRelease {
            version: version.to_owned(),
            release_url: release.html_url,
        };
        selected = Some(select_newer_candidate(selected, candidate, |candidate| {
            candidate.version.as_str()
        }));
    }
    Ok(selected)
}

fn is_valid_semver(version: &str) -> bool {
    let version = version.split_once('+').map_or(version, |(value, _)| value);
    let (core, prerelease) = version
        .split_once('-')
        .map_or((version, None), |(core, pre)| (core, Some(pre)));
    let core = core.split('.').collect::<Vec<_>>();
    if core.len() != 3
        || core.iter().any(|part| {
            part.is_empty()
                || (part.len() > 1 && part.starts_with('0'))
                || part.parse::<u64>().is_err()
        })
    {
        return false;
    }
    prerelease.is_none_or(|value| {
        !value.is_empty()
            && value.split('.').all(|part| {
                !part.is_empty()
                    && (!part.chars().all(|character| character.is_ascii_digit())
                        || (part.len() == 1 || !part.starts_with('0')))
                    && part
                        .chars()
                        .all(|character| character.is_ascii_alphanumeric() || character == '-')
            })
    })
}
