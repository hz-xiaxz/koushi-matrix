use koushi_state::{
    AppState, BasicOperationState, InviteOperationState, InviteScopeSelection,
    InviteTargetQueryState, InviteWorkflowState, SearchCrawlerLastActive,
    SearchCrawlerLastActiveStatus, SearchCrawlerRoomState, SearchCrawlerState, SessionInfo,
    SessionState, SyncState, VerificationAccountKind, VerificationGateState,
    VerificationMethodCapability,
};

pub(super) fn session_info() -> SessionInfo {
    SessionInfo {
        homeserver: "https://matrix.example.org".to_owned(),
        user_id: "@user-a:example.invalid".to_owned(),
        device_id: "DEVICE".to_owned(),
        authentication_method: koushi_state::SessionAuthenticationMethod::Unknown,
    }
}

pub(super) fn alternate_session_info() -> SessionInfo {
    SessionInfo {
        homeserver: "https://matrix.example.org".to_owned(),
        user_id: "@user-b:example.invalid".to_owned(),
        device_id: "DEVICE-B".to_owned(),
        authentication_method: koushi_state::SessionAuthenticationMethod::Unknown,
    }
}

pub(super) fn state_with_session_scoped_workflows() -> AppState {
    AppState {
        session: SessionState::Ready(session_info()),
        sync: SyncState::Running,
        basic_operation: BasicOperationState::CreatingRoom {
            request_id: 77,
            name: "Stale room".to_owned(),
        },
        invite_workflow: InviteWorkflowState {
            query: InviteTargetQueryState {
                room_id: Some("room-a".to_owned()),
                query: "alice".to_owned(),
                candidates: Vec::new(),
                explicit_user_id: None,
            },
            operation: InviteOperationState::Pending {
                request_id: 88,
                room_id: "room-a".to_owned(),
                user_ids: vec!["@alice:example.invalid".to_owned()],
                scope: InviteScopeSelection::RoomOnly,
            },
            ..Default::default()
        },
        search_crawler: SearchCrawlerState {
            rooms: std::collections::BTreeMap::from([(
                "room-a".to_owned(),
                SearchCrawlerRoomState::Running {
                    processed: 4,
                    indexed: 3,
                },
            )]),
            last_active: Some(SearchCrawlerLastActive {
                room_id: "room-a".to_owned(),
                updated_at_ms: 1_000,
                status: SearchCrawlerLastActiveStatus::Running,
                processed: 4,
                indexed: 3,
            }),
        },
        ..AppState::default()
    }
}

pub(super) fn assert_session_scoped_workflows_cleared(state: &AppState) {
    assert_eq!(state.basic_operation, BasicOperationState::Idle);
    assert_eq!(state.invite_workflow, InviteWorkflowState::default());
    assert_eq!(state.search_crawler, SearchCrawlerState::default());
}

pub(super) fn recovery_gate() -> VerificationGateState {
    VerificationGateState {
        methods: vec![VerificationMethodCapability::RecoveryKey],
        account_kind: VerificationAccountKind::ExistingIdentity,
        failure: None,
    }
}
