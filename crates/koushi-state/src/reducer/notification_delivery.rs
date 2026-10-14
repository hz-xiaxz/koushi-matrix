//! #1427: the device-local notification delivery projection.
//!
//! The core observes the delivery channel outside the reducer - it reads the credential
//! store and reaches the network - and hands the result here as one whole app-scoped
//! value. The reducer only installs it, so the frontend's view of "which sinks are
//! configured and how their last send ended" always comes from a state transition.

use crate::{
    effect::AppEffect,
    state::{AppState, NotificationDeliveryState},
};

/// Install the observed delivery projection.
///
/// A projection equal to the current one changes nothing, so re-observing an idle channel
/// produces no state update for the frontend to re-render.
pub(crate) fn handle_observed(
    state: &mut AppState,
    delivery: NotificationDeliveryState,
) -> Vec<AppEffect> {
    if state.notification_delivery != delivery {
        state.notification_delivery = delivery;
    }
    Vec::new()
}
