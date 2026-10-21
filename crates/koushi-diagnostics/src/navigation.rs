//! Causal navigation metadata with no raw identity or free-form string fields.
//!
//! Core owns the private bounded identity-to-ordinal lookup. Reset this ledger
//! when that lookup is exhausted, and continue its alias serial across resets.
use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use crate::lock_best_effort;

pub const DEFAULT_NAVIGATION_DIAGNOSTIC_CAPACITY: usize = 256;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NavigationCause {
    SelectAccount,
    AccountFallback,
    SelectRoom,
    SelectSpace,
    RoomListReplacement,
    SessionReset,
    StartDm,
    Notification,
    EventNavigation,
    ActivityNavigation,
    SearchNavigation,
    PinnedNavigation,
    StartupRestoration,
    DirectRead,
    DirectSearch,
    DirectAttachment,
    DirectSend,
    #[default]
    Unknown,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NavigationOrigin {
    RendererPointer,
    RendererKeyboard,
    RendererProgrammatic,
    NativeNotification,
    Mcp,
    Core,
    #[default]
    Unknown,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NavigationStage {
    #[default]
    Admitted,
    Committed,
    NoOp,
    Rejected,
    Superseded,
    Completed,
}

/// Numeric aliases are process/session-local ordinals, never identifier hashes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize)]
pub struct NavigationDiagnostic {
    pub cause: NavigationCause,
    pub authoritative_transition: bool,
    pub origin: NavigationOrigin,
    pub stage: NavigationStage,
    pub request_connection: Option<u64>,
    pub request_sequence: Option<u64>,
    pub previous_account_alias: Option<u64>,
    pub requested_account_alias: Option<u64>,
    pub committed_account_alias: Option<u64>,
    pub previous_space_alias: Option<u64>,
    pub requested_space_alias: Option<u64>,
    pub committed_space_alias: Option<u64>,
    pub previous_room_alias: Option<u64>,
    pub requested_room_alias: Option<u64>,
    pub committed_room_alias: Option<u64>,
    pub old_room_present: Option<bool>,
    pub old_dm: Option<bool>,
    pub new_dm: Option<bool>,
    pub list_authoritative: Option<bool>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct NavigationDiagnosticRecord {
    pub sequence: u64,
    pub elapsed_ms: u64,
    #[serde(flatten)]
    pub diagnostic: NavigationDiagnostic,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, serde::Serialize)]
pub struct NavigationDiagnosticSnapshot {
    pub records: Vec<NavigationDiagnosticRecord>,
    pub evicted_records: u64,
    pub reset_count: u64,
}

#[derive(Default)]
struct NavigationDiagnosticState {
    records: VecDeque<NavigationDiagnosticRecord>,
    sequence: u64,
    evicted_records: u64,
    reset_count: u64,
}

/// Independent of the general event ring, so sync noise cannot evict causality.
pub struct NavigationDiagnosticLedger {
    state: Mutex<NavigationDiagnosticState>,
    capacity: usize,
    started: Instant,
}

impl NavigationDiagnosticLedger {
    pub fn new(capacity: usize) -> Self {
        Self {
            state: Mutex::new(NavigationDiagnosticState::default()),
            capacity: capacity.min(DEFAULT_NAVIGATION_DIAGNOSTIC_CAPACITY),
            started: Instant::now(),
        }
    }

    pub fn record(&self, diagnostic: NavigationDiagnostic) {
        let mut state = lock_best_effort(&self.state);
        state.sequence = state.sequence.saturating_add(1);
        if self.capacity == 0 {
            state.evicted_records = state.evicted_records.saturating_add(1);
            return;
        }
        if state.records.len() == self.capacity {
            state.records.pop_front();
            state.evicted_records = state.evicted_records.saturating_add(1);
        }
        let sequence = state.sequence;
        state.records.push_back(NavigationDiagnosticRecord {
            sequence,
            elapsed_ms: self.started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
            diagnostic,
        });
    }

    /// Explicit alias-lookup/session reset. Sequence and lifetime counters survive.
    pub fn reset(&self) {
        let mut state = lock_best_effort(&self.state);
        state.records.clear();
        state.reset_count = state.reset_count.saturating_add(1);
    }

    pub fn snapshot(&self) -> NavigationDiagnosticSnapshot {
        let state = lock_best_effort(&self.state);
        NavigationDiagnosticSnapshot {
            records: state.records.iter().cloned().collect(),
            evicted_records: state.evicted_records,
            reset_count: state.reset_count,
        }
    }
}

static GLOBAL_NAVIGATION_LEDGER: OnceLock<NavigationDiagnosticLedger> = OnceLock::new();

fn global_ledger() -> &'static NavigationDiagnosticLedger {
    GLOBAL_NAVIGATION_LEDGER
        .get_or_init(|| NavigationDiagnosticLedger::new(DEFAULT_NAVIGATION_DIAGNOSTIC_CAPACITY))
}

pub fn record_navigation(diagnostic: NavigationDiagnostic) {
    global_ledger().record(diagnostic);
}

pub fn reset_navigation() {
    global_ledger().reset();
}

pub fn navigation_snapshot() -> NavigationDiagnosticSnapshot {
    global_ledger().snapshot()
}
