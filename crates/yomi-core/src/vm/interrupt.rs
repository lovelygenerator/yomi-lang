//! The token to interrupt a running VM from another thread.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// The token to interrupt a `run` from another thread (`spec/host/01-execution.md`, 1.9a).
///
/// Cloning it gives another handle to the same token, so one token can stop many VMs. An interrupt stays requested until
/// [`InterruptHandle::reset`] is called.
#[derive(Debug, Clone, Default)]
pub struct InterruptHandle {
    flag: Arc<AtomicBool>,
}

impl InterruptHandle {
    /// Makes a token with no interrupt requested.
    pub fn new() -> Self {
        Self::default()
    }

    /// Requests an interrupt. The VMs that watch this token return `Interrupted` at their next check point.
    pub fn interrupt(&self) {
        self.flag.store(true, Ordering::Relaxed);
    }

    /// Whether an interrupt is requested.
    pub fn is_interrupted(&self) -> bool {
        self.flag.load(Ordering::Relaxed)
    }

    /// Cancels the request.
    pub fn reset(&self) {
        self.flag.store(false, Ordering::Relaxed);
    }
}
