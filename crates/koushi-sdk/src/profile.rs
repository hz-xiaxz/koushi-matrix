use crate::room_projection::matrix_room;
use crate::{LOCAL_USER_ALIASES_ACCOUNT_DATA_TYPE, MatrixClientSession, MatrixRoomOperationError};
use matrix_sdk::ruma::{
    events::{AnyGlobalAccountDataEventContent, GlobalAccountDataEventType},
    serde::Raw,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};
use thiserror::Error;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatrixOwnProfile {
    pub display_name: Option<String>,
    pub avatar_mxc_uri: Option<String>,
}

#[derive(Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct MatrixLocalUserAliases {
    #[serde(default)]
    pub aliases: BTreeMap<String, String>,
}

impl fmt::Debug for MatrixLocalUserAliases {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MatrixLocalUserAliases")
            .field("alias_count", &self.aliases.len())
            .finish()
    }
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum MatrixProfileError {
    #[error("Matrix profile mime type is invalid")]
    InvalidMimeType,
    #[error("Matrix profile operation failed")]
    Sdk(MatrixProfileFailureKind),
}

impl MatrixProfileError {
    pub fn failure_kind(&self) -> MatrixProfileFailureKind {
        match self {
            Self::InvalidMimeType => MatrixProfileFailureKind::InvalidMimeType,
            Self::Sdk(kind) => *kind,
        }
    }

