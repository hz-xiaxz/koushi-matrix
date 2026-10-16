use super::*;

#[test]
fn only_the_default_notification_response_navigates() {
    use notify_rust::{CloseReason, NotificationResponse};

    assert!(notification_response_activates(
        &NotificationResponse::Default
    ));
    assert!(!notification_response_activates(
        &NotificationResponse::Action("other".to_owned())
    ));
    assert!(!notification_response_activates(
        &NotificationResponse::Closed(CloseReason::Dismissed)
    ));
}

#[cfg(target_os = "linux")]
#[test]
fn linux_banner_registers_a_default_click_action() {
    let notification = response_notification("Title", "Body", None);
    assert_eq!(notification.actions, vec!["default", "Open"]);
}

#[cfg(windows)]
#[test]
fn windows_installed_banner_uses_its_own_app_identity() {
    assert_eq!(
        notification_app_id(false, "chat.koushi.desktop"),
        Some("chat.koushi.desktop")
    );
    assert_eq!(notification_app_id(true, "chat.koushi.desktop"), None);
}
