use crate::{MatrixClientSession, MatrixRoomOperationError};
use matrix_sdk::ruma::events::AnyGlobalAccountDataEventContent;
use matrix_sdk::ruma::{events::GlobalAccountDataEventType, serde::Raw};
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

#[derive(Clone, Debug, Eq, PartialEq)]
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
