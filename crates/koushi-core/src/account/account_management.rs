//! `account_management` ownership for AccountActor.

use std::collections::BTreeMap;

use koushi_state::{AccountManagementOperation, AppAction, AuthFailureKind, DeviceSessionSummary};

use crate::failure::CoreFailure;
use crate::ids::RequestId;

use super::actor::AccountActor;
use super::recovery_backup::classify_e2ee_trust_auth_failure;

pub(super) struct PendingUiaOperation {
    operation: AccountManagementOperation,
    raw_device_ids: Vec<String>,
    new_password: Option<koushi_state::AuthSecret>,
    erase_data: bool,
    uiaa_session: Option<String>,
}

enum AccountManagementUiaError {
    DeleteDevices(koushi_sdk::DeleteDevicesError),
    AccountManagement(koushi_sdk::AccountManagementError),
}

impl AccountActor {
    pub(super) async fn handle_query_devices(&mut self, request_id: RequestId) {
        let session = match &self.session {
            Some(session) => session.clone(),
            None => {
                self.send_actions(vec![AppAction::DeviceSessionsLoadFailed {
                    request_id: request_id.sequence,
                    kind: AuthFailureKind::Sdk,
                }])
                .await;
                self.emit_failure(request_id, CoreFailure::SessionRequired);
                return;
            }
        };

        match koushi_sdk::list_devices(&session).await {
            Ok(devices) => {
                let mut ordinal_map = BTreeMap::new();
                let summaries = devices
                    .into_iter()
                    .enumerate()
                    .map(|(index, device)| {
                        let ordinal = index as u64 + 1;
                        ordinal_map.insert(ordinal, device.raw_device_id);
                        DeviceSessionSummary {
                            device_ordinal: ordinal,
                            display_name: device.display_name,
                            current: device.current,
                            verified: device.verified,
                            inactive: device.inactive,
                        }
                    })
                    .collect();
                self.device_session_ordinals = ordinal_map;
                self.send_actions(vec![AppAction::DeviceSessionsLoaded {
                    request_id: request_id.sequence,
                    devices: summaries,
                }])
                .await;
            }
            Err(error) => {
                let kind = classify_e2ee_trust_auth_failure(&error);
                self.send_actions(vec![AppAction::DeviceSessionsLoadFailed {
                    request_id: request_id.sequence,
                    kind,
                }])
                .await;
                self.emit_failure(request_id, CoreFailure::AccountOperationFailed { kind });
            }
        }
    }

    pub(super) async fn handle_load_account_management_capabilities(
        &mut self,
        request_id: RequestId,
    ) {
        let session = match &self.session {
            Some(session) => session.clone(),
            None => {
                self.send_actions(vec![AppAction::AccountManagementCapabilitiesLoadFailed])
                    .await;
                self.emit_failure(request_id, CoreFailure::SessionRequired);
                return;
            }
        };

        let capabilities = koushi_sdk::account_management_capabilities(&session).await;
        self.send_actions(vec![AppAction::AccountManagementCapabilitiesLoaded {
            change_password: capabilities.change_password,
        }])
        .await;
    }

    pub(super) async fn handle_rename_device(
        &mut self,
        request_id: RequestId,
        device_ordinal: u64,
        display_name: String,
    ) {
        let operation = AccountManagementOperation::RenameDevice;
        let session = match &self.session {
            Some(session) => session.clone(),
            None => {
                self.project_account_management_failure(
                    request_id,
                    operation,
                    AuthFailureKind::Sdk,
                    CoreFailure::SessionRequired,
                )
                .await;
                return;
            }
        };
        let Some(raw_device_id) = self.device_session_ordinals.get(&device_ordinal).cloned() else {
            self.project_account_management_failure(
                request_id,
                operation,
                AuthFailureKind::Sdk,
                CoreFailure::AccountOperationFailed {
                    kind: AuthFailureKind::Sdk,
                },
            )
            .await;
            return;
        };

        let result = koushi_sdk::rename_device(&session, &raw_device_id, &display_name).await;
        drop(display_name);
        match result {
            Ok(()) => {
                self.send_actions(vec![AppAction::AccountManagementSucceeded {
                    request_id: request_id.sequence,
                    operation,
                }])
                .await;
            }
            Err(_) => {
                self.project_account_management_failure(
                    request_id,
                    operation,
                    AuthFailureKind::Sdk,
                    CoreFailure::AccountOperationFailed {
                        kind: AuthFailureKind::Sdk,
                    },
                )
                .await
            }
        }
    }

