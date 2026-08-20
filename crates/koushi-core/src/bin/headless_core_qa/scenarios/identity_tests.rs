use crate::participants::authenticated_session_info_from_state;
use crate::{SessionInfo, SessionState};

#[test]
fn e2ee_trust_qa_uses_authenticated_provisional_session_info() {
    let info = SessionInfo {
        homeserver: "https://example.invalid".to_owned(),
        user_id: "@alice:example.invalid".to_owned(),
        device_id: "ALICEDEVICE".to_owned(),
        authentication_method: koushi_state::SessionAuthenticationMethod::Unknown,
    };

    assert_eq!(
        authenticated_session_info_from_state(&SessionState::Provisional {
            info: info.clone(),
            phase: koushi_state::ProvisionalPhase::CheckingTrust,
        }),
        Some(&info)
    );
    assert_eq!(
        authenticated_session_info_from_state(&SessionState::AwaitingVerification {
            info: info.clone(),
            gate: koushi_state::VerificationGateState {
                methods: vec![],
                account_kind: koushi_state::VerificationAccountKind::Unknown,
                failure: None,
            },
        }),
        Some(&info)
    );
    assert_eq!(
        authenticated_session_info_from_state(&SessionState::Ready(info.clone())),
        Some(&info)
    );
    assert_eq!(
        authenticated_session_info_from_state(&SessionState::SignedOut),
        None
    );
}
