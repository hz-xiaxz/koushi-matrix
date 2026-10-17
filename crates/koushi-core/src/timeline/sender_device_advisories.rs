//! Sender device advisories (#1494).
//!
//! State-machine contract: docs/architecture/state-machine.md, "Sender Device
//! Advisories". A Room `TimelineActor` publishes the distinct senders of its
//! loaded window into a watch channel; one observer task per actor owns the
//! SDK device and identity streams and the store read, so a slow read never
//! delays timeline diffs. The task lives in the actor's `auxiliary_tasks` and
//! is aborted with it, dropping its SDK handles inside the runtime.

use std::collections::BTreeSet;
use std::sync::Arc;

use futures_util::{FutureExt, StreamExt};
use koushi_protocol::event::TimelineItem;
use koushi_protocol::ids::TimelineKey;
use koushi_sdk::{MatrixClientSession, SenderDeviceChanges};
use koushi_state::AppAction;
use tokio::sync::{mpsc, watch};

use super::navigation::{TimelineActorGenerationGate, send_generation_fenced};

/// Classified senders per room window; senders beyond the cap show no mark.
pub(super) const SENDER_DEVICE_ADVISORY_SENDER_CAP: usize = 64;

/// The visible distinct senders of `items`, newest first, excluding the viewing user
/// and ignored users, capped at [`SENDER_DEVICE_ADVISORY_SENDER_CAP`].
pub(super) fn window_senders(
    items: &[TimelineItem],
    own_user_id: Option<&str>,
    ignored_user_ids: &BTreeSet<String>,
) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut senders = Vec::new();
    for sender in items
        .iter()
        .rev()
        .filter(|item| !item.is_hidden)
        .filter_map(|item| item.sender.as_deref())
    {
        if Some(sender) == own_user_id || ignored_user_ids.contains(sender) {
            continue;
        }
        if seen.insert(sender) {
            senders.push(sender.to_owned());
            if senders.len() == SENDER_DEVICE_ADVISORY_SENDER_CAP {
                break;
            }
        }
    }
    senders
}

enum Wake {
    Senders,
    Store(Option<BTreeSet<String>>),
    StoreClosed,
    Stopped,
}

async fn next_store_change(changes: &mut Option<SenderDeviceChanges>) -> Wake {
    match changes {
        Some(stream) => match stream.next().await {
            Some(named) => Wake::Store(named),
            None => Wake::StoreClosed,
        },
        None => std::future::pending().await,
    }
}

/// Whether a coalesced store change named someone in `senders`; `None`
/// (an update that named nobody) touches everyone.
fn touches(named: &Option<BTreeSet<String>>, senders: &[String]) -> bool {
    named
        .as_ref()
        .is_none_or(|named| senders.iter().any(|sender| named.contains(sender)))
}

pub(super) async fn run_sender_device_advisories(
    session: Arc<MatrixClientSession>,
    action_tx: mpsc::Sender<Vec<AppAction>>,
    timeline_actor_generations: Arc<TimelineActorGenerationGate>,
    key: TimelineKey,
    actor_generation: u64,
    room_id: String,
    mut senders_rx: watch::Receiver<Vec<String>>,
) {
    // Subscribe before the first read so a concurrent store write is not
    // missed. Without the streams the actor still classifies on window
    // changes.
    let mut changes = koushi_sdk::observe_sender_device_changes(&session)
        .await
        .ok();
    let mut last: Option<BTreeSet<String>> = None;
    let mut dirty = true;
    loop {
        if !dirty {
            let wake = tokio::select! {
                changed = senders_rx.changed() => {
                    if changed.is_err() { Wake::Stopped } else { Wake::Senders }
                }
                wake = next_store_change(&mut changes) => wake,
            };
            match wake {
                Wake::Stopped => break,
                Wake::Senders => {}
                Wake::StoreClosed => {
                    changes = None;
                    continue;
                }
                Wake::Store(mut named) => {
                    // Coalesce a burst of store writes into one read.
                    if let Some(stream) = changes.as_mut() {
                        while let Some(Some(more)) = stream.next().now_or_never() {
                            named = match (named, more) {
                                (Some(mut named), Some(more)) => {
                                    named.extend(more);
                                    Some(named)
                                }
                                _ => None,
                            };
                        }
                    }
                    if !touches(&named, &senders_rx.borrow()) {
                        continue;
                    }
                }
            }
        }
        dirty = false;
        let senders = senders_rx.borrow_and_update().clone();
        let Ok(user_ids) =
            koushi_sdk::read_room_sender_device_advisories(&session, &room_id, &senders).await
        else {
            // A failed store read keeps the last projected set.
            continue;
        };
        if senders_rx.has_changed().unwrap_or(false) {
            // The result is for an older sender set.
            dirty = true;
            continue;
        }
        // The first result always projects, so a replacement actor corrects
        // whatever its predecessor left.
        if last.as_ref() == Some(&user_ids) {
            continue;
        }
        if !send_generation_fenced(
            &action_tx,
            &timeline_actor_generations,
            &key,
            actor_generation,
            vec![AppAction::SenderDeviceAdvisoriesUpdated {
                room_id: room_id.clone(),
                user_ids: user_ids.clone(),
            }],
        )
        .await
        {
            break;
        }
        last = Some(user_ids);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(sender: &str) -> TimelineItem {
        crate::timeline::test_support::timeline_item("$event:example.test", None, sender, false)
    }

    #[test]
    fn window_senders_are_distinct_newest_first_without_self_or_ignored() {
        let items = vec![
            item("@carol:example.test"),
            item("@me:example.test"),
            item("@bob:example.test"),
            item("@mallory:example.test"),
            item("@carol:example.test"),
        ];
        let ignored = BTreeSet::from(["@mallory:example.test".to_owned()]);
        assert_eq!(
            window_senders(&items, Some("@me:example.test"), &ignored),
            vec!["@carol:example.test", "@bob:example.test"]
        );
    }

    #[test]
    fn window_senders_stop_at_the_cap() {
        let items: Vec<_> = (0..100)
            .map(|index| item(&format!("@user{index}:example.test")))
            .collect();
        let senders = window_senders(&items, None, &BTreeSet::new());
        assert_eq!(senders.len(), SENDER_DEVICE_ADVISORY_SENDER_CAP);
        assert_eq!(senders[0], "@user99:example.test");
    }

    #[test]
    fn a_change_that_names_nobody_touches_every_sender() {
        let senders = vec!["@bob:example.test".to_owned()];
        assert!(touches(&None, &senders));
        assert!(touches(
            &Some(BTreeSet::from(["@bob:example.test".to_owned()])),
            &senders
        ));
        assert!(!touches(
            &Some(BTreeSet::from(["@carol:example.test".to_owned()])),
            &senders
        ));
    }
}