    pub(super) async fn handle_delete_devices(
        &mut self,
        request_id: RequestId,
        device_ordinals: Vec<u64>,
        auth: Option<koushi_state::IdentityResetAuthRequest>,
    ) {
        let operation = if device_ordinals.len() == 1 {
            AccountManagementOperation::DeleteDevice
        } else {
            AccountManagementOperation::DeleteOtherDevices
        };
        let session = match &self.session {
            Some(session) => session.clone(),
            None => {
                self.project_account_management_failure(
                    request_id,
                    operation,
                    AuthFailureKind::Sdk,
                    CoreFailure::SessionRequired,
                )
                .await;
                return;
            }
        };
        let mut raw_device_ids = Vec::with_capacity(device_ordinals.len());
        for ordinal in &device_ordinals {
            let Some(raw_device_id) = self.device_session_ordinals.get(ordinal) else {
                self.project_account_management_failure(
                    request_id,
                    operation,
                    AuthFailureKind::Sdk,
                    CoreFailure::AccountOperationFailed {
                        kind: AuthFailureKind::Sdk,
                    },
                )
                .await;
                return;
            };
            raw_device_ids.push(raw_device_id.clone());
        }

        // If this is the first attempt (no auth), try without auth so the
        // server can challenge us with UIA. The challenge response is handled
        // below by projecting AwaitingUia and storing the continuation.
        let uiaa_session = auth
            .as_ref()
            .and_then(|_| self.pending_uia_operations.get(&request_id.sequence))
            .and_then(|pending| pending.uiaa_session.clone());
        let result = koushi_sdk::delete_devices(
            &session,
            &raw_device_ids,
            auth.as_ref(),
            uiaa_session.as_deref(),
        )
        .await;
        drop(auth);
        match result {
            Ok(()) => {
                self.pending_uia_operations.remove(&request_id.sequence);
                self.send_actions(vec![AppAction::AccountManagementSucceeded {
                    request_id: request_id.sequence,
                    operation,
                }])
                .await;
            }
            Err(koushi_sdk::DeleteDevicesError::UiaaChallenge { session }) => {
                let flow_id = request_id.sequence;
                self.pending_uia_operations.insert(
                    flow_id,
                    PendingUiaOperation {
                        operation,
                        raw_device_ids,
                        new_password: None,
                        erase_data: false,
                        uiaa_session: session,
                    },
                );
                self.send_actions(vec![AppAction::AccountManagementUiaRequired {
                    request_id: request_id.sequence,
                    flow_id,
                    operation,
                }])
                .await;
            }
            Err(koushi_sdk::DeleteDevicesError::Sdk(_)) => {
                self.pending_uia_operations.remove(&request_id.sequence);
                self.project_account_management_failure(
                    request_id,
                    operation,
                    AuthFailureKind::Sdk,
                    CoreFailure::AccountOperationFailed {
                        kind: AuthFailureKind::Sdk,
                    },
                )
                .await;
            }
        }
    }

