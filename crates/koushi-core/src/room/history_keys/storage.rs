//! A cancelled write has an unknown durable outcome. Poison before its future
//! is dropped, so the session worker never writes a newer ledger over it.
use super::{AtomicBool, Failure, Ordering};

pub(super) async fn persist(
    poisoned: &AtomicBool,
    write: impl std::future::Future<Output = Result<(), Failure>>,
) -> Result<(), Failure> {
    let mut guard = WriteGuard {
        poisoned,
        complete: false,
    };
    let result = write.await;
    guard.complete = result.is_ok();
    result
}

struct WriteGuard<'a> {
    poisoned: &'a AtomicBool,
    complete: bool,
}
impl Drop for WriteGuard<'_> {
    fn drop(&mut self) {
        if !self.complete {
            self.poisoned.store(true, Ordering::Release);
        }
    }
}

#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;
