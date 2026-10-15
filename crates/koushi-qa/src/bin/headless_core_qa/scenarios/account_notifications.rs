//! Account notification settings QA (#981) against a disposable local
//! homeserver.
//!
//! A second SDK session for user A plays "another client (Element)": it
//! customises the standard push rules, then Koushi's Core loads the settings
//! twice. The server ruleset must be byte-for-byte unchanged afterwards (no
//! write on open), the mixed/custom state must be projected as-is, a category
//! toggle must write only its own rules, and email notifications must never
//! turn ON for an address that is not a verified 3PID.

use koushi_protocol::command::AccountNotificationsRequest;
use koushi_state::{
    AccountNotificationsFailureKind, AccountNotificationsLoadState, AccountNotificationsOperation,
    AccountNotificationsOperationState, AccountNotificationsState, NotificationCategory,
    NotificationCategoryState, NotificationEmailManagement,
};
use matrix_sdk::ruma::{
    api::client::push::{
        get_pushers, get_pushrules_all, set_pushrule_actions, set_pushrule_enabled,
    },
    push::{Action, PatternedPushRule, RuleKind, Ruleset},
};

use super::event_wait::QaEventDeadline;
use super::registry::{EVENT_TIMEOUT, QaConfig};
use super::{AccountCommand, AuthSecret, CoreCommand, CoreConnection, CoreEvent, RequestId};

const QA_CUSTOM_SOUND: &str = "koushi-qa-custom-sound";

/// Synthetic keyword pattern for this lane's disposable account (#1427).
/// `vantor` is not a word in any language: the pattern only has to be a glob
/// the server stores and matches whole-word, so it is never a real keyword.
const QA_KEYWORD_PATTERN: &str = "qa-keyword-vantor";

/// The same pattern in another case. Matrix matches `content` patterns
/// case-insensitively, so the account already holds this keyword.
const QA_KEYWORD_PATTERN_OTHER_CASE: &str = "QA-KEYWORD-VANTOR";

async fn fetch_rules(raw: &koushi_sdk::MatrixClientSession) -> Result<Ruleset, String> {
    raw.client()
        .send(get_pushrules_all::v3::Request::new())
        .await
        .map(|response| response.global)
        .map_err(|_| "account_notifications: raw pushrules read failed".to_owned())
}

async fn email_pusher_count(raw: &koushi_sdk::MatrixClientSession) -> Result<usize, String> {
    raw.client()
        .send(get_pushers::v3::Request::new())
        .await
        .map(|response| {
            response
                .pushers
                .iter()
                .filter(|pusher| pusher.ids.app_id == koushi_sdk::EMAIL_PUSHER_APP_ID)
                .count()
        })
        .map_err(|_| "account_notifications: raw pushers read failed".to_owned())
}

async fn raw_set_actions(
    raw: &koushi_sdk::MatrixClientSession,
    kind: RuleKind,
    rule_id: &str,
    actions: serde_json::Value,
) -> Result<(), String> {
    let actions: Vec<Action> = serde_json::from_value(actions)
        .map_err(|_| "account_notifications: fixture actions invalid".to_owned())?;
    raw.client()
        .send(set_pushrule_actions::v3::Request::new(
            kind,
            rule_id.to_owned(),
            actions,
        ))
        .await
        .map(|_| ())
        .map_err(|_| format!("account_notifications: fixture write {rule_id} failed"))
}

async fn raw_set_enabled(
    raw: &koushi_sdk::MatrixClientSession,
    kind: RuleKind,
    rule_id: &str,
    enabled: bool,
) -> Result<(), String> {
    raw.client()
        .send(set_pushrule_enabled::v3::Request::new(
            kind,
            rule_id.to_owned(),
            enabled,
        ))
        .await
        .map(|_| ())
        .map_err(|_| format!("account_notifications: fixture enable {rule_id} failed"))
}

async fn send_request(
    conn: &mut CoreConnection,
    request: AccountNotificationsRequest,
    label: &str,
) -> Result<RequestId, String> {
    let request_id = conn.next_request_id();
    tokio::time::timeout(
        EVENT_TIMEOUT,
        conn.command(CoreCommand::Account(AccountCommand::AccountNotifications {
            request_id,
            request,
        })),
    )
    .await
    .map_err(|_| format!("account_notifications: {label} submit timed out"))?
    .map_err(|_| format!("account_notifications: {label} command rejected"))?;
    Ok(request_id)
}

