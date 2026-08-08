#[test]
fn all_session_constructors_leave_the_per_send_backup_fence_disabled() {
    let source = include_str!("../src/lib.rs");
    assert_eq!(
        source
            .matches("require_secure_backup_for_encrypted_sends(false)")
            .count(),
        4
    );
    assert!(!source.contains("require_secure_backup_for_encrypted_sends(true)"));
}
