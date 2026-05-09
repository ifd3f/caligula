use std::{fs::File, path::PathBuf, sync::Arc, time::Instant};

use bytesize::ByteSize;
use tracing::{info, trace};

use crate::{
    byteseries::{ByteSeries, EstimatedTime},
    compression::CompressionFormat,
    device::WriteTarget,
    facade::{DaemonError, workflow::WorkflowState},
    herder_api::write_verify::*,
};

/// Params for starting a write + verify workflow.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct WriteVerifyWorkflow {
    pub input_file: PathBuf,
    pub input_file_size: ByteSize,
    pub compression: CompressionFormat,
    pub target: WriteTarget,
}

impl super::Workflow for WriteVerifyWorkflow {
    type State = WVState;
}

impl WriteVerifyWorkflow {
    pub fn from_file(
        input_file: PathBuf,
        compression: CompressionFormat,
        target: WriteTarget,
    ) -> std::io::Result<Self> {
        let input_file_size = ByteSize::b(File::open(&input_file)?.metadata()?.len());
        Ok(Self {
            input_file,
            input_file_size,
            compression,
            target,
        })
    }

    pub fn make_child_config(&self) -> WriteVerifyAction {
        WriteVerifyAction {
            dest: self.target.devnode.clone(),
            src: self.input_file.clone(),
            verify: true,
            compression: self.compression,
            target_type: self.target.target_type,
            block_size: self.target.block_size.0.map(|s| s.as_u64()),
        }
    }
}

/// A state machine for tracking the state of the write + verify workflow.
#[derive(Debug, Clone, PartialEq)]
pub enum WVState {
    Writing(Writing),
    Verifying {
        write_hist: ByteSeries,
        verify_hist: ByteSeries,
        total_write_bytes: u64,
    },
    Finished {
        finish_time: Instant,
        result: Result<(), Arc<WriteVerifyWorkflowError>>,
        write_hist: ByteSeries,
        verify_hist: Option<ByteSeries>,
        total_write_bytes: u64,
    },
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum WriteVerifyWorkflowError {
    #[error("Unexpected first event: {0:?}")]
    Unexpected(WriteVerifyEvent),
    #[error("Daemon management error: {0}")]
    Daemon(#[from] Arc<DaemonError>),
    #[error("Worker error: {0}")]
    Worker(#[from] LegacyWriteVerifyError),
    #[error("Orchestrator panicked!")]
    Panicked,
}

impl PartialEq for WriteVerifyWorkflowError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Daemon(_), Self::Daemon(_)) => true,
            (Self::Worker(l0), Self::Worker(r0)) => l0 == r0,
            _ => false,
        }
    }
}

impl WorkflowState for WVState {
    type Error = Arc<WriteVerifyWorkflowError>;
    type Success = ();

    fn result(&self) -> Option<&Result<Self::Success, Self::Error>> {
        match self {
            WVState::Finished { result, .. } => Some(result),
            _ => None,
        }
    }
}

impl WVState {
    pub fn initial(now: Instant, is_input_compressed: bool, input_file_bytes: u64) -> Self {
        WVState::Writing(Writing::new(now, is_input_compressed, input_file_bytes))
    }

    pub fn error(now: Instant, error: WriteVerifyWorkflowError) -> Self {
        WVState::Finished {
            finish_time: now,
            result: Err(error.into()),
            write_hist: ByteSeries::new(now),
            verify_hist: None,
            total_write_bytes: 0,
        }
    }

    #[tracing::instrument(skip_all, fields(msg), level = "debug")]
    pub fn on_status(mut self, now: Instant, msg: Option<WriteVerifyEvent>) -> Self {
        match msg {
            Some(WriteVerifyEvent::TotalBytes { src, dest }) => {
                trace!("Received total bytes notification");
                self.on_total_bytes(now, src, dest);
                self
            }
            Some(WriteVerifyEvent::FinishedWriting { verifying }) => {
                info!("Received finished writing notification");
                match self {
                    WVState::Writing(st) => st.into_finished(now, verifying),
                    c => c,
                }
            }
            Some(WriteVerifyEvent::Error(reason)) => {
                info!("Received error notification");
                self.into_finished(now, Err(reason.into()))
            }
            Some(WriteVerifyEvent::Success) => {
                info!("Received success notification");
                self.into_finished(now, Ok(()))
            }
            None => {
                info!("Messages terminated unexpectedly");
                self.into_finished(
                    now,
                    Err(LegacyWriteVerifyError::UnexpectedTermination.into()),
                )
            }
            other => panic!(
                "Received unexpected child status {:#?}\nCurrent state: {:#?}",
                other, self
            ),
        }
    }