async fn wait_for(
    conn: &mut CoreConnection,
    label: &str,
    predicate: impl Fn(&AccountNotificationsState) -> bool,
) -> Result<AccountNotificationsState, String> {
    let deadline = QaEventDeadline::after(EVENT_TIMEOUT);
    loop {
        let state = conn.snapshot().account_notifications.clone();
        if predicate(&state) {
            return Ok(state);
        }
        let event = deadline
            .recv(conn)
            .await
            .map_err(|_| format!("account_notifications: timed out waiting for {label}"))?
            .map_err(|lag| {
                format!(
                    "account_notifications: event stream lagged during {label} (skipped={})",
                    lag.skipped
                )
            })?;
        if !matches!(
            event,
            CoreEvent::StateDelta(_) | CoreEvent::OperationFailed { .. }
        ) {
            continue;
        }
    }
}

async fn load(conn: &mut CoreConnection, label: &str) -> Result<AccountNotificationsState, String> {
    let request_id = send_request(conn, AccountNotificationsRequest::Load, label).await?;
    let sequence = request_id.sequence;
    let state = wait_for(conn, label, |state| {
        matches!(state.load, AccountNotificationsLoadState::Loaded)
            || matches!(state.load, AccountNotificationsLoadState::Failed { request_id, .. } if request_id == sequence)
    })
    .await?;
    if !matches!(state.load, AccountNotificationsLoadState::Loaded) {
        return Err(format!("account_notifications: {label} failed"));
    }
    Ok(state)
}

async fn settle_operation(
    conn: &mut CoreConnection,
    request: AccountNotificationsRequest,
    label: &str,
) -> Result<AccountNotificationsState, String> {
    let request_id = send_request(conn, request, label).await?;
    let sequence = request_id.sequence;
    wait_for(conn, label, |state| match &state.operation {
        AccountNotificationsOperationState::Succeeded { request_id, .. }
        | AccountNotificationsOperationState::Failed { request_id, .. } => *request_id == sequence,
        _ => false,
    })
    .await
}

fn rule_json(ruleset: &Ruleset, kind: RuleKind, rule_id: &str) -> Option<serde_json::Value> {
    let rule = ruleset.get(kind, rule_id)?;
    Some(serde_json::json!({
        "enabled": rule.enabled(),
        "actions": serde_json::to_value(rule.actions()).ok()?,
    }))
}

/// The 16 lowercase hex characters after the app's keyword prefix, if the rule
/// id has exactly that shape. The spec reserves a dot prefix for server-default
/// rules, so an account keyword id is the app prefix plus a fixed-width hash.
fn keyword_rule_id_suffix(rule_id: &str) -> Option<&str> {
    let suffix = rule_id.strip_prefix(koushi_sdk::KEYWORD_RULE_ID_PREFIX)?;
    (suffix.len() == 16
        && suffix
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
    .then_some(suffix)
}

/// The account's own `content` rules that name a Koushi keyword for `pattern`,
/// compared Matrix's own case-insensitive way.
fn keyword_rules_with_pattern<'a>(
    ruleset: &'a Ruleset,
    pattern: &str,
) -> Vec<&'a PatternedPushRule> {
    ruleset
        .content
        .iter()
        .filter(|rule| {
            !rule.default
                && keyword_rule_id_suffix(&rule.rule_id).is_some()
                && rule.pattern.eq_ignore_ascii_case(pattern)
        })
        .collect()
}

