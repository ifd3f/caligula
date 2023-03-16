use std::time::Instant;

use tracing::{debug, info};

use crate::{byteseries::ByteSeries, ui::burn::start::BeginParams};

use super::ipc::{ErrorType, StatusMessage};

#[derive(Debug, Clone)]
pub enum ChildState {
    Burning {
        write_hist: ByteSeries,
        read_hist: ByteSeries,
        max_bytes: Option<u64>,
        input_file_bytes: u64,
    },
    Verifying {
        write_hist: ByteSeries,
        verify_hist: ByteSeries,
        max_bytes: u64,
    },
    Finished {
        finish_time: Instant,
        error: Option<ErrorType>,
        write_hist: ByteSeries,
        verify_hist: Option<ByteSeries>,
        max_bytes: u64,
    },
}

impl ChildState {
    pub fn initial(params: &BeginParams, input_file_bytes: u64) -> Self {
        ChildState::Burning {
            write_hist: ByteSeries::new(Instant::now()),
            read_hist: ByteSeries::new(Instant::now()),
            max_bytes: if params.compression.is_identity() {
                Some(input_file_bytes)
            } else {
                None
            },
            input_file_bytes,
        }
    }

    pub fn on_status(mut self, now: Instant, msg: Option<StatusMessage>) -> Self {
        match msg {
            Some(StatusMessage::TotalBytes { src, dest }) => {
                self.on_total_bytes(now, src, dest);
                self
            }
            Some(StatusMessage::FinishedWriting { verifying }) => {
                debug!(verifying, "Got FinishedWriting");
                match self {
                    ChildState::Burning { write_hist, .. } => {
                        let max_bytes = write_hist.bytes_encountered();

                        if verifying {
                            info!(verifying, "Transition to verifying");

                            ChildState::Verifying {
                                write_hist,
                                verify_hist: ByteSeries::new(now),
                                max_bytes,
                            }
                        } else {
                            info!(verifying, "Transition to finished");
                            ChildState::Finished {
                                finish_time: now,
                                error: None,
                                write_hist,
                                verify_hist: None,
                                max_bytes,
                            }
                        }
                    }
                    c => c,
                }
            }
            Some(StatusMessage::Error(reason)) => self.into_finished(now, Some(reason)),
            Some(StatusMessage::Success) => self.into_finished(now, None),
            None => self.into_finished(now, Some(ErrorType::UnexpectedTermination)),
            other => panic!(
                "Recieved nexpected child status {:#?}\nCurrent state: {:#?}",
                other, self
            ),
        }
    }

    fn on_total_bytes(&mut self, now: Instant, src: u64, dest: u64) {
        match self {
            ChildState::Burning {
                write_hist,
                read_hist,
                ..
            } => {
                read_hist.push(now, src);
                write_hist.push(now, dest);
            }
            ChildState::Verifying { verify_hist, .. } => verify_hist.push(now, dest),
            ChildState::Finished { .. } => {}
        };
    }

    fn into_finished(self, now: Instant, error: Option<ErrorType>) -> ChildState {
        match self {
            ChildState::Burning { write_hist, .. } => {
                let max_bytes = write_hist.bytes_encountered();
                ChildState::Finished {
                    finish_time: now,
                    error,
                    write_hist,
                    verify_hist: None,
                    max_bytes,
                }
            }
            ChildState::Verifying {
                write_hist,
                verify_hist,
                ..
            } => {
                let max_bytes = write_hist.bytes_encountered();
                ChildState::Finished {
                    finish_time: now,
                    error,
                    write_hist,
                    verify_hist: Some(verify_hist),
                    max_bytes,
                }
            }
            fin => fin,
        }
    }

    pub fn is_finished(&self) -> bool {
        match self {
            ChildState::Finished { .. } => true,
            _ => false,
        }
    }
}
