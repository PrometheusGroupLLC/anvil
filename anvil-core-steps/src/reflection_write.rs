//! Fault-injecting `ReflectionWritePort` adapter for use in brine step definitions.
//!
//! ## Adapter taxonomy
//!
//! Two families of `ReflectionWritePort` implementations exist in the workspace:
//!
//! - **Production-shape in-memory adapters** (`InMemoryReflectionWriteAdapter` in
//!   `anvil-core-hearth/src/`) — implement the same port trait as the filesystem
//!   adapters, record written files in memory for domain-seam assertions, and are
//!   suitable for pure-domain feature tests that do not need filesystem setup.
//!
//! - **Fault-injecting adapters** (this module, `anvil-test-support/src/`) — test
//!   infrastructure for step definitions that exercise failure paths.  They are never
//!   consumed by production code.  The two families serve different purposes; do not
//!   conflate them.
//!
//! ## Usage
//!
//! `FaultyReflectionWriteAdapter` wraps an inner `ReflectionWritePort` (typically
//! an `InMemoryReflectionWriteAdapter`) and intercepts calls when a fault has been
//! armed.  The step definition `given the reflection write adapter will fail on the
//! next call with {string}` arms the fault; the next call to `write_reflection_file`
//! returns `ReflectionWriteError::IoError` with the supplied message.  Subsequent
//! calls are forwarded to the inner adapter.

use anvil_core_hearth::in_memory_reflection_write_adapter::InMemoryReflectionWriteAdapter;
use anvil_core::ports::reflection_write_port::{ReflectionWriteError, ReflectionWritePort};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// A `ReflectionWritePort` that can be armed to fail a configurable number of
/// times on the next call(s).
///
/// Thread-safe: the fault counter and the inner adapter are both guarded.
pub struct FaultyReflectionWriteAdapter {
    /// Pending fault messages — each item is consumed (popped front) on the
    /// next call.  When the queue is empty, calls are forwarded to `inner`.
    pending_faults: Mutex<std::collections::VecDeque<String>>,
    /// Counts the total number of calls made (including failed ones).
    call_count: AtomicUsize,
    /// Inner adapter that receives non-faulted calls.
    inner: Arc<InMemoryReflectionWriteAdapter>,
}

impl FaultyReflectionWriteAdapter {
    /// Create a new, unarmed adapter backed by a fresh in-memory store.
    pub fn new() -> Self {
        Self {
            pending_faults: Mutex::new(std::collections::VecDeque::new()),
            call_count: AtomicUsize::new(0),
            inner: Arc::new(InMemoryReflectionWriteAdapter::new()),
        }
    }

    /// Arm the adapter to fail the next call with `message`.  Multiple calls
    /// to `arm` queue additional faults (each consumed in order).
    pub fn arm(&self, message: String) {
        self.pending_faults.lock().unwrap().push_back(message);
    }

    /// Return the total number of `write_reflection_file` calls made so far,
    /// including calls that returned an error.
    pub fn call_count(&self) -> usize {
        self.call_count.load(Ordering::SeqCst)
    }

    /// Access the inner in-memory adapter for inspection (e.g., call-count on
    /// the success path when the fault has been consumed).
    pub fn inner(&self) -> &InMemoryReflectionWriteAdapter {
        &self.inner
    }
}

impl Default for FaultyReflectionWriteAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ReflectionWritePort for FaultyReflectionWriteAdapter {
    fn write_reflection_file(
        &self,
        artifact_path: &str,
        source_state: &str,
        filename: &str,
        body: &str,
    ) -> Result<String, ReflectionWriteError> {
        self.call_count.fetch_add(1, Ordering::SeqCst);

        // Check for a pending fault.
        let fault_msg = {
            let mut queue = self.pending_faults.lock().unwrap();
            queue.pop_front()
        };

        if let Some(msg) = fault_msg {
            // Build the path string that the adapter would have written to,
            // so callers can assert on the path substring in the error.
            let path = format!("{}/{}_reflection/{}", artifact_path, source_state, filename);
            return Err(ReflectionWriteError::IoError { path, message: msg });
        }

        // No fault — delegate to inner adapter.
        self.inner
            .write_reflection_file(artifact_path, source_state, filename, body)
    }
}
