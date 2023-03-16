use std::time::Instant;

use tracing::{debug, info};

use crate::{
    byteseries::{ByteSeries, EstimatedTime},
    compression::CompressionFormat,
    ui::burn::start::BeginParams,
};

use super::ipc::{ErrorType, StatusMessage};

#[derive(Debug, Clone)]
pub enum ChildState {
    Burning(Burning),
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

impl ChildState {
    pub fn initial(now: Instant, params: &BeginParams, input_file_bytes: u64) -> Self {
        ChildState::Burning(Burning::new(now, params.compression, input_file_bytes))
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
                    ChildState::Burning(st) => st.into_finished(now, verifying),
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

    pub fn write_hist(&self) -> &ByteSeries {
        match self {
            Self::Burning(Burning { write_hist, .. }) => write_hist,
            Self::Verifying { write_hist, .. } => write_hist,
            Self::Finished { write_hist, .. } => write_hist,
        }
    }

    pub fn verify_hist(&self) -> Option<&ByteSeries> {
        match self {
            Self::Burning { .. } => None,
            Self::Verifying { verify_hist, .. } => Some(verify_hist),
            Self::Finished { verify_hist, .. } => verify_hist.as_ref(),
        }
    }

    fn on_total_bytes(&mut self, now: Instant, src: u64, dest: u64) {
        match self {
            ChildState::Burning(st) => {
                st.read_hist.push(now, src);
                st.write_hist.push(now, dest);
            }
            ChildState::Verifying { verify_hist, .. } => verify_hist.push(now, dest),
            ChildState::Finished { .. } => {}
        };
    }

    fn into_finished(self, now: Instant, error: Option<ErrorType>) -> ChildState {
        match self {
            ChildState::Burning(st) => {
                let total_write_bytes = st.write_hist.bytes_encountered();
                ChildState::Finished {
                    finish_time: now,
                    error,
                    write_hist: st.write_hist,
                    verify_hist: None,
                    total_write_bytes,
                }
            }
            ChildState::Verifying {
                write_hist,
                verify_hist,
                ..
            } => {
                let total_write_bytes = write_hist.bytes_encountered();
                ChildState::Finished {
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
            ChildState::Finished { .. } => true,
            _ => false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Burning {
    pub write_hist: ByteSeries,
    pub total_raw_bytes: Option<u64>,
    pub read_hist: ByteSeries,
    pub input_file_bytes: u64,
}

impl Burning {
    pub fn new(start: Instant, compression: CompressionFormat, input_file_bytes: u64) -> Self {
        Self {
            write_hist: ByteSeries::new(start),
            total_raw_bytes: if compression.is_identity() {
                Some(input_file_bytes)
            } else {
                None
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

    fn into_finished(self, time: Instant, verifying: bool) -> ChildState {
        let total_write_bytes = self.write_hist.bytes_encountered();

        if verifying {
            info!(verifying, "Transition to verifying");

            ChildState::Verifying {
                write_hist: self.write_hist,
                verify_hist: ByteSeries::new(time),
                total_write_bytes,
            }
        } else {
            info!(verifying, "Transition to finished");
            ChildState::Finished {
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
    use std::time::Instant;

    use bytesize::ByteSize;

    use crate::{
        compression::CompressionFormat,
        device::{self, BurnTarget, Removable},
        ui::burn::start::BeginParams,
    };

    use super::ChildState;

    fn example_disk(input_bytes: u64, compression: CompressionFormat) -> BeginParams {
        BeginParams {
            input_file: "test".into(),
            input_file_size: ByteSize::b(input_bytes),
            compression,
            target: BurnTarget {
                name: "sda1".into(),
                devnode: "/dev/sda1".into(),
                size: Some(ByteSize::b(100)).into(),
                model: Some("foobar".to_string()).into(),
                removable: Removable::Yes,
                target_type: device::Type::Partition,
            },
        }
    }

    #[test]
    fn init_without_compression() {
        let s = ChildState::initial(
            Instant::now(),
            &example_disk(100, CompressionFormat::Identity),
            10,
        );
    }
}
