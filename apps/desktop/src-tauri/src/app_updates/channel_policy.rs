//! Platform-neutral update channel policy consumed by install backends.
//!
//! Channel selection and greatest-SemVer candidate choice are product policy,
//! not macOS integration: every backend (macOS today, a future Linux backend)
//! reuses them. The module is compiled wherever a backend exists and in tests
//! on every platform, so the policy stays exercised on Linux CI.

use std::cmp::Ordering;

pub(crate) const STABLE_UPDATE_ENDPOINT: &str =
    "https://github.com/shinaoka/koushi-matrix/releases/latest/download/latest.json";
pub(crate) const BETA_UPDATE_ENDPOINT: &str =
    "https://github.com/shinaoka/koushi-matrix/releases/download/latest-beta/latest-beta.json";

/// Feeds a check consults for the persisted pre-release policy.
pub(crate) fn update_endpoints(include_prereleases: bool) -> &'static [&'static str] {
    if include_prereleases {
        &[STABLE_UPDATE_ENDPOINT, BETA_UPDATE_ENDPOINT]
    } else {
        &[STABLE_UPDATE_ENDPOINT]
    }
}

/// Keeps the greater SemVer candidate when several feeds report one.
pub(crate) fn select_newer_candidate<C>(
    current: Option<C>,
    candidate: C,
    version: impl Fn(&C) -> &str,
) -> C {
    match current {
        None => candidate,
        Some(current) => {
            if candidate_version_is_newer(version(&current), version(&candidate)) {
                candidate
            } else {
                current
            }
        }
    }
}

pub(crate) fn candidate_version_is_newer(current: &str, candidate: &str) -> bool {
    compare_semver(candidate, current) == Ordering::Greater
}

fn compare_semver(left: &str, right: &str) -> Ordering {
    let left = parse_semver(left);
    let right = parse_semver(right);
    for (left_part, right_part) in left.core.iter().zip(right.core.iter()) {
        match left_part.cmp(right_part) {
            Ordering::Equal => {}
            ordering => return ordering,
        }
    }
    match (left.prerelease.as_slice(), right.prerelease.as_slice()) {
        ([], []) => Ordering::Equal,
        ([], _) => Ordering::Greater,
        (_, []) => Ordering::Less,
        (left, right) => {
            for (left_part, right_part) in left.iter().zip(right.iter()) {
                match compare_prerelease_identifier(left_part, right_part) {
                    Ordering::Equal => {}
                    ordering => return ordering,
                }
            }
            left.len().cmp(&right.len())
        }
    }
}

struct ParsedSemVer<'a> {
    core: [u64; 3],
    prerelease: Vec<&'a str>,
}

fn parse_semver(version: &str) -> ParsedSemVer<'_> {
    let version = version
        .split_once('+')
        .map_or(version, |(version, _)| version);
    let (core, prerelease) = match version.split_once('-') {
        Some((core, prerelease)) => (core, prerelease.split('.').collect::<Vec<_>>()),
        None => (version, Vec::new()),
    };
    let mut core_parts = core.split('.');
    let core = [
        core_parts
            .next()
            .and_then(|part| part.parse().ok())
            .expect("tauri updater returns valid SemVer"),
        core_parts
            .next()
            .and_then(|part| part.parse().ok())
            .expect("tauri updater returns valid SemVer"),
        core_parts
            .next()
            .and_then(|part| part.parse().ok())
            .expect("tauri updater returns valid SemVer"),
    ];
    ParsedSemVer { core, prerelease }
}

fn compare_prerelease_identifier(left: &str, right: &str) -> Ordering {
    let left_numeric = left.parse::<u64>();
    let right_numeric = right.parse::<u64>();
    match (left_numeric, right_numeric) {
        (Ok(left), Ok(right)) => left.cmp(&right),
        (Ok(_), Err(_)) => Ordering::Less,
        (Err(_), Ok(_)) => Ordering::Greater,
        (Err(_), Err(_)) => left.cmp(right),
    }
}