/// #1427 keyword delivery, driven through the app's own commands and read back
/// raw from the disposable homeserver.
///
/// A keyword is a `kind: content` push rule, and the only reason it reaches the
/// mention trigger is the rule's action array: the write must store exactly
/// `["notify", {"set_tweak": "highlight"}]`. The vendored SDK's own keyword
/// helper writes a default sound and no highlight, so this client does not use
/// it, and only the server can confirm what the app actually wrote.
async fn run_keyword_phase(
    conn: &mut CoreConnection,
    raw: &koushi_sdk::MatrixClientSession,
) -> Result<(), String> {
    let state = settle_operation(
        conn,
        AccountNotificationsRequest::AddKeyword {
            pattern: QA_KEYWORD_PATTERN.to_owned(),
        },
        "add keyword",
    )
    .await?;
    if !matches!(
        state.operation,
        AccountNotificationsOperationState::Succeeded { .. }
    ) {
        return Err("account_notifications: keyword add did not succeed".to_owned());
    }

    // 1. Read the minted rule back raw from the server: one rule, the app's id
    // shape, the minted pattern, and the highlight tweak in the content
    // position. The actions comparison is element by element, so a default
    // sound, a custom tweak, or a second highlight fails where a substring
    // check would pass.
    let rule_id = {
        let rules = fetch_rules(raw).await?;
        let matching = keyword_rules_with_pattern(&rules, QA_KEYWORD_PATTERN);
        if matching.len() != 1 {
            return Err(format!(
                "account_notifications: keyword add left {} matching content rules",
                matching.len()
            ));
        }
        let rule = matching[0];
        if keyword_rule_id_suffix(&rule.rule_id).is_none() {
            return Err("account_notifications: keyword rule id is not the app shape".to_owned());
        }
        if rule.pattern != QA_KEYWORD_PATTERN {
            return Err(
                "account_notifications: keyword rule pattern is not the minted word".to_owned(),
            );
        }
        let expected_actions = serde_json::json!(["notify", {"set_tweak": "highlight"}]);
        let stored_actions = serde_json::to_value(&rule.actions).map_err(|_| {
            "account_notifications: keyword actions serialization failed".to_owned()
        })?;
        if stored_actions != expected_actions {
            return Err(format!(
                "account_notifications: keyword actions were {stored_actions}"
            ));
        }
        rule.rule_id.clone()
    };
    println!("account_notifications_keyword_actions=ok");

    // 2. The app projects the rule with the minted pattern and ON.
    let projected = state
        .snapshot
        .as_ref()
        .and_then(|snapshot| {
            snapshot
                .keywords
                .iter()
                .find(|keyword| keyword.rule_id == rule_id)
        })
        .ok_or("account_notifications: keyword add is missing from the projection")?;
    if projected.pattern != QA_KEYWORD_PATTERN || !projected.enabled {
        return Err("account_notifications: keyword projection is not the minted rule".to_owned());
    }
    println!("account_notifications_keyword_projection=ok");

    // 3. The same word in another case is the same keyword: the add is refused
    // and the refusal writes no second rule and no second id.
    let state = settle_operation(
        conn,
        AccountNotificationsRequest::AddKeyword {
            pattern: QA_KEYWORD_PATTERN_OTHER_CASE.to_owned(),
        },
        "duplicate keyword",
    )
    .await?;
    if !matches!(
        &state.operation,
        AccountNotificationsOperationState::Failed {
            failure_kind: AccountNotificationsFailureKind::Duplicate,
            operation: AccountNotificationsOperation::AddNotificationKeyword { .. },
            ..
        }
    ) {
        return Err("account_notifications: second-case keyword add was not refused".to_owned());
    }
    let rules = fetch_rules(raw).await?;
    if keyword_rules_with_pattern(&rules, QA_KEYWORD_PATTERN).len() != 1 {
        return Err(
            "account_notifications: duplicate refusal wrote a second content rule".to_owned(),
        );
    }
    println!("account_notifications_keyword_duplicate=ok");

    // 4. Another client turns the rule off out of band. Off is not gone: the
    // projection shows it OFF and the account still refuses a same-pattern add.
    raw_set_enabled(raw, RuleKind::Content, &rule_id, false).await?;
    // The raw server agrees the rule is off before the app re-reads it.
    let rules = fetch_rules(raw).await?;
    let raw_enabled = rule_json(&rules, RuleKind::Content, &rule_id)
        .and_then(|rule| rule.get("enabled").and_then(serde_json::Value::as_bool));
    if raw_enabled != Some(false) {
        return Err(
            "account_notifications: out-of-band keyword disable did not reach the server"
                .to_owned(),
        );
    }
    // A completed load carries no request id and the previous operation already
    // left the load state at `Loaded`, so wait for the projection to carry the
    // server's new fact rather than for `Loaded` to be reached again.
    let _ = send_request(conn, AccountNotificationsRequest::Load, "keyword reload").await?;
    let state = wait_for(conn, "keyword reload", |state| {
        state.snapshot.as_ref().is_some_and(|snapshot| {
            snapshot
                .keywords
                .iter()
                .any(|keyword| keyword.rule_id == rule_id && !keyword.enabled)
        })
    })
    .await?;
    let projected = state
        .snapshot
        .as_ref()
        .and_then(|snapshot| {
            snapshot
                .keywords
                .iter()
                .find(|keyword| keyword.rule_id == rule_id)
        })
        .ok_or("account_notifications: disabled keyword left the projection")?;
    if projected.enabled {
        return Err("account_notifications: disabled keyword is still projected ON".to_owned());
    }
    if projected.pattern != QA_KEYWORD_PATTERN {
        return Err("account_notifications: disabled keyword lost its pattern".to_owned());
    }
    let state = settle_operation(
        conn,
        AccountNotificationsRequest::AddKeyword {
            pattern: QA_KEYWORD_PATTERN.to_owned(),
        },
        "disabled keyword duplicate",
    )
    .await?;
    if !matches!(
        &state.operation,
        AccountNotificationsOperationState::Failed {
            failure_kind: AccountNotificationsFailureKind::Duplicate,
            ..
        }
    ) {
        return Err(
            "account_notifications: disabled keyword did not refuse a same-pattern add".to_owned(),
        );
    }
    println!("account_notifications_keyword_disabled=ok");

    // 5. Removal deletes that rule from the account, not from a local list.
    let state = settle_operation(
        conn,
        AccountNotificationsRequest::RemoveKeyword {
            rule_id: rule_id.clone(),
        },
        "remove keyword",
    )
    .await?;
    if !matches!(
        state.operation,
        AccountNotificationsOperationState::Succeeded { .. }
    ) {
        return Err("account_notifications: keyword removal did not succeed".to_owned());
    }
    let rules = fetch_rules(raw).await?;
    if !keyword_rules_with_pattern(&rules, QA_KEYWORD_PATTERN).is_empty() {
        return Err("account_notifications: removed keyword is still on the server".to_owned());
    }
    if state.snapshot.as_ref().is_some_and(|snapshot| {
        snapshot
            .keywords
            .iter()
            .any(|keyword| keyword.rule_id == rule_id)
    }) {
        return Err("account_notifications: removed keyword is still projected".to_owned());
    }
    println!("account_notifications_keyword_remove=ok");
    Ok(())
}

