use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};
use std::time::{SystemTime, UNIX_EPOCH};

pub const DEFAULT_DIAGNOSTIC_CAPACITY: usize = 10_000;
const DEFAULT_ROTATION_DIAGNOSTIC_CAPACITY: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum DiagnosticValue {
    Boolean(bool),
    Count(u64),
    Correlation(u64),
    Milliseconds(u64),
    RequestId { connection_id: u64, sequence: u64 },
    Token(&'static str),
    OrdinalAlias { kind: &'static str, ordinal: u64 },
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct DiagnosticField {
    pub key: &'static str,
    pub value: DiagnosticValue,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct DiagnosticEvent {
    pub level: DiagnosticLevel,
    pub source: &'static str,
    pub stage: &'static str,
    pub fields: Vec<DiagnosticField>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct DiagnosticRecord {
    #[serde(rename = "timestampMs")]
    pub timestamp_ms: u64,
    pub event: DiagnosticEvent,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct DiagnosticSnapshot {
    pub records: Vec<DiagnosticRecord>,
    #[serde(rename = "droppedRecords")]
    pub dropped_records: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RotationBoundaryDiagnostic {
    pub room_alias: u64,
    pub previous_session_alias: Option<u64>,
    pub new_session_alias: Option<u64>,
    pub reason: &'static str,
    pub creation_outcome: &'static str,
    pub first_share_outcome: &'static str,
    pub first_send_correlation_present: bool,
    pub discard_elapsed_ms: Option<u64>,
    pub elapsed_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RotationDiagnosticSnapshot {
    pub records: Vec<DiagnosticRecord>,
    pub dropped_boundaries: u64,
}

struct RotationBoundaryRecord {
    timestamp_ms: u64,
    boundary: RotationBoundaryDiagnostic,
}

#[derive(Default)]
struct RotationDiagnosticState {
    records: VecDeque<RotationBoundaryRecord>,
    dropped_boundaries: u64,
}

pub struct RotationDiagnosticLedger {
    state: Mutex<RotationDiagnosticState>,
    capacity: usize,
}

impl RotationDiagnosticLedger {
    pub fn new(capacity: usize) -> Self {
        Self {
            state: Mutex::new(RotationDiagnosticState {
                records: VecDeque::with_capacity(capacity),
                dropped_boundaries: 0,
            }),
            capacity,
        }
    }

    pub fn record(&self, boundary: RotationBoundaryDiagnostic) {
        self.record_at(timestamp_millis_at(SystemTime::now()), boundary);
    }

    pub fn record_at(&self, timestamp_ms: u64, boundary: RotationBoundaryDiagnostic) {
        let mut state = lock_best_effort(&self.state);
        if self.capacity == 0 {
            state.dropped_boundaries = state.dropped_boundaries.saturating_add(1);
            return;
        }
        if state.records.len() == self.capacity {
            state.records.pop_front();
            state.dropped_boundaries = state.dropped_boundaries.saturating_add(1);
        }
        state.records.push_back(RotationBoundaryRecord {
            timestamp_ms,
            boundary,
        });
    }

    pub fn mark_first_send_correlation(&self, session_alias: u64) -> bool {
        let mut state = lock_best_effort(&self.state);
        let Some(record) = state
            .records
            .iter_mut()
            .rev()
            .find(|record| record.boundary.new_session_alias == Some(session_alias))
        else {
            return false;
        };
        record.boundary.first_send_correlation_present = true;
        true
    }

    pub fn reset(&self) {
        *lock_best_effort(&self.state) = RotationDiagnosticState::default();
    }

    pub fn snapshot(&self) -> RotationDiagnosticSnapshot {
        let state = lock_best_effort(&self.state);
        RotationDiagnosticSnapshot {
            records: state
                .records
                .iter()
                .map(|record| DiagnosticRecord {
                    timestamp_ms: record.timestamp_ms,
                    event: rotation_boundary_event(record.boundary),
                })
                .collect(),
            dropped_boundaries: state.dropped_boundaries,
        }
    }
}

fn rotation_boundary_event(boundary: RotationBoundaryDiagnostic) -> DiagnosticEvent {
    let mut event =
        DiagnosticEvent::new(DiagnosticLevel::Info, "core.room_key_rotation", "boundary")
            .field(DiagnosticField::ordinal_alias(
                "room_alias",
                "room",
                boundary.room_alias,
            ))
            .field(DiagnosticField::token("reason", boundary.reason))
            .field(DiagnosticField::token(
                "creation_outcome",
                boundary.creation_outcome,
            ))
            .field(DiagnosticField::token(
                "first_share_outcome",
                boundary.first_share_outcome,
            ))
            .field(DiagnosticField::boolean(
                "first_send_correlation_present",
                boundary.first_send_correlation_present,
            ))
            .field(DiagnosticField::milliseconds(
                "elapsed_ms",
                boundary.elapsed_ms.into(),
            ));
    if let Some(previous) = boundary.previous_session_alias {
        event = event.field(DiagnosticField::ordinal_alias(
            "previous_session_alias",
            "session",
            previous,
        ));
    }
    if let Some(new) = boundary.new_session_alias {
        event = event.field(DiagnosticField::ordinal_alias(
            "new_session_alias",
            "session",
            new,
        ));
    }
    if let Some(discard_elapsed_ms) = boundary.discard_elapsed_ms {
        event = event.field(DiagnosticField::milliseconds(
            "discard_elapsed_ms",
            discard_elapsed_ms.into(),
        ));
    }
    event
}

impl DiagnosticEvent {
    pub fn new(level: DiagnosticLevel, source: &'static str, stage: &'static str) -> Self {
        Self {
            level,
            source,
            stage,
            fields: Vec::new(),
        }
    }

    pub fn field(mut self, field: DiagnosticField) -> Self {
        self.fields.push(field);
        self
    }
}

impl DiagnosticField {
    pub fn token(key: &'static str, value: &'static str) -> Self {
        Self {
            key,
            value: DiagnosticValue::Token(value),
        }
    }

    pub fn boolean(key: &'static str, value: bool) -> Self {
        Self {
            key,
            value: DiagnosticValue::Boolean(value),
        }
    }

    pub fn count(key: &'static str, value: u64) -> Self {
        Self {
            key,
            value: DiagnosticValue::Count(value),
        }
    }

    pub fn optional_count(key: &'static str, value: Option<u32>) -> Self {
        match value {
            Some(value) => Self::count(key, u64::from(value)),
            None => Self::token(key, "none"),
        }
    }

    pub fn correlation(key: &'static str, value: u64) -> Self {
        Self {
            key,
            value: DiagnosticValue::Correlation(value),
        }
    }

    pub fn milliseconds(key: &'static str, value: u128) -> Self {
        Self {
            key,
            value: DiagnosticValue::Milliseconds(value.min(u64::MAX as u128) as u64),
        }
    }

    pub fn request_id(key: &'static str, connection_id: u64, sequence: u64) -> Self {
        Self {
            key,
            value: DiagnosticValue::RequestId {
                connection_id,
                sequence,
            },
        }
    }

    pub fn ordinal_alias(key: &'static str, kind: &'static str, ordinal: u64) -> Self {
        Self {
            key,
            value: DiagnosticValue::OrdinalAlias { kind, ordinal },
        }
    }
}

pub struct DiagnosticBuffer {
    records: Mutex<VecDeque<DiagnosticRecord>>,
    dropped_records: Mutex<u64>,
    capacity: usize,
}

/// Aggregate diagnostic counters owned by one client/runtime.
///
/// Keeping resets and mutations on this context prevents replacing one
/// account runtime from erasing counters that belong to another runtime.
#[derive(Default)]
pub struct DiagnosticCounterContext {
    counters: Mutex<BTreeMap<&'static str, u64>>,
}

impl DiagnosticCounterContext {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a runtime-owned context that also contributes to the legacy
    /// process-wide diagnostic export while the runtime remains alive.
    pub fn registered() -> Arc<Self> {
        let context = Arc::new(Self::new());
        lock_best_effort(REGISTERED_COUNTER_CONTEXTS.get_or_init(|| Mutex::new(Vec::new())))
            .push(Arc::downgrade(&context));
        context
    }

    pub fn increment(&self, name: &'static str) {
        let mut counters = lock_best_effort(&self.counters);
        let counter = counters.entry(name).or_default();
        *counter = counter.saturating_add(1);
    }

    pub fn reset(&self, name: &'static str) {
        lock_best_effort(&self.counters).remove(name);
    }

    pub fn set(&self, name: &'static str, value: u64) {
        lock_best_effort(&self.counters).insert(name, value);
    }

    /// Snapshot only this client/runtime's aggregate counters.
    pub fn snapshot(&self) -> DiagnosticSnapshot {
        DiagnosticSnapshot {
            records: counter_records(
                &lock_best_effort(&self.counters),
                timestamp_millis_at(SystemTime::now()),
            ),
            dropped_records: 0,
        }
    }

    fn values(&self) -> BTreeMap<&'static str, u64> {
        lock_best_effort(&self.counters).clone()
    }
}

impl DiagnosticBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            records: Mutex::new(VecDeque::with_capacity(capacity)),
            dropped_records: Mutex::new(0),
            capacity,
        }
    }

    pub fn record(&self, event: DiagnosticEvent) {
        self.record_at(timestamp_millis_at(SystemTime::now()), event);
    }

    pub fn record_at(&self, timestamp_ms: u64, event: DiagnosticEvent) {
        let mut records = lock_best_effort(&self.records);
        if self.capacity == 0 {
            increment_dropped(&self.dropped_records);
            return;
        }
        if records.len() == self.capacity {
            records.pop_front();
            increment_dropped(&self.dropped_records);
        }
        records.push_back(DiagnosticRecord {
            timestamp_ms,
            event,
        });
    }

    pub fn record_batch(&self, events: impl IntoIterator<Item = DiagnosticEvent>) {
        self.record_batch_at(timestamp_millis_at(SystemTime::now()), events);
    }

    pub fn record_batch_at(
        &self,
        timestamp_ms: u64,
        events: impl IntoIterator<Item = DiagnosticEvent>,
    ) {
        let mut records = lock_best_effort(&self.records);
        let mut dropped_records = lock_best_effort(&self.dropped_records);
        for event in events {
            if self.capacity == 0 {
                *dropped_records = dropped_records.saturating_add(1);
                continue;
            }
            if records.len() == self.capacity {
                records.pop_front();
                *dropped_records = dropped_records.saturating_add(1);
            }
            records.push_back(DiagnosticRecord {
                timestamp_ms,
                event,
            });
        }
    }

    pub fn snapshot(&self) -> DiagnosticSnapshot {
        let records_guard = lock_best_effort(&self.records);
        let records = records_guard.iter().cloned().collect();
        let dropped_records = *lock_best_effort(&self.dropped_records);
        DiagnosticSnapshot {
            records,
            dropped_records,
        }
    }
}

static GLOBAL_BUFFER: OnceLock<DiagnosticBuffer> = OnceLock::new();
static GLOBAL_ROTATION_LEDGER: OnceLock<RotationDiagnosticLedger> = OnceLock::new();
static GLOBAL_COUNTER_CONTEXT: OnceLock<DiagnosticCounterContext> = OnceLock::new();
static REGISTERED_COUNTER_CONTEXTS: OnceLock<Mutex<Vec<Weak<DiagnosticCounterContext>>>> =
    OnceLock::new();

/// Test-only coordination for assertions against the process-wide diagnostic
/// buffer. Production diagnostics remain concurrent; tests that inspect the
/// global stream must hold this guard across the operation that emits and
/// checks records so parallel tests cannot consume one another's evidence.
#[doc(hidden)]
pub mod test_support {
    use std::sync::{Mutex, MutexGuard, OnceLock};

    use super::{
        DEFAULT_DIAGNOSTIC_CAPACITY, DEFAULT_ROTATION_DIAGNOSTIC_CAPACITY, DiagnosticBuffer,
        DiagnosticSnapshot, GLOBAL_BUFFER, GLOBAL_ROTATION_LEDGER, RotationDiagnosticLedger,
        RotationDiagnosticSnapshot,
    };

    static GLOBAL_DIAGNOSTIC_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    pub fn lock() -> MutexGuard<'static, ()> {
        GLOBAL_DIAGNOSTIC_TEST_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Snapshot only the bounded detail ring. Tests that compare positions
    /// before and after one emission must not include synthesized aggregate
    /// counter records, whose count can change independently of the ring.
    pub fn detail_snapshot() -> DiagnosticSnapshot {
        GLOBAL_BUFFER
            .get_or_init(|| DiagnosticBuffer::new(DEFAULT_DIAGNOSTIC_CAPACITY))
            .snapshot()
    }

    pub fn rotation_snapshot() -> RotationDiagnosticSnapshot {
        GLOBAL_ROTATION_LEDGER
            .get_or_init(|| RotationDiagnosticLedger::new(DEFAULT_ROTATION_DIAGNOSTIC_CAPACITY))
            .snapshot()
    }
}

pub fn record(event: DiagnosticEvent) {
    GLOBAL_BUFFER
        .get_or_init(|| DiagnosticBuffer::new(DEFAULT_DIAGNOSTIC_CAPACITY))
        .record(event);
}

/// Records a structured event and mirrors the same private-data-free format
/// to stderr for startup paths whose UI diagnostics may not yet be reachable.
pub fn record_and_stderr(event: DiagnosticEvent) {
    eprintln!("[koushi] {} {}", event.source, format_event(&event));
    record(event);
}

pub fn record_batch(events: impl IntoIterator<Item = DiagnosticEvent>) {
    GLOBAL_BUFFER
        .get_or_init(|| DiagnosticBuffer::new(DEFAULT_DIAGNOSTIC_CAPACITY))
        .record_batch(events);
}

pub fn record_rotation_boundary(boundary: RotationBoundaryDiagnostic) {
    GLOBAL_ROTATION_LEDGER
        .get_or_init(|| RotationDiagnosticLedger::new(DEFAULT_ROTATION_DIAGNOSTIC_CAPACITY))
        .record(boundary);
}

pub fn mark_rotation_first_send_correlation(session_alias: u64) -> bool {
    GLOBAL_ROTATION_LEDGER
        .get_or_init(|| RotationDiagnosticLedger::new(DEFAULT_ROTATION_DIAGNOSTIC_CAPACITY))
        .mark_first_send_correlation(session_alias)
}

pub fn reset_rotation_ledger() {
    GLOBAL_ROTATION_LEDGER
        .get_or_init(|| RotationDiagnosticLedger::new(DEFAULT_ROTATION_DIAGNOSTIC_CAPACITY))
        .reset();
}

/// Increment a closed, privacy-safe aggregate diagnostic counter. Counter
/// summaries are appended outside the bounded detail ring when exported.
pub fn increment_counter(name: &'static str) {
    GLOBAL_COUNTER_CONTEXT
        .get_or_init(DiagnosticCounterContext::new)
        .increment(name);
}

/// Reset one aggregate counter when its owning account runtime is replaced.
pub fn reset_counter(name: &'static str) {
    GLOBAL_COUNTER_CONTEXT
        .get_or_init(DiagnosticCounterContext::new)
        .reset(name);
}

/// Set an aggregate counter to an absolute value. Used when mirroring an
/// authoritative SDK snapshot so repeated summaries do not inflate the count.
pub fn set_counter(name: &'static str, value: u64) {
    GLOBAL_COUNTER_CONTEXT
        .get_or_init(DiagnosticCounterContext::new)
        .set(name, value);
}

pub fn snapshot() -> DiagnosticSnapshot {
    let mut snapshot = GLOBAL_BUFFER
        .get_or_init(|| DiagnosticBuffer::new(DEFAULT_DIAGNOSTIC_CAPACITY))
        .snapshot();
    let rotation_snapshot = GLOBAL_ROTATION_LEDGER
        .get_or_init(|| RotationDiagnosticLedger::new(DEFAULT_ROTATION_DIAGNOSTIC_CAPACITY))
        .snapshot();
    snapshot.records.extend(rotation_snapshot.records);
    let timestamp_ms = timestamp_millis_at(SystemTime::now());
    snapshot
        .records
        .extend(counter_records(&aggregate_counter_values(), timestamp_ms));
    if rotation_snapshot.dropped_boundaries > 0 {
        snapshot.records.push(DiagnosticRecord {
            timestamp_ms,
            event: DiagnosticEvent::new(DiagnosticLevel::Info, "core.room_key_summary", "counter")
                .field(DiagnosticField::token(
                    "name",
                    "rotation_boundaries_dropped",
                ))
                .field(DiagnosticField::count(
                    "count",
                    rotation_snapshot.dropped_boundaries,
                )),
        });
    }
    snapshot
}

fn aggregate_counter_values() -> BTreeMap<&'static str, u64> {
    let mut totals = GLOBAL_COUNTER_CONTEXT
        .get_or_init(DiagnosticCounterContext::new)
        .values();
    let mut registered =
        lock_best_effort(REGISTERED_COUNTER_CONTEXTS.get_or_init(|| Mutex::new(Vec::new())));
    registered.retain(|context| {
        let Some(context) = context.upgrade() else {
            return false;
        };
        for (name, count) in context.values() {
            let total = totals.entry(name).or_default();
            *total = total.saturating_add(count);
        }
        true
    });
    totals
}

fn counter_records(
    counters: &BTreeMap<&'static str, u64>,
    timestamp_ms: u64,
) -> Vec<DiagnosticRecord> {
    counters
        .iter()
        .map(|(name, count)| DiagnosticRecord {
            timestamp_ms,
            event: DiagnosticEvent::new(DiagnosticLevel::Info, "core.room_key_summary", "counter")
                .field(DiagnosticField::token("name", name))
                .field(DiagnosticField::count("count", *count)),
        })
        .collect()
}

pub fn format_event(event: &DiagnosticEvent) -> String {
    let mut line = format!("stage={}", event.stage);
    for field in &event.fields {
        line.push(' ');
        line.push_str(field.key);
        line.push('=');
        match &field.value {
            DiagnosticValue::Boolean(value) => line.push_str(if *value { "true" } else { "false" }),
            DiagnosticValue::Count(value) | DiagnosticValue::Milliseconds(value) => {
                line.push_str(&value.to_string())
            }
            DiagnosticValue::Correlation(value) => line.push_str(&format!("send-{value}")),
            DiagnosticValue::RequestId {
                connection_id,
                sequence,
            } => line.push_str(&format!("{}:{}", connection_id, sequence)),
            DiagnosticValue::Token(value) => line.push_str(value),
            DiagnosticValue::OrdinalAlias { kind, ordinal } => {
                line.push_str(kind);
                line.push('-');
                line.push_str(&ordinal.to_string());
            }
        }
    }
    line
}

fn timestamp_millis_at(now: SystemTime) -> u64 {
    now.duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(0)
}

fn lock_best_effort<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn increment_dropped(counter: &Mutex<u64>) {
    let mut counter = lock_best_effort(counter);
    *counter = counter.saturating_add(1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn event(stage: &'static str) -> DiagnosticEvent {
        DiagnosticEvent::new(DiagnosticLevel::Debug, "test", stage)
    }

    #[test]
    fn keeps_latest_records_and_reports_drops() {
        let buffer = DiagnosticBuffer::new(2);
        buffer.record_at(1, event("one"));
        buffer.record_at(2, event("two"));
        buffer.record_at(3, event("three"));

        let snapshot = buffer.snapshot();
        assert_eq!(snapshot.dropped_records, 1);
        assert_eq!(
            snapshot
                .records
                .iter()
                .map(|record| record.event.stage)
                .collect::<Vec<_>>(),
            vec!["two", "three"]
        );
    }

    #[test]
    fn records_concurrently_without_exceeding_capacity() {
        let buffer = Arc::new(DiagnosticBuffer::new(64));
        let workers = (0..8)
            .map(|_| {
                let buffer = Arc::clone(&buffer);
                std::thread::spawn(move || {
                    for index in 0..100 {
                        buffer.record_at(index, event("concurrent"));
                    }
                })
            })
            .collect::<Vec<_>>();
        for worker in workers {
            worker.join().unwrap();
        }
        let snapshot = buffer.snapshot();
        assert_eq!(snapshot.records.len(), 64);
        assert_eq!(snapshot.dropped_records, 736);
    }

    #[test]
    fn batch_records_share_timestamp_and_preserve_order() {
        let buffer = DiagnosticBuffer::new(4);

        buffer.record_batch_at(
            42,
            [event("batch_one"), event("batch_two"), event("batch_three")],
        );

        let snapshot = buffer.snapshot();
        assert_eq!(snapshot.dropped_records, 0);
        assert_eq!(
            snapshot
                .records
                .iter()
                .map(|record| (record.timestamp_ms, record.event.stage))
                .collect::<Vec<_>>(),
            vec![(42, "batch_one"), (42, "batch_two"), (42, "batch_three")]
        );
    }

    #[test]
    fn batch_keeps_latest_records_and_counts_every_drop() {
        let buffer = DiagnosticBuffer::new(2);
        buffer.record_at(1, event("existing"));

        buffer.record_batch_at(2, [event("one"), event("two"), event("three")]);

        let snapshot = buffer.snapshot();
        assert_eq!(snapshot.dropped_records, 2);
        assert_eq!(
            snapshot
                .records
                .iter()
                .map(|record| (record.timestamp_ms, record.event.stage))
                .collect::<Vec<_>>(),
            vec![(2, "two"), (2, "three")]
        );
    }

    #[test]
    fn aggregate_counter_is_exported_outside_the_bounded_detail_ring() {
        let _guard = test_support::lock();
        reset_counter("synthetic_room_key_counter");
        increment_counter("synthetic_room_key_counter");
        increment_counter("synthetic_room_key_counter");

        let snapshot = super::snapshot();
        let summary = snapshot
            .records
            .iter()
            .find(|record| {
                record.event.source == "core.room_key_summary"
                    && record.event.fields.iter().any(|field| {
                        field.key == "name"
                            && field.value == DiagnosticValue::Token("synthetic_room_key_counter")
                    })
            })
            .expect("aggregate summary remains exportable independently of the detail ring");
        assert!(
            summary
                .event
                .fields
                .iter()
                .any(|field| { field.key == "count" && field.value == DiagnosticValue::Count(2) })
        );
        reset_counter("synthetic_room_key_counter");
    }

    #[test]
    fn runtime_counter_contexts_reset_independently() {
        let first = DiagnosticCounterContext::new();
        let second = DiagnosticCounterContext::new();
        first.increment("runtime_counter");
        second.increment("runtime_counter");
        second.increment("runtime_counter");

        first.reset("runtime_counter");

        assert!(first.snapshot().records.is_empty());
        let second_snapshot = second.snapshot();
        assert!(second_snapshot.records.iter().any(|record| {
            record.event.fields.iter().any(|field| {
                field.key == "name" && field.value == DiagnosticValue::Token("runtime_counter")
            }) && record
                .event
                .fields
                .iter()
                .any(|field| field.key == "count" && field.value == DiagnosticValue::Count(2))
        }));
    }

    #[test]
    fn concurrent_batches_remain_bounded_and_count_drops() {
        let buffer = Arc::new(DiagnosticBuffer::new(64));
        let workers = (0..8)
            .map(|worker| {
                let buffer = Arc::clone(&buffer);
                std::thread::spawn(move || {
                    buffer.record_batch_at(worker, (0..100).map(|_| event("concurrent_batch")));
                })
            })
            .collect::<Vec<_>>();
        for worker in workers {
            worker.join().unwrap();
        }

        let snapshot = buffer.snapshot();
        assert_eq!(snapshot.records.len(), 64);
        assert_eq!(snapshot.dropped_records, 736);
        assert!(
            snapshot
                .records
                .windows(2)
                .all(|records| { records[0].timestamp_ms == records[1].timestamp_ms })
        );
    }

    #[test]
    fn large_batch_retains_only_the_latest_capacity_without_timing_assumptions() {
        let buffer = DiagnosticBuffer::new(1_000);

        buffer.record_batch_at(7, (0..25_000).map(|_| event("large_batch")));

        let snapshot = buffer.snapshot();
        assert_eq!(snapshot.records.len(), 1_000);
        assert_eq!(snapshot.dropped_records, 24_000);
        assert!(
            snapshot
                .records
                .iter()
                .all(|record| record.timestamp_ms == 7)
        );
    }

    #[test]
    fn formats_only_structured_fields() {
        let line = format_event(
            &DiagnosticEvent::new(DiagnosticLevel::Debug, "core.timeline", "actor_finish")
                .field(DiagnosticField::token("operation", "send_reaction"))
                .field(DiagnosticField::milliseconds("elapsed_ms", 42))
                .field(DiagnosticField::boolean("success", true)),
        );
        assert_eq!(
            line,
            "stage=actor_finish operation=send_reaction elapsed_ms=42 success=true"
        );
    }

    #[test]
    fn recovers_after_records_mutex_poisoning() {
        let buffer = Arc::new(DiagnosticBuffer::new(1));
        let poisoned_buffer = Arc::clone(&buffer);
        let poisoner = std::thread::spawn(move || {
            let _records = poisoned_buffer.records.lock().unwrap();
            panic!("poison records mutex");
        });
        assert!(poisoner.join().is_err());

        buffer.record_at(7, event("after_records_poison"));

        let snapshot = buffer.snapshot();
        assert_eq!(snapshot.dropped_records, 0);
        assert_eq!(snapshot.records.len(), 1);
        assert_eq!(snapshot.records[0].event.stage, "after_records_poison");
    }

    #[test]
    fn recovers_after_dropped_counter_mutex_poisoning() {
        let buffer = Arc::new(DiagnosticBuffer::new(1));
        buffer.record_at(1, event("first"));

        let poisoned_buffer = Arc::clone(&buffer);
        let poisoner = std::thread::spawn(move || {
            let _dropped_records = poisoned_buffer.dropped_records.lock().unwrap();
            panic!("poison dropped counter mutex");
        });
        assert!(poisoner.join().is_err());

        buffer.record_at(2, event("second"));

        let snapshot = buffer.snapshot();
        assert_eq!(snapshot.dropped_records, 1);
        assert_eq!(snapshot.records[0].event.stage, "second");
    }

    #[test]
    fn clamps_pre_epoch_timestamp_to_zero() {
        let before_epoch = UNIX_EPOCH - std::time::Duration::from_millis(1);
        assert_eq!(timestamp_millis_at(before_epoch), 0);
    }

    #[test]
    fn zero_capacity_drops_every_record() {
        let buffer = DiagnosticBuffer::new(0);
        buffer.record_at(1, event("dropped"));

        let snapshot = buffer.snapshot();
        assert!(snapshot.records.is_empty());
        assert_eq!(snapshot.dropped_records, 1);
    }

    #[test]
    fn saturates_maximum_millisecond_duration() {
        assert_eq!(
            DiagnosticField::milliseconds("elapsed_ms", u128::MAX).value,
            DiagnosticValue::Milliseconds(u64::MAX)
        );
    }

    fn rotation_boundary(
        room_alias: u64,
        session_alias: u64,
        reason: &'static str,
    ) -> RotationBoundaryDiagnostic {
        RotationBoundaryDiagnostic {
            room_alias,
            previous_session_alias: None,
            new_session_alias: Some(session_alias),
            reason,
            creation_outcome: "created",
            first_share_outcome: "pending",
            first_send_correlation_present: false,
            discard_elapsed_ms: None,
            elapsed_ms: 3,
        }
    }

    #[test]
    fn rotation_ledger_survives_general_ring_overflow_and_updates_one_session() {
        let detail = DiagnosticBuffer::new(1);
        let ledger = RotationDiagnosticLedger::new(2);
        ledger.record_at(10, rotation_boundary(1, 11, "expired_time"));
        ledger.record_at(11, rotation_boundary(2, 12, "explicit_discard"));
        for index in 0..10 {
            detail.record_at(index, event("churn"));
        }
        assert_eq!(detail.snapshot().dropped_records, 9);

        assert!(ledger.mark_first_send_correlation(11));
        let snapshot = ledger.snapshot();
        assert_eq!(snapshot.dropped_boundaries, 0);
        assert_eq!(snapshot.records.len(), 2);
        let first = &snapshot.records[0];
        let second = &snapshot.records[1];
        assert!(first.event.fields.iter().any(|field| {
            field.key == "first_send_correlation_present"
                && field.value == DiagnosticValue::Boolean(true)
        }));
        assert!(second.event.fields.iter().any(|field| {
            field.key == "first_send_correlation_present"
                && field.value == DiagnosticValue::Boolean(false)
        }));
    }

    #[test]
    fn rotation_ledger_evicts_oldest_and_reset_clears_drop_count() {
        let ledger = RotationDiagnosticLedger::new(2);
        ledger.record_at(1, rotation_boundary(1, 1, "initial"));
        ledger.record_at(2, rotation_boundary(2, 2, "expired_message_count"));
        ledger.record_at(3, rotation_boundary(3, 3, "invalidated"));

        let snapshot = ledger.snapshot();
        assert_eq!(snapshot.dropped_boundaries, 1);
        assert_eq!(snapshot.records.len(), 2);
        assert!(snapshot.records.iter().all(|record| {
            !record.event.fields.iter().any(|field| {
                field.key == "new_session_alias"
                    && field.value
                        == DiagnosticValue::OrdinalAlias {
                            kind: "session",
                            ordinal: 1,
                        }
            })
        }));

        ledger.reset();
        let reset = ledger.snapshot();
        assert!(reset.records.is_empty());
        assert_eq!(reset.dropped_boundaries, 0);
    }

    #[test]
    fn exported_snapshot_includes_rotation_ledger_and_its_drop_counter() {
        let _guard = test_support::lock();
        reset_rotation_ledger();
        for session in 1..=129 {
            record_rotation_boundary(rotation_boundary(session, session, "expired_time"));
        }

        let snapshot = super::snapshot();
        assert!(snapshot.records.iter().any(|record| {
            record.event.source == "core.room_key_rotation"
                && record.event.fields.iter().any(|field| {
                    field.key == "new_session_alias"
                        && field.value
                            == DiagnosticValue::OrdinalAlias {
                                kind: "session",
                                ordinal: 129,
                            }
                })
        }));
        assert!(snapshot.records.iter().any(|record| {
            record.event.source == "core.room_key_summary"
                && record.event.fields.iter().any(|field| {
                    field.key == "name"
                        && field.value == DiagnosticValue::Token("rotation_boundaries_dropped")
                })
                && record
                    .event
                    .fields
                    .iter()
                    .any(|field| field.key == "count" && field.value == DiagnosticValue::Count(1))
        }));
        reset_rotation_ledger();
    }

    #[test]
    fn rotation_ledger_exports_only_closed_private_data_free_fields() {
        let ledger = RotationDiagnosticLedger::new(1);
        ledger.record_at(1, rotation_boundary(4, 5, "membership_or_device_change"));
        let encoded = format!("{:?}", ledger.snapshot().records);
        for forbidden in [
            "room_id",
            "event_id",
            "session_id",
            "device_id",
            "user_id",
            "fingerprint",
            "ciphertext",
            "sender_key",
            "identity_key",
            "raw_error",
            "example.invalid",
        ] {
            assert!(!encoded.contains(forbidden), "privacy leak: {forbidden}");
        }
    }
}