    pub(super) async fn handle_change_password(
        &mut self,
        request_id: RequestId,
        new_password: koushi_state::AuthSecret,
    ) {
        let operation = AccountManagementOperation::ChangePassword;
        let session = match &self.session {
            Some(session) => session.clone(),
            None => {
                self.project_account_management_failure(
                    request_id,
                    operation,
                    AuthFailureKind::Sdk,
                    CoreFailure::SessionRequired,
                )
                .await;
                return;
            }
        };

        let result = koushi_sdk::change_password(&session, &new_password, None, None).await;
        match result {
            Ok(()) => {
                self.send_actions(vec![AppAction::AccountManagementSucceeded {
                    request_id: request_id.sequence,
                    operation,
                }])
                .await;
            }
            Err(koushi_sdk::AccountManagementError::UiaaChallenge { session }) => {
                let flow_id = request_id.sequence;
                self.pending_uia_operations.insert(
                    flow_id,
                    PendingUiaOperation {
                        operation,
                        raw_device_ids: Vec::new(),
                        new_password: Some(new_password),
                        erase_data: false,
                        uiaa_session: session,
                    },
                );
                self.send_actions(vec![AppAction::AccountManagementUiaRequired {
                    request_id: request_id.sequence,
                    flow_id,
                    operation,
                }])
                .await;
            }
            Err(koushi_sdk::AccountManagementError::Sdk(_)) => {
                drop(new_password);
                self.project_account_management_failure(
                    request_id,
                    operation,
                    AuthFailureKind::Sdk,
                    CoreFailure::AccountOperationFailed {
                        kind: AuthFailureKind::Sdk,
                    },
                )
                .await;
            }
        }
    }

    pub(super) async fn handle_deactivate_account(
        &mut self,
        request_id: RequestId,
        erase_data: bool,
    ) {
        let operation = AccountManagementOperation::DeactivateAccount;
        let session = match &self.session {
            Some(session) => session.clone(),
            None => {
                self.project_account_management_failure(
                    request_id,
                    operation,
                    AuthFailureKind::Sdk,
                    CoreFailure::SessionRequired,
                )
                .await;
                return;
            }
        };

        let result = koushi_sdk::deactivate_account(&session, erase_data, None, None).await;
        match result {
            Ok(()) => {
                self.pending_uia_operations.remove(&request_id.sequence);
                self.send_actions(vec![AppAction::AccountManagementSucceeded {
                    request_id: request_id.sequence,
                    operation,
                }])
                .await;
                // Deactivation ends the account on the server. Perform local
                // sign-out cleanup without sending a second /logout request.
                self.perform_logout(request_id, false, false).await;
            }
            Err(koushi_sdk::AccountManagementError::UiaaChallenge { session }) => {
                let flow_id = request_id.sequence;
                self.pending_uia_operations.insert(
                    flow_id,
                    PendingUiaOperation {
                        operation,
                        raw_device_ids: Vec::new(),
                        new_password: None,
                        erase_data,
                        uiaa_session: session,
                    },
                );
                self.send_actions(vec![AppAction::AccountManagementUiaRequired {
                    request_id: request_id.sequence,
                    flow_id,
                    operation,
                }])
                .await;
            }
            Err(koushi_sdk::AccountManagementError::Sdk(_)) => {
                self.project_account_management_failure(
                    request_id,
                    operation,
                    AuthFailureKind::Sdk,
                    CoreFailure::AccountOperationFailed {
                        kind: AuthFailureKind::Sdk,
                    },
                )
                .await;
            }
        }
    }