    pub fn write_hist(&self) -> &ByteSeries {
        match self {
            Self::Writing(Writing { write_hist, .. }) => write_hist,
            Self::Verifying { write_hist, .. } => write_hist,
            Self::Finished { write_hist, .. } => write_hist,
        }
    }

    pub fn verify_hist(&self) -> Option<&ByteSeries> {
        match self {
            Self::Writing { .. } => None,
            Self::Verifying { verify_hist, .. } => Some(verify_hist),
            Self::Finished { verify_hist, .. } => verify_hist.as_ref(),
        }
    }

    fn on_total_bytes(&mut self, now: Instant, src: u64, dest: u64) {
        match self {
            WVState::Writing(st) => {
                st.read_hist.push(now, src);
                st.write_hist.push(now, dest);
            }
            WVState::Verifying { verify_hist, .. } => verify_hist.push(now, dest),
            WVState::Finished { .. } => {}
        };
    }

    fn into_finished(self, now: Instant, error: Result<(), WriteVerifyWorkflowError>) -> WVState {
        match self {
            WVState::Writing(st) => {
                let total_write_bytes = st.write_hist.bytes_encountered();
                WVState::Finished {
                    finish_time: now,
                    result: error.map_err(Arc::new),
                    write_hist: st.write_hist,
                    verify_hist: None,
                    total_write_bytes,
                }
            }
            WVState::Verifying {
                write_hist,
                verify_hist,
                ..
            } => {
                let total_write_bytes = write_hist.bytes_encountered();
                WVState::Finished {
                    finish_time: now,
                    result: error.map_err(Arc::new),
                    write_hist,
                    verify_hist: Some(verify_hist),
                    total_write_bytes,
                }
            }
            fin => fin,
        }
    }

    pub fn is_finished(&self) -> bool {
        matches!(self, WVState::Finished { .. })
    }
}

impl Default for WVState {
    /// Suitable value to put into the cell when [`std::mem::take()`] is called.
    fn default() -> Self {
        let now = Instant::now();
        Self::Finished {
            finish_time: now,
            result: Err(Arc::new(WriteVerifyWorkflowError::Panicked)),
            write_hist: ByteSeries::new(now),
            verify_hist: None,
            total_write_bytes: 0,
        }
    }
}

/// Data tracked during active writing.
#[derive(Debug, Clone, PartialEq)]
pub struct Writing {
    pub write_hist: ByteSeries,
    pub total_raw_bytes: Option<u64>,
    pub read_hist: ByteSeries,
    pub input_file_bytes: u64,
}

impl Writing {
    pub fn new(start: Instant, is_input_compressed: bool, input_file_bytes: u64) -> Self {
        Self {
            write_hist: ByteSeries::new(start),
            total_raw_bytes: if is_input_compressed {
                None
            } else {
                Some(input_file_bytes)
            },
            read_hist: ByteSeries::new(start),
            input_file_bytes,
        }
    }

    pub fn approximate_ratio(&self) -> f64 {
        match self.total_raw_bytes {
            Some(total_bytes) => self.write_hist.bytes_encountered() as f64 / total_bytes as f64,
            None => self.read_hist.bytes_encountered() as f64 / self.input_file_bytes as f64,
        }
    }

    pub fn eta_write(&self) -> EstimatedTime {
        match self.total_raw_bytes {
            Some(total_bytes) => self.write_hist.estimated_time_left(total_bytes),
            None => self.read_hist.estimated_time_left(self.input_file_bytes),
        }
    }

