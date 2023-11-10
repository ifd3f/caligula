use std::time::Instant;

use tracing::{info, trace};

use crate::byteseries::{ByteSeries, EstimatedTime};

use super::ipc::{ErrorType, StatusMessage};

#[derive(Debug, Clone, PartialEq)]
pub enum WriterState {
    Writing(Writing),
    Verifying {
        write_hist: ByteSeries,
        verify_hist: ByteSeries,
        total_write_bytes: u64,
    },
    Finished {
        finish_time: Instant,
        error: Option<ErrorType>,
        write_hist: ByteSeries,
        verify_hist: Option<ByteSeries>,
        total_write_bytes: u64,
    },
}

impl WriterState {
    #[tracing::instrument]
    pub fn initial(now: Instant, is_input_compressed: bool, input_file_bytes: u64) -> Self {
        WriterState::Writing(Writing::new(now, is_input_compressed, input_file_bytes))
    }

    #[tracing::instrument(skip_all, fields(msg), level = "debug")]
    pub fn on_status(mut self, now: Instant, msg: Option<StatusMessage>) -> Self {
        match msg {
            Some(StatusMessage::TotalBytes { src, dest }) => {
                trace!("Received total bytes notification");
                self.on_total_bytes(now, src, dest);
                self
            }
            Some(StatusMessage::FinishedWriting { verifying }) => {
                info!("Received finished writing notification");
                match self {
                    WriterState::Writing(st) => st.into_finished(now, verifying),
                    c => c,
                }
            }
            Some(StatusMessage::Error(reason)) => {
                info!("Received error notification");
                self.into_finished(now, Some(reason))
            }
            Some(StatusMessage::Success) => {
                info!("Received success notification");
                self.into_finished(now, None)
            }
            None => {
                info!("Messages terminated unexpectedly");
                self.into_finished(now, Some(ErrorType::UnexpectedTermination))
            }
            other => panic!(
                "Recieved nexpected child status {:#?}\nCurrent state: {:#?}",
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
            WriterState::Writing(st) => {
                st.read_hist.push(now, src);
                st.write_hist.push(now, dest);
            }
            WriterState::Verifying { verify_hist, .. } => verify_hist.push(now, dest),
            WriterState::Finished { .. } => {}
        };
    }

    fn into_finished(self, now: Instant, error: Option<ErrorType>) -> WriterState {
        match self {
            WriterState::Writing(st) => {
                let total_write_bytes = st.write_hist.bytes_encountered();
                WriterState::Finished {
                    finish_time: now,
                    error,
                    write_hist: st.write_hist,
                    verify_hist: None,
                    total_write_bytes,
                }
            }
            WriterState::Verifying {
                write_hist,
                verify_hist,
                ..
            } => {
                let total_write_bytes = write_hist.bytes_encountered();
                WriterState::Finished {
                    finish_time: now,
                    error,
                    write_hist,
                    verify_hist: Some(verify_hist),
                    total_write_bytes,
                }
            }
            fin => fin,
        }
    }

    pub fn is_finished(&self) -> bool {
        match self {
            WriterState::Finished { .. } => true,
            _ => false,
        }
    }
}

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

    fn into_finished(self, time: Instant, verifying: bool) -> WriterState {
        let total_write_bytes = self.write_hist.bytes_encountered();

        if verifying {
            info!(verifying, "Transition to verifying");

            WriterState::Verifying {
                write_hist: self.write_hist,
                verify_hist: ByteSeries::new(time),
                total_write_bytes,
            }
        } else {
            info!(verifying, "Transition to finished");
            WriterState::Finished {
                finish_time: time,
                error: None,
                write_hist: self.write_hist,
                verify_hist: None,
                total_write_bytes,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use crate::{
        byteseries::ByteSeries,
        writer_process::ipc::{ErrorType, StatusMessage},
    };

    use super::WriterState;

    #[test]
    fn accept_total_bytes_messages() {
        let t0 = Instant::now();
        let s = WriterState::initial(t0, false, 80)
            .on_status(
                t0 + Duration::from_secs(1),
                Some(StatusMessage::TotalBytes { src: 20, dest: 10 }),
            )
            .on_status(
                t0 + Duration::from_secs(2),
                Some(StatusMessage::TotalBytes { src: 30, dest: 30 }),
            )
            .on_status(
                t0 + Duration::from_secs(3),
                Some(StatusMessage::TotalBytes { src: 60, dest: 50 }),
            );

        let s = match s {
            WriterState::Writing(s) => s,
            s => panic!("unexpected {:#?}", s),
        };
        assert_eq!(s.read_hist.last_datapoint(), (3.0, 60));
        assert_eq!(s.write_hist.last_datapoint(), (3.0, 50));
    }

    #[test]
    fn writing_value_for_uncompressed_ratio() {
        let t0 = Instant::now();
        let s = WriterState::initial(t0, false, 400).on_status(
            t0 + Duration::from_secs(1),
            Some(StatusMessage::TotalBytes { src: 15, dest: 40 }),
        );

        let s = match s {
            WriterState::Writing(s) => s,
            s => panic!("unexpected {:#?}", s),
        };
        assert_eq!(s.approximate_ratio(), 0.1);
    }

    #[test]
    fn writing_value_for_compressed_ratio() {
        let t0 = Instant::now();
        let s = WriterState::initial(t0, true, 80).on_status(
            t0 + Duration::from_secs(1),
            Some(StatusMessage::TotalBytes {
                src: 20,
                dest: 100000, // very big number to make errors obvious
            }),
        );

        let s = match s {
            WriterState::Writing(s) => s,
            s => panic!("unexpected {s:#?}"),
        };
        assert_eq!(s.approximate_ratio(), 0.25);
    }

    #[test]
    fn sudden_terminate_in_writing_state_sets_error() {
        let t0 = Instant::now();
        let s = WriterState::initial(t0, true, 80)
            .on_status(
                t0 + Duration::from_secs(1),
                Some(StatusMessage::TotalBytes { src: 20, dest: 20 }),
            )
            .on_status(t0 + Duration::from_secs(2), None);

        match s {
            WriterState::Finished {
                finish_time, error, ..
            } => {
                assert_eq!(finish_time - t0, Duration::from_secs(2));
                assert_eq!(error, Some(ErrorType::UnexpectedTermination));
            }
            s => panic!("Unexpected {s:#?}"),
        }
    }

    #[test]
    fn terminate_during_finished_is_idempotent() {
        let t0 = Instant::now();
        let finish_time = t0 + Duration::from_secs(10);
        let s0 = WriterState::Finished {
            finish_time,
            error: None,
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
        let s0 = WriterState::Finished {
            finish_time,
            error: None,
            write_hist: ByteSeries::new(t0),
            verify_hist: None,
            total_write_bytes: 12345678,
        };
        let s1 = s0.clone().on_status(
            finish_time + Duration::from_secs(2),
            Some(StatusMessage::FinishedWriting { verifying: false }),
        );

        assert_eq!(s1, s0);
    }
}
