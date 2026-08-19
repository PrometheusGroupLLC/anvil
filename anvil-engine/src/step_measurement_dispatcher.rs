//! Ordered, fail-open dispatch for durable lean step measurements.
//!
//! Lifecycle RPCs build the complete redacted record, then hand it to this
//! bounded queue with `try_send`. A single dedicated OS thread performs the
//! synchronous filesystem append, preserving FIFO order for accepted rows
//! without occupying or blocking a Tokio worker. Full, disconnected, or
//! failing sinks are telemetry failures only: the originating transition has
//! already proceeded and is never failed or delayed by the durable writer.

use anvil_core_hearth::fs_step_measurement_adapter::FileSystemStepMeasurementAdapter;
use anvil_core::ports::step_measurement_port::{
    StepMeasurementError, StepMeasurementRecord, StepMeasurementWritePort,
};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::Arc;

pub const STEP_MEASUREMENT_QUEUE_CAPACITY: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnqueueOutcome {
    Enqueued,
    DroppedFull,
    DroppedDisconnected,
}

#[derive(Clone)]
pub struct StepMeasurementDispatcher {
    sender: SyncSender<DispatchItem>,
}

struct DispatchItem {
    hearth: PathBuf,
    record: StepMeasurementRecord,
}

type AppendRecord = dyn Fn(&Path, &StepMeasurementRecord) -> Result<(), StepMeasurementError>
    + Send
    + Sync
    + 'static;

impl StepMeasurementDispatcher {
    /// Start the production, multi-hearth filesystem dispatcher.
    pub fn filesystem() -> Self {
        let append: Arc<AppendRecord> = Arc::new(|hearth, record| {
            FileSystemStepMeasurementAdapter::new(hearth).append_step_measurement(record)
        });
        Self::spawn(
            configured_queue_capacity(),
            append,
            configured_parking_control(),
        )
    }

    /// Start a dispatcher over an injected fixed writer.
    ///
    /// This is the test seam for a parking writer. Production uses
    /// [`Self::filesystem`] because one server may resolve multiple hearths.
    pub fn with_writer(writer: Arc<dyn StepMeasurementWritePort>, queue_capacity: usize) -> Self {
        let append: Arc<AppendRecord> =
            Arc::new(move |_hearth, record| writer.append_step_measurement(record));
        Self::spawn(queue_capacity.max(1), append, None)
    }

    fn spawn(
        queue_capacity: usize,
        append: Arc<AppendRecord>,
        parking: Option<TestParkingControl>,
    ) -> Self {
        let (sender, receiver) = mpsc::sync_channel::<DispatchItem>(queue_capacity.max(1));
        let spawn_result = std::thread::Builder::new()
            .name("anvil-step-measurement-writer".to_string())
            .spawn(move || {
                let mut first_append = true;
                let mut delivered = 0_u64;
                for item in receiver {
                    if first_append {
                        first_append = false;
                        park_first_append(parking.as_ref());
                    }

                    let result = append(&item.hearth, &item.record);
                    if let Err(error) = &result {
                        tracing::warn!(
                            outcome = "step_measurement_append_failed",
                            error = %error,
                            "step-measurement sink append failed (non-fatal)"
                        );
                    }
                    delivered = delivered.saturating_add(1);
                    mark_delivery(parking.as_ref(), delivered, result.is_ok());
                }
            });

        if let Err(error) = spawn_result {
            tracing::warn!(
                outcome = "step_measurement_dispatch_start_failed",
                error = %error,
                "step-measurement dispatcher could not start (non-fatal)"
            );
        }

        Self { sender }
    }

    /// Attempt to enqueue one fully built record without waiting for sink I/O.
    pub fn try_enqueue(&self, hearth: &Path, record: StepMeasurementRecord) -> EnqueueOutcome {
        let item = DispatchItem {
            hearth: hearth.to_path_buf(),
            record,
        };
        match self.sender.try_send(item) {
            Ok(()) => EnqueueOutcome::Enqueued,
            Err(TrySendError::Full(_)) => {
                tracing::warn!(
                    outcome = "step_measurement_queue_full",
                    "step-measurement queue is full; dropping record (non-fatal)"
                );
                EnqueueOutcome::DroppedFull
            }
            Err(TrySendError::Disconnected(_)) => {
                tracing::warn!(
                    outcome = "step_measurement_dispatch_disconnected",
                    "step-measurement dispatcher is disconnected; dropping record (non-fatal)"
                );
                EnqueueOutcome::DroppedDisconnected
            }
        }
    }
}

#[derive(Clone)]
struct TestParkingControl {
    directory: PathBuf,
}

#[cfg(debug_assertions)]
fn configured_queue_capacity() -> usize {
    std::env::var("ANVIL_TEST_STEP_MEASUREMENT_QUEUE_CAPACITY")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|capacity| *capacity > 0)
        .unwrap_or(STEP_MEASUREMENT_QUEUE_CAPACITY)
}

#[cfg(not(debug_assertions))]
fn configured_queue_capacity() -> usize {
    STEP_MEASUREMENT_QUEUE_CAPACITY
}

#[cfg(debug_assertions)]
fn configured_parking_control() -> Option<TestParkingControl> {
    std::env::var_os("ANVIL_TEST_STEP_MEASUREMENT_PARK_DIR")
        .filter(|value| !value.is_empty())
        .map(|directory| TestParkingControl {
            directory: PathBuf::from(directory),
        })
}

#[cfg(not(debug_assertions))]
fn configured_parking_control() -> Option<TestParkingControl> {
    None
}

#[cfg(debug_assertions)]
fn park_first_append(control: Option<&TestParkingControl>) {
    let Some(control) = control else {
        return;
    };
    let parked = control.directory.join("parked");
    if let Err(error) = std::fs::write(&parked, b"parked\n") {
        tracing::warn!(
            outcome = "step_measurement_test_park_marker_failed",
            error = %error,
            "step-measurement test parking marker failed; continuing (non-fatal)"
        );
        return;
    }

    let release = control.directory.join("release");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !release.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    if !release.exists() {
        tracing::warn!(
            outcome = "step_measurement_test_park_timeout",
            "step-measurement test writer timed out waiting for release; continuing (non-fatal)"
        );
    }
}

#[cfg(not(debug_assertions))]
fn park_first_append(_control: Option<&TestParkingControl>) {}

#[cfg(debug_assertions)]
fn mark_delivery(control: Option<&TestParkingControl>, delivered: u64, succeeded: bool) {
    let Some(control) = control else {
        return;
    };
    let result = if succeeded { "ok" } else { "error" };
    let marker = format!("{}:{}\n", delivered, result);
    if let Err(error) = std::fs::write(control.directory.join("drained"), marker) {
        tracing::warn!(
            outcome = "step_measurement_test_drain_marker_failed",
            error = %error,
            "step-measurement test drain marker failed (non-fatal)"
        );
    }
}

#[cfg(not(debug_assertions))]
fn mark_delivery(_control: Option<&TestParkingControl>, _delivered: u64, _succeeded: bool) {}
