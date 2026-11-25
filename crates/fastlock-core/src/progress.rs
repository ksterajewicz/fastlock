//! Progress reporting and cancellation.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// The part of an operation that is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Hashing the encrypted file to compare it with the checksum file next
    /// to it.
    Checksum,
    /// Decrypting, decompressing and reading the contents.
    Decrypt,
    /// Finishing: final checks, and moving the output into place.
    Finish,
}

/// Receives progress from a running operation.
///
/// The methods are called from the thread that runs the operation, often many
/// times per second. Keep them cheap.
pub trait Progress: Send + Sync {
    /// A new stage has started.
    fn stage(&self, stage: Stage) {
        let _ = stage;
    }

    /// `done` bytes of the current stage are finished. `total` is the expected
    /// size of the stage when it is known.
    fn advance(&self, done: u64, total: Option<u64>) {
        let _ = (done, total);
    }
}

/// A [`Progress`] that ignores everything.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoProgress;

impl Progress for NoProgress {}

/// Lets another thread cancel a running operation.
///
/// Clones share the same flag.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    /// Creates a token that is not cancelled.
    pub fn new() -> Self {
        Self::default()
    }

    /// Asks the operation to stop. It stops at the next block of data.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// Whether [`CancelToken::cancel`] was called.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Progress and cancellation for one operation.
#[derive(Clone, Copy)]
pub struct Control<'a> {
    /// Where progress is reported.
    pub progress: &'a dyn Progress,
    /// How the operation is cancelled.
    pub cancel: &'a CancelToken,
}

impl<'a> Control<'a> {
    /// Creates a control from its two parts.
    pub fn new(progress: &'a dyn Progress, cancel: &'a CancelToken) -> Self {
        Control { progress, cancel }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clones_of_a_token_share_the_flag() {
        let a = CancelToken::new();
        let b = a.clone();
        assert!(!b.is_cancelled());
        a.cancel();
        assert!(b.is_cancelled());
    }
}
