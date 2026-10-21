//! Explicit private current-session disclosure; never a snapshot or event.
use super::*;

pub struct AccessTokenResponse(koushi_core::session_access_token::SessionAccessToken);
impl Serialize for AccessTokenResponse {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("AccessTokenResponse", 4)?;
        state.serialize_field("access_token", self.0.token())?;
        state.serialize_field("homeserver", &self.0.session().homeserver)?;
        state.serialize_field("user_id", &self.0.session().user_id)?;
        state.serialize_field("device_id", &self.0.session().device_id)?;
        state.end()
    }
}

#[tauri::command]
pub async fn current_session_access_token(
    account_tab_id: Option<String>,
    expected_session: SessionInfo,
    state: State<'_, CoreRuntimeState>,
) -> Result<AccessTokenResponse, String> {
    let connection = account_connection(state.inner(), account_tab_id.as_deref()).await?;
    let result = tokio::time::timeout(
        Duration::from_secs(10),
        connection.current_session_access_token(expected_session),
    )
    .await
    .map_err(|_| "current session access token unavailable".to_owned())?
    .map_err(|_| "current session access token unavailable".to_owned())?;
    let current = account_connection(state.inner(), account_tab_id.as_deref()).await?;
    if !connection.shares_runtime_with(&current) {
        return Err("current session access token unavailable".into());
    }
    tokio::time::timeout(
        Duration::from_secs(10),
        current.validate_session_access_token(&result),
    )
    .await
    .map_err(|_| "current session access token unavailable".to_owned())?
    .map_err(|_| "current session access token unavailable".to_owned())?;
    Ok(AccessTokenResponse(result))
}