    fn from_sdk_error(error: matrix_sdk::Error) -> Self {
        Self::Sdk(matrix_profile_failure_kind(&error))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatrixProfileFailureKind {
    Forbidden,
    Network,
    InvalidMimeType,
    Sdk,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum MatrixIgnoredUserListError {
    #[error("Matrix user id is invalid")]
    InvalidUserId,
    #[error("Matrix ignored user list operation failed")]
    Sdk(MatrixIgnoredUserListFailureKind),
}

impl MatrixIgnoredUserListError {
    pub fn failure_kind(&self) -> MatrixIgnoredUserListFailureKind {
        match self {
            Self::InvalidUserId => MatrixIgnoredUserListFailureKind::InvalidUserId,
            Self::Sdk(kind) => *kind,
        }
    }

    fn from_sdk_error(error: matrix_sdk::Error) -> Self {
        Self::Sdk(matrix_ignored_user_list_failure_kind(&error))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatrixIgnoredUserListFailureKind {
    Forbidden,
    Network,
    InvalidUserId,
    Sdk,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum MatrixReportError {
    #[error("Matrix user id is invalid")]
    InvalidUserId,
    #[error("Matrix room id is invalid")]
    InvalidRoomId,
    #[error("Matrix event id is invalid")]
    InvalidEventId,
    #[error("Matrix report operation failed")]
    Sdk(MatrixReportFailureKind),
}

impl MatrixReportError {
    pub fn failure_kind(&self) -> MatrixReportFailureKind {
        match self {
            Self::InvalidUserId => MatrixReportFailureKind::InvalidUserId,
            Self::InvalidRoomId => MatrixReportFailureKind::InvalidRoomId,
            Self::InvalidEventId => MatrixReportFailureKind::InvalidEventId,
            Self::Sdk(kind) => *kind,
        }
    }

    fn from_sdk_error(error: matrix_sdk::Error) -> Self {
        Self::Sdk(matrix_report_failure_kind(&error))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatrixReportFailureKind {
    Forbidden,
    Network,
    InvalidUserId,
    InvalidRoomId,
    InvalidEventId,
    Sdk,
}

pub async fn get_own_profile(
    session: &MatrixClientSession,
) -> Result<MatrixOwnProfile, MatrixProfileError> {
    matrix_own_profile_from_session(session).await
}

pub async fn set_display_name(
    session: &MatrixClientSession,
    display_name: Option<&str>,
) -> Result<MatrixOwnProfile, MatrixProfileError> {
    session
        .client()
        .account()
        .set_display_name(display_name)
        .await
        .map_err(MatrixProfileError::from_sdk_error)?;
    matrix_own_profile_from_session(session).await
}

pub async fn set_avatar(
    session: &MatrixClientSession,
    mime_type: &str,
    bytes: Vec<u8>,
) -> Result<MatrixOwnProfile, MatrixProfileError> {
    let mime = mime_type
        .parse::<mime::Mime>()
        .map_err(|_| MatrixProfileError::InvalidMimeType)?;
    session
        .client()
        .account()
        .upload_avatar(&mime, bytes)
        .await
        .map_err(MatrixProfileError::from_sdk_error)?;
    matrix_own_profile_from_session(session).await
}

pub async fn get_local_user_aliases(
    session: &MatrixClientSession,
) -> Result<MatrixLocalUserAliases, MatrixProfileError> {
    let raw = fetch_local_user_aliases_raw(session).await?;
    let Some(raw) = raw else {
        return Ok(MatrixLocalUserAliases::default());
    };
    let content = raw
        .deserialize_as_unchecked::<MatrixLocalUserAliases>()
        .map_err(|_| matrix_profile_serialization_error())?;

    Ok(MatrixLocalUserAliases {
        aliases: normalized_local_user_aliases(content.aliases),
    })
}

pub async fn set_local_user_aliases(
    session: &MatrixClientSession,
    aliases: BTreeMap<String, String>,
) -> Result<MatrixLocalUserAliases, MatrixProfileError> {
    let content = MatrixLocalUserAliases {
        aliases: normalized_local_user_aliases(aliases),
    };
    let raw: Raw<AnyGlobalAccountDataEventContent> = Raw::new(&content)
        .map_err(|_| matrix_profile_serialization_error())?
        .cast_unchecked();
    session
        .client()
        .account()
        .set_account_data_raw(local_user_aliases_event_type(), raw)
        .await
        .map_err(MatrixProfileError::from_sdk_error)?;

    Ok(content)
}

pub async fn update_local_user_alias(
    session: &MatrixClientSession,
    user_id: &str,
    alias: Option<&str>,
) -> Result<MatrixLocalUserAliases, MatrixProfileError> {
    let mut aliases = get_local_user_aliases(session).await?.aliases;
    if let Some(alias) = normalize_local_user_alias(alias) {
        aliases.insert(user_id.to_owned(), alias);
    } else {
        aliases.remove(user_id);
    }
    set_local_user_aliases(session, aliases).await
}

pub async fn get_ignored_user_list(
    session: &MatrixClientSession,
) -> Result<BTreeSet<String>, MatrixIgnoredUserListError> {
    let account = session.client().account();
    let raw = account
        .account_data::<matrix_sdk::ruma::events::ignored_user_list::IgnoredUserListEventContent>()
        .await
        .map_err(MatrixIgnoredUserListError::from_sdk_error)?;
    let Some(raw) = raw else {
        return Ok(BTreeSet::new());
    };
    let content = raw
        .deserialize()
        .map_err(|_| MatrixIgnoredUserListError::Sdk(MatrixIgnoredUserListFailureKind::Sdk))?;

    Ok(content
        .ignored_users
        .into_keys()
        .map(|user_id| user_id.to_string())
        .collect())
}

pub async fn ignore_user(
    session: &MatrixClientSession,
    user_id: &str,
) -> Result<BTreeSet<String>, MatrixIgnoredUserListError> {
    let user_id = matrix_sdk::ruma::UserId::parse(user_id)
        .map_err(|_| MatrixIgnoredUserListError::InvalidUserId)?;
    session
        .client()
        .account()
        .ignore_user(&user_id)
        .await
        .map_err(MatrixIgnoredUserListError::from_sdk_error)?;
    get_ignored_user_list(session).await
}

pub async fn unignore_user(
    session: &MatrixClientSession,
    user_id: &str,
) -> Result<BTreeSet<String>, MatrixIgnoredUserListError> {
    let user_id = matrix_sdk::ruma::UserId::parse(user_id)
        .map_err(|_| MatrixIgnoredUserListError::InvalidUserId)?;
    session
        .client()
        .account()
        .unignore_user(&user_id)
        .await
        .map_err(MatrixIgnoredUserListError::from_sdk_error)?;
    get_ignored_user_list(session).await
}

pub async fn report_content(
    session: &MatrixClientSession,
    room_id: &str,
    event_id: &str,
    reason: Option<String>,
) -> Result<(), MatrixReportError> {
    let room = matrix_room(session, room_id).map_err(|error| match error {
        MatrixRoomOperationError::InvalidRoomId => MatrixReportError::InvalidRoomId,
        _ => MatrixReportError::Sdk(MatrixReportFailureKind::Sdk),
    })?;
    let event_id = matrix_sdk::ruma::EventId::parse(event_id)
        .map_err(|_| MatrixReportError::InvalidEventId)?;
    room.report_content(event_id, reason)
        .await
        .map_err(MatrixReportError::from_sdk_error)?;
    Ok(())
}

pub async fn report_room(
    session: &MatrixClientSession,
    room_id: &str,
    reason: String,
) -> Result<(), MatrixReportError> {
    let room = matrix_room(session, room_id).map_err(|error| match error {
        MatrixRoomOperationError::InvalidRoomId => MatrixReportError::InvalidRoomId,
        _ => MatrixReportError::Sdk(MatrixReportFailureKind::Sdk),
    })?;
    room.report_room(reason)
        .await
        .map_err(MatrixReportError::from_sdk_error)?;
    Ok(())
}

pub async fn report_user(
    session: &MatrixClientSession,
    user_id: &str,
    reason: String,
) -> Result<(), MatrixReportError> {
    let user_id =
        matrix_sdk::ruma::UserId::parse(user_id).map_err(|_| MatrixReportError::InvalidUserId)?;
    let request =
        matrix_sdk::ruma::api::client::reporting::report_user::v3::Request::new(user_id, reason);
    session.client().send(request).await.map_err(|error| {
        MatrixReportError::from_sdk_error(matrix_sdk::Error::Http(Box::new(error)))
    })?;
    Ok(())
}

async fn matrix_own_profile_from_session(
    session: &MatrixClientSession,
) -> Result<MatrixOwnProfile, MatrixProfileError> {
    let account = session.client().account();
    let display_name = account
        .get_display_name()
        .await
        .map_err(MatrixProfileError::from_sdk_error)?;
    let avatar_mxc_uri = account
        .get_avatar_url()
        .await
        .map_err(MatrixProfileError::from_sdk_error)?
        .map(|uri| uri.to_string());
    Ok(MatrixOwnProfile {
        display_name,
        avatar_mxc_uri,
    })
}

fn local_user_aliases_event_type() -> GlobalAccountDataEventType {
    GlobalAccountDataEventType::from(LOCAL_USER_ALIASES_ACCOUNT_DATA_TYPE.to_owned())
}

async fn fetch_local_user_aliases_raw(
    session: &MatrixClientSession,
) -> Result<Option<Raw<AnyGlobalAccountDataEventContent>>, MatrixProfileError> {
    let account = session.client().account();
    account
        .fetch_account_data(local_user_aliases_event_type())
        .await
        .map_err(MatrixProfileError::from_sdk_error)
}

pub(super) fn normalized_local_user_aliases(
    aliases: BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    aliases
        .into_iter()
        .filter_map(|(user_id, alias)| {
            if user_id.trim().is_empty() {
                return None;
            }
            normalize_local_user_alias(Some(&alias)).map(|alias| (user_id, alias))
        })
        .collect()
}

fn normalize_local_user_alias(alias: Option<&str>) -> Option<String> {
    alias
        .map(str::trim)
        .filter(|alias| !alias.is_empty())
        .map(ToOwned::to_owned)
}

fn matrix_profile_serialization_error() -> MatrixProfileError {
    MatrixProfileError::Sdk(MatrixProfileFailureKind::Sdk)
}

fn matrix_profile_failure_kind(error: &matrix_sdk::Error) -> MatrixProfileFailureKind {
    match error {
        matrix_sdk::Error::Http(error) => {
            if error
                .as_client_api_error()
                .is_some_and(|error| error.status_code.as_u16() == 403)
                || matches!(
                    error.client_api_error_kind(),
                    Some(matrix_sdk::ruma::api::error::ErrorKind::Forbidden)
                )
            {
                MatrixProfileFailureKind::Forbidden
            } else {
                MatrixProfileFailureKind::Sdk
            }
        }
        matrix_sdk::Error::Timeout => MatrixProfileFailureKind::Network,
        _ => MatrixProfileFailureKind::Sdk,
    }
}

fn matrix_ignored_user_list_failure_kind(
    error: &matrix_sdk::Error,
) -> MatrixIgnoredUserListFailureKind {
    match error {
        matrix_sdk::Error::Http(error) => {
            if error
                .as_client_api_error()
                .is_some_and(|error| error.status_code.as_u16() == 403)
                || matches!(
                    error.client_api_error_kind(),
                    Some(matrix_sdk::ruma::api::error::ErrorKind::Forbidden)
                )
            {
                MatrixIgnoredUserListFailureKind::Forbidden
            } else {
                MatrixIgnoredUserListFailureKind::Sdk
            }
        }
        matrix_sdk::Error::Timeout => MatrixIgnoredUserListFailureKind::Network,
        _ => MatrixIgnoredUserListFailureKind::Sdk,
    }
}

fn matrix_report_failure_kind(error: &matrix_sdk::Error) -> MatrixReportFailureKind {
    match error {
        matrix_sdk::Error::Http(error) => {
            if error
                .as_client_api_error()
                .is_some_and(|error| error.status_code.as_u16() == 403)
                || matches!(
                    error.client_api_error_kind(),
                    Some(matrix_sdk::ruma::api::error::ErrorKind::Forbidden)
                )
            {
                MatrixReportFailureKind::Forbidden
            } else {
                MatrixReportFailureKind::Sdk
            }
        }
        matrix_sdk::Error::Timeout => MatrixReportFailureKind::Network,
        _ => MatrixReportFailureKind::Sdk,
    }
}

#[cfg(test)]
mod tests {
    use super::{MatrixLocalUserAliases, normalized_local_user_aliases};
    use crate::LOCAL_USER_ALIASES_ACCOUNT_DATA_TYPE;

    use std::collections::BTreeMap;
    #[test]
    fn local_user_aliases_account_data_serde_uses_private_flat_map() {
        let mut aliases = BTreeMap::new();
        aliases.insert(
            "@alice:example.invalid".to_owned(),
            "Local Alice".to_owned(),
        );
        let content = MatrixLocalUserAliases {
            aliases: aliases.clone(),
        };

        let value = serde_json::to_value(&content).expect("serialize local aliases");
        assert_eq!(
            LOCAL_USER_ALIASES_ACCOUNT_DATA_TYPE,
            "app.koushi.local_aliases"
        );
        assert_eq!(
            value["aliases"]["@alice:example.invalid"],
            serde_json::json!("Local Alice")
        );

        let parsed: MatrixLocalUserAliases =
            serde_json::from_value(value).expect("deserialize local aliases");
        assert_eq!(parsed.aliases, aliases);
    }
    #[test]
    fn local_user_aliases_debug_is_artifact_safe() {
        let content = MatrixLocalUserAliases {
            aliases: BTreeMap::from([(
                "@alice:example.invalid".to_owned(),
                "Local Alice".to_owned(),
            )]),
        };

        let debug = format!("{content:?}");

        assert!(debug.contains("MatrixLocalUserAliases"));
        assert!(debug.contains("alias_count"));
        assert!(!debug.contains("@alice:example.invalid"));
        assert!(!debug.contains("Local Alice"));
    }
    #[test]
    fn local_user_aliases_debug_redacts_user_ids_and_aliases() {
        let content = MatrixLocalUserAliases {
            aliases: BTreeMap::from([(
                "@alice:example.invalid".to_owned(),
                "Local Alice".to_owned(),
            )]),
        };

        let debug = format!("{content:?}");

        assert!(debug.contains("MatrixLocalUserAliases"));
        assert!(debug.contains("alias_count"));
        assert!(!debug.contains("@alice:example.invalid"));
        assert!(!debug.contains("Local Alice"));
    }
    #[test]
    fn normalized_local_user_aliases_trims_and_drops_empty_entries() {
        let aliases = BTreeMap::from([
            (
                "@alice:example.invalid".to_owned(),
                "  Local Alice  ".to_owned(),
            ),
            ("@bob:example.invalid".to_owned(), "   ".to_owned()),
        ]);

        let normalized = normalized_local_user_aliases(aliases);

        assert_eq!(
            normalized,
            BTreeMap::from([(
                "@alice:example.invalid".to_owned(),
                "Local Alice".to_owned()
            )])
        );
    }
}
