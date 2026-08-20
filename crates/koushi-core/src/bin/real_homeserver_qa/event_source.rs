use super::{AppState, CoreConnection, CoreEvent, EventStreamLag};
use std::time::Duration;

#[cfg(any(debug_assertions, test))]
pub(super) type QaEventFuture<'a> = std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<CoreEvent, EventStreamLag>> + Send + 'a>,
>;

#[cfg(any(debug_assertions, test))]
pub(super) trait QaEventSource {
    fn recv_event(&mut self) -> QaEventFuture<'_>;
}

#[cfg(any(debug_assertions, test))]
pub(super) trait QaSnapshotEventSource: QaEventSource {
    fn snapshot(&self) -> AppState;
}

#[cfg(any(debug_assertions, test))]
impl QaEventSource for CoreConnection {
    fn recv_event(&mut self) -> QaEventFuture<'_> {
        Box::pin(CoreConnection::recv_event(self))
    }
}

#[cfg(any(debug_assertions, test))]
impl QaSnapshotEventSource for CoreConnection {
    fn snapshot(&self) -> AppState {
        CoreConnection::snapshot(self)
    }
}

#[cfg(any(debug_assertions, test))]
#[derive(Clone, Copy)]
pub(super) struct QaEventDeadline {
    pub(super) instant: tokio::time::Instant,
}

#[cfg(any(debug_assertions, test))]
impl QaEventDeadline {
    pub(super) fn after(timeout: Duration) -> Self {
        Self {
            instant: tokio::time::Instant::now() + timeout,
        }
    }

    pub(super) async fn recv<S: QaEventSource + ?Sized>(
        self,
        source: &mut S,
    ) -> Result<Result<CoreEvent, EventStreamLag>, tokio::time::error::Elapsed> {
        tokio::time::timeout_at(self.instant, source.recv_event()).await
    }
}