    pub(super) async fn handle_submit_account_management_uia(
        &mut self,
        request_id: RequestId,
        flow_id: u64,
        auth: koushi_state::IdentityResetAuthRequest,
    ) {
        let Some(mut pending) = self.pending_uia_operations.remove(&flow_id) else {
            self.emit_failure(
                request_id,
                CoreFailure::AccountOperationFailed {
                    kind: AuthFailureKind::Sdk,
                },
            );
            return;
        };
        let operation = pending.operation;
        let session = match &self.session {
            Some(session) => session.clone(),
            None => {
                self.project_account_management_failure(
                    RequestId {
                        connection_id: request_id.connection_id,
                        sequence: flow_id,
                    },
                    operation,
                    AuthFailureKind::Sdk,
                    CoreFailure::SessionRequired,
                )
                .await;
                return;
            }
        };

        let result = match operation {
            AccountManagementOperation::RenameDevice
            | AccountManagementOperation::ThreePid
            | AccountManagementOperation::IdentityServer => {
                // These operations do not use UIA; no pending op should exist.
                self.emit_failure(
                    RequestId {
                        connection_id: request_id.connection_id,
                        sequence: flow_id,
                    },
                    CoreFailure::AccountOperationFailed {
                        kind: AuthFailureKind::Sdk,
                    },
                );
                return;
            }
            AccountManagementOperation::DeleteDevice
            | AccountManagementOperation::DeleteOtherDevices => koushi_sdk::delete_devices(
                &session,
                &pending.raw_device_ids,
                Some(&auth),
                pending.uiaa_session.as_deref(),
            )
            .await
            .map_err(AccountManagementUiaError::DeleteDevices),
            AccountManagementOperation::ChangePassword => {
                let Some(new_password) = pending.new_password.as_ref() else {
                    self.project_account_management_failure(
                        RequestId {
                            connection_id: request_id.connection_id,
                            sequence: flow_id,
                        },
                        operation,
                        AuthFailureKind::Sdk,
                        CoreFailure::AccountOperationFailed {
                            kind: AuthFailureKind::Sdk,
                        },
                    )
                    .await;
                    return;
                };
                koushi_sdk::change_password(
                    &session,
                    new_password,
                    Some(&auth),
                    pending.uiaa_session.as_deref(),
                )
                .await
                .map_err(AccountManagementUiaError::AccountManagement)
            }
            AccountManagementOperation::DeactivateAccount => koushi_sdk::deactivate_account(
                &session,
                pending.erase_data,
                Some(&auth),
                pending.uiaa_session.as_deref(),
            )
            .await
            .map_err(AccountManagementUiaError::AccountManagement),
        };
        drop(auth);
        match result {
            Ok(()) => {
                let was_deactivation = operation == AccountManagementOperation::DeactivateAccount;
                self.send_actions(vec![AppAction::AccountManagementSucceeded {
                    request_id: flow_id,
                    operation,
                }])
                .await;
                if was_deactivation {
                    self.perform_logout(
                        RequestId {
                            connection_id: request_id.connection_id,
                            sequence: flow_id,
                        },
                        false,
                        false,
                    )
                    .await;
                }
            }
            Err(AccountManagementUiaError::DeleteDevices(
                koushi_sdk::DeleteDevicesError::UiaaChallenge { session },
            ))
            | Err(AccountManagementUiaError::AccountManagement(
                koushi_sdk::AccountManagementError::UiaaChallenge { session },
            )) => {
                pending.uiaa_session = session;
                self.pending_uia_operations.insert(flow_id, pending);
                self.emit_failure(
                    request_id,
                    CoreFailure::AccountOperationFailed {
                        kind: AuthFailureKind::Forbidden,
                    },
                );
            }
            Err(AccountManagementUiaError::DeleteDevices(koushi_sdk::DeleteDevicesError::Sdk(
                _,
            )))
            | Err(AccountManagementUiaError::AccountManagement(
                koushi_sdk::AccountManagementError::Sdk(_),
            )) => {
                self.project_account_management_failure(
                    RequestId {
                        connection_id: request_id.connection_id,
                        sequence: flow_id,
                    },
                    operation,
                    AuthFailureKind::Sdk,
                    CoreFailure::AccountOperationFailed {
                        kind: AuthFailureKind::Sdk,
                    },
                )
                .await;
            }
        }
    }

    async fn project_account_management_failure(
        &self,
        request_id: RequestId,
        operation: AccountManagementOperation,
        kind: AuthFailureKind,
        failure: CoreFailure,
    ) {
        self.send_actions(vec![AppAction::AccountManagementFailed {
            request_id: request_id.sequence,
            operation,
            kind,
        }])
        .await;
        self.emit_failure(request_id, failure);
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn device_list_failures_are_not_reported_as_store_unavailable() {
        let handler = crate::account::test_source::item_body(
            include_str!("account_management.rs"),
            "async fn handle_query_devices",
        );

        assert!(
            handler.contains("classify_e2ee_trust_auth_failure(&error)"),
            "device list failures must classify SDK/network channel errors"
        );
        assert!(
            !handler.contains("CoreFailure::StoreUnavailable"),
            "device list failures must not masquerade as credential-store failures"
        );
    }
}