pub(super) async fn run_account_notifications_stage(
    config: &QaConfig,
    conn: &mut CoreConnection,
) -> Result<(), String> {
    // "Another client" for the same account.
    let raw = koushi_sdk::login_with_password(&koushi_state::LoginRequest {
        homeserver: config.homeserver.clone(),
        username: config.user_a.clone(),
        password: AuthSecret::new(config.password_a.clone()),
        device_display_name: Some("Koushi Notification Fixture".to_owned()),
    })
    .await
    .map_err(|_| "account_notifications: fixture login failed".to_owned())?;
    let result = run_with_fixture(conn, &raw).await;
    let _ = koushi_sdk::close_session_stores(&raw).await;
    drop(raw);
    result
}

async fn run_with_fixture(
    conn: &mut CoreConnection,
    raw: &koushi_sdk::MatrixClientSession,
) -> Result<(), String> {
    // Element-style customisation: mixed group rules, a custom DM sound, and
    // only @room mentions disabled.
    raw_set_actions(
        raw,
        RuleKind::Underride,
        ".m.rule.encrypted",
        serde_json::json!([]),
    )
    .await?;
    raw_set_actions(
        raw,
        RuleKind::Underride,
        ".m.rule.room_one_to_one",
        serde_json::json!(["notify", {"set_tweak": "sound", "value": QA_CUSTOM_SOUND}]),
    )
    .await?;
    raw_set_enabled(raw, RuleKind::Override, ".m.rule.is_room_mention", false).await?;
    let before = serde_json::to_value(fetch_rules(raw).await?)
        .map_err(|_| "account_notifications: ruleset serialization failed".to_owned())?;

    let state = load(conn, "first load").await?;
    let snapshot = state
        .snapshot
        .as_ref()
        .ok_or("account_notifications: load produced no snapshot")?;
    if snapshot.categories.group_messages != NotificationCategoryState::Mixed
        || snapshot.categories.mentions_and_replies != NotificationCategoryState::Mixed
        || snapshot.categories.direct_messages != NotificationCategoryState::On
        || snapshot.categories.invites != NotificationCategoryState::On
    {
        return Err(format!(
            "account_notifications: unexpected category projection {:?}",
            snapshot.categories
        ));
    }
    if snapshot.email_notifications_active() {
        return Err("account_notifications: fresh account reported email ON".to_owned());
    }
    println!("account_notifications_load=ok");

    // Re-open: still read-only.
    load(conn, "second load").await?;
    let after = serde_json::to_value(fetch_rules(raw).await?)
        .map_err(|_| "account_notifications: ruleset serialization failed".to_owned())?;
    if before != after {
        return Err(
            "account_notifications: opening the settings changed server push rules".to_owned(),
        );
    }
    println!("account_notifications_no_write_on_open=ok");

    // Toggle group messages OFF: only group rules change.
    let state = settle_operation(
        conn,
        AccountNotificationsRequest::SetCategory {
            category: NotificationCategory::GroupMessages,
            enabled: false,
        },
        "group off",
    )
    .await?;
    if !matches!(
        state.operation,
        AccountNotificationsOperationState::Succeeded { .. }
    ) {
        return Err("account_notifications: group off did not succeed".to_owned());
    }
    let projected = state
        .snapshot
        .as_ref()
        .map(|snapshot| snapshot.categories.group_messages);
    if projected != Some(NotificationCategoryState::Off) {
        return Err(format!(
            "account_notifications: group off re-read projected {projected:?}"
        ));
    }
    let rules = fetch_rules(raw).await?;
    let silent = serde_json::json!({"enabled": true, "actions": []});
    for rule_id in [".m.rule.message", ".m.rule.encrypted"] {
        if rule_json(&rules, RuleKind::Underride, rule_id).as_ref() != Some(&silent) {
            return Err(format!("account_notifications: {rule_id} not silenced"));
        }
    }
    let before_rules: Ruleset = serde_json::from_value(before.clone())
        .map_err(|_| "account_notifications: ruleset parse failed".to_owned())?;
    for (kind, rule_id) in [
        (RuleKind::Underride, ".m.rule.room_one_to_one"),
        (RuleKind::Underride, ".m.rule.encrypted_room_one_to_one"),
        (RuleKind::Override, ".m.rule.is_room_mention"),
        (RuleKind::Override, ".m.rule.is_user_mention"),
        (RuleKind::Override, ".m.rule.invite_for_me"),
    ] {
        if rule_json(&rules, kind.clone(), rule_id) != rule_json(&before_rules, kind, rule_id) {
            return Err(format!(
                "account_notifications: group toggle touched unrelated rule {rule_id}"
            ));
        }
    }
    println!("account_notifications_category_write=ok");

    // Overlap on the real server ruleset: group OFF, mentions still notify.
    if message_notifies(&rules, &raw.info.user_id, false).await {
        return Err(
            "account_notifications: group off still notifies a plain group message".to_owned(),
        );
    }
    if !message_notifies(&rules, &raw.info.user_id, true).await {
        return Err("account_notifications: group off suppressed a user mention".to_owned());
    }
    println!("account_notifications_overlap=ok");

    // Turn group back ON; the custom DM sound is still untouched.
    let state = settle_operation(
        conn,
        AccountNotificationsRequest::SetCategory {
            category: NotificationCategory::GroupMessages,
            enabled: true,
        },
        "group on",
    )
    .await?;
    let projected = state
        .snapshot
        .as_ref()
        .map(|snapshot| snapshot.categories.group_messages);
    if projected != Some(NotificationCategoryState::On) {
        return Err(format!(
            "account_notifications: group on re-read projected {projected:?}"
        ));
    }
    let rules = serde_json::to_string(&fetch_rules(raw).await?)
        .map_err(|_| "account_notifications: ruleset serialization failed".to_owned())?;
    if !rules.contains(QA_CUSTOM_SOUND) {
        return Err("account_notifications: custom DM sound was overwritten".to_owned());
    }
    println!("account_notifications_category_restore=ok");

    // Email: never ON without a verified address; unsupported servers are
    // reported instead of pretending success.
    let management = state
        .snapshot
        .as_ref()
        .map(|snapshot| snapshot.email_management);
    match management {
        Some(NotificationEmailManagement::Unsupported) => {}
        Some(NotificationEmailManagement::Available) => {
            let state = settle_operation(
                conn,
                AccountNotificationsRequest::RequestEmailToken {
                    address: "qa-notifications@example.invalid".to_owned(),
                    lang: "en".to_owned(),
                },
                "request email token",
            )
            .await?;
            match &state.operation {
                AccountNotificationsOperationState::Failed {
                    failure_kind:
                        AccountNotificationsFailureKind::Unsupported
                        | AccountNotificationsFailureKind::EmailDenied,
                    ..
                } => {
                    if state.pending_email.is_some() {
                        return Err(
                            "account_notifications: failed token request left a pending email"
                                .to_owned(),
                        );
                    }
                }
                // A fixture with a working mail sender would reach here; the
                // local QA homeservers are not configured to send email.
                AccountNotificationsOperationState::Succeeded { .. } => {
                    let _ = send_request(
                        conn,
                        AccountNotificationsRequest::CancelPendingEmail,
                        "cancel pending",
                    )
                    .await;
                }
                other => {
                    return Err(format!(
                        "account_notifications: unexpected token request outcome {other:?}"
                    ));
                }
            }
        }
        other => {
            return Err(format!(
                "account_notifications: unexpected email management {other:?}"
            ));
        }
    }
    println!("account_notifications_email_unsupported=ok");

    let pushers_before = email_pusher_count(raw).await?;
    let state = settle_operation(
        conn,
        AccountNotificationsRequest::EnableEmailNotifications {
            address: "unverified@example.invalid".to_owned(),
            lang: "en".to_owned(),
        },
        "enable unverified email",
    )
    .await?;
    if !matches!(
        state.operation,
        AccountNotificationsOperationState::Failed {
            failure_kind: AccountNotificationsFailureKind::EmailNotRegistered,
            ..
        }
    ) {
        return Err(format!(
            "account_notifications: unverified email enable settled {:?}",
            state.operation
        ));
    }
    if state
        .snapshot
        .as_ref()
        .is_some_and(|snapshot| snapshot.email_notifications_active())
        || email_pusher_count(raw).await? != pushers_before
    {
        return Err("account_notifications: unverified email produced an email pusher".to_owned());
    }
    println!("account_notifications_email_requires_verified=ok");

    // #1427: the keyword write and its raw server readback, through the app's
    // own commands.
    run_keyword_phase(conn, raw).await?;

    println!("account_notifications=ok");
    Ok(())
}

async fn message_notifies(rules: &Ruleset, user_id: &str, mention: bool) -> bool {
    use matrix_sdk::ruma::{OwnedUserId, owned_room_id, push::PushConditionRoomCtx, serde::Raw};
    let Ok(user) = OwnedUserId::try_from(user_id) else {
        return false;
    };
    let Ok(event) = Raw::new(&serde_json::json!({
        "type": "m.room.message",
        "event_id": "$qa:example.invalid",
        "room_id": "!qa:example.invalid",
        "sender": "@other:example.invalid",
        "origin_server_ts": 1,
        "content": {
            "msgtype": "m.text",
            "body": "synthetic qa body",
            "m.mentions": {"user_ids": if mention { vec![user.as_str()] } else { Vec::new() }}
        }
    })) else {
        return false;
    };
    let event = event.cast_unchecked::<matrix_sdk::ruma::events::AnySyncTimelineEvent>();
    let context = PushConditionRoomCtx::new(
        owned_room_id!("!qa:example.invalid"),
        5u32.into(),
        user,
        "QA".to_owned(),
    );
    rules
        .get_actions(&event, &context)
        .await
        .iter()
        .any(Action::should_notify)
}