    fn into_finished(self, time: Instant, verifying: bool) -> WVState {
        let total_write_bytes = self.write_hist.bytes_encountered();

        if verifying {
            info!(verifying, "Transition to verifying");

            WVState::Verifying {
                write_hist: self.write_hist,
                verify_hist: ByteSeries::new(time),
                total_write_bytes,
            }
        } else {
            info!(verifying, "Transition to finished");
            WVState::Finished {
                finish_time: time,
                result: Ok(()),
                write_hist: self.write_hist,
                verify_hist: None,
                total_write_bytes,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };

    use super::WVState;
    use crate::{byteseries::ByteSeries, herder_api::write_verify::*};

    #[test]
    fn accept_total_bytes_messages() {
        let t0 = Instant::now();
        let s = WVState::initial(t0, false, 80)
            .on_status(
                t0 + Duration::from_secs(1),
                Some(WriteVerifyEvent::TotalBytes { src: 20, dest: 10 }),
            )
            .on_status(
                t0 + Duration::from_secs(2),
                Some(WriteVerifyEvent::TotalBytes { src: 30, dest: 30 }),
            )
            .on_status(
                t0 + Duration::from_secs(3),
                Some(WriteVerifyEvent::TotalBytes { src: 60, dest: 50 }),
            );

        let s = match s {
            WVState::Writing(s) => s,
            s => panic!("unexpected {:#?}", s),
        };
        assert_eq!(s.read_hist.last_datapoint(), (3.0, 60));
        assert_eq!(s.write_hist.last_datapoint(), (3.0, 50));
    }

    #[test]
    fn writing_value_for_uncompressed_ratio() {
        let t0 = Instant::now();
        let s = WVState::initial(t0, false, 400).on_status(
            t0 + Duration::from_secs(1),
            Some(WriteVerifyEvent::TotalBytes { src: 15, dest: 40 }),
        );

        let s = match s {
            WVState::Writing(s) => s,
            s => panic!("unexpected {:#?}", s),
        };
        assert_eq!(s.approximate_ratio(), 0.1);
    }

    #[test]
    fn writing_value_for_compressed_ratio() {
        let t0 = Instant::now();
        let s = WVState::initial(t0, true, 80).on_status(
            t0 + Duration::from_secs(1),
            Some(WriteVerifyEvent::TotalBytes {
                src: 20,
                dest: 100000, // very big number to make errors obvious
            }),
        );

        let s = match s {
            WVState::Writing(s) => s,
            s => panic!("unexpected {s:#?}"),
        };
        assert_eq!(s.approximate_ratio(), 0.25);
    }

    #[test]
    fn sudden_terminate_in_writing_state_sets_error() {
        let t0 = Instant::now();
        let s = WVState::initial(t0, true, 80)
            .on_status(
                t0 + Duration::from_secs(1),
                Some(WriteVerifyEvent::TotalBytes { src: 20, dest: 20 }),
            )
            .on_status(t0 + Duration::from_secs(2), None);

        match s {
            WVState::Finished {
                finish_time,
                result: error,
                ..
            } => {
                assert_eq!(finish_time - t0, Duration::from_secs(2));
                assert_eq!(
                    error,
                    Err(Arc::new(
                        LegacyWriteVerifyError::UnexpectedTermination.into()
                    ))
                );
            }
            s => panic!("Unexpected {s:#?}"),
        }
    }

    #[test]
    fn terminate_during_finished_is_idempotent() {
        let t0 = Instant::now();
        let finish_time = t0 + Duration::from_secs(10);
        let s0 = WVState::Finished {
            finish_time,
            result: Ok(()),
            write_hist: ByteSeries::new(t0),
            verify_hist: None,
            total_write_bytes: 12345678,
        };
        let s1 = s0
            .clone()
            .on_status(finish_time + Duration::from_secs(2), None);

        assert_eq!(s1, s0);
    }

    #[test]
    fn finished_during_finished_is_idempotent() {
        let t0 = Instant::now();
        let finish_time = t0 + Duration::from_secs(10);
        let s0 = WVState::Finished {
            finish_time,
            result: Ok(()),
            write_hist: ByteSeries::new(t0),
            verify_hist: None,
            total_write_bytes: 12345678,
        };
        let s1 = s0.clone().on_status(
            finish_time + Duration::from_secs(2),
            Some(WriteVerifyEvent::FinishedWriting { verifying: false }),
        );

        assert_eq!(s1, s0);
    }
}
