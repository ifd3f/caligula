use std::time::Instant;

use bytesize::ByteSize;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use tracing::{debug, info, trace};

use crate::{
    burn::{
        ipc::{ErrorType, StatusMessage},
        Handle,
    },
    ui::burn::byteseries::ByteSeries,
};

#[derive(Debug, PartialEq, Clone)]
pub enum UIEvent {
    SleepTimeout,
    RecvChildStatus(Instant, Option<StatusMessage>),
    RecvTermEvent(Event),
}

#[derive(Debug)]
pub struct State {
    pub input_filename: String,
    pub target_filename: String,
    pub child: ChildState,
}

#[derive(Debug)]
pub enum ChildState {
    Burning {
        handle: Handle,
        write_hist: ByteSeries,
    },
    Verifying {
        handle: Handle,
        write_hist: ByteSeries,
        verify_hist: ByteSeries,
    },
    Finished {
        finish_time: Instant,
        error: Option<ErrorType>,
        write_hist: ByteSeries,
        verify_hist: Option<ByteSeries>,
    },
}

impl State {
    pub fn on_event(self, ev: UIEvent) -> anyhow::Result<Self> {
        trace!("Handling {ev:?}");

        Ok(match ev {
            UIEvent::SleepTimeout => self,
            UIEvent::RecvChildStatus(t, m) => self.on_child_status(t, m),
            UIEvent::RecvTermEvent(e) => self.on_term_event(e)?,
        })
    }

    fn on_term_event(self, ev: Event) -> anyhow::Result<Self> {
        match ev {
            Event::Key(KeyEvent {
                code: KeyCode::Char('c'),
                modifiers: KeyModifiers::CONTROL,
                kind: KeyEventKind::Press,
                ..
            }) => {
                info!("Got CTRL-C, quitting");
                Err(Quit)?
            }
            _ => Ok(self),
        }
    }

    fn on_child_status(mut self, now: Instant, msg: Option<StatusMessage>) -> Self {
        match msg {
            Some(StatusMessage::TotalBytes { src, dest }) => {
                self.child.on_total_bytes(now, src, dest);
                self
            }
            Some(StatusMessage::FinishedWriting { verifying }) => {
                debug!(verifying, "Got FinishedWriting");
                let child = match self.child {
                    ChildState::Burning {
                        handle,
                        mut write_hist,
                    } => {
                        write_hist.finished_verifying_at(now);
                        if verifying {
                            info!(verifying, "Transition to verifying");

                            let max_bytes = ByteSize::b(handle.initial_info().input_file_bytes);
                            ChildState::Verifying {
                                handle,
                                write_hist,
                                verify_hist: ByteSeries::new(now, max_bytes),
                            }
                        } else {
                            info!(verifying, "Transition to finished");
                            ChildState::Finished {
                                finish_time: now,
                                error: None,
                                write_hist,
                                verify_hist: None,
                            }
                        }
                    }
                    c => c,
                };
                Self { child, ..self }
            }
            Some(StatusMessage::Error(reason)) => Self {
                child: self.child.into_finished(now, Some(reason)),
                ..self
            },
            Some(StatusMessage::Success) => Self {
                child: self.child.into_finished(now, None),
                ..self
            },
            None => Self {
                child: self
                    .child
                    .into_finished(now, Some(ErrorType::UnexpectedTermination)),
                ..self
            },
            other => panic!(
                "Recieved nexpected child status {:#?}\nCurrent state: {:#?}",
                other, self
            ),
        }
    }
}

impl ChildState {
    pub fn on_total_bytes(&mut self, now: Instant, src: u64, dest: u64) {
        match self {
            ChildState::Burning { write_hist, .. } => write_hist.push(now, src),
            ChildState::Verifying { verify_hist, .. } => verify_hist.push(now, dest),
            ChildState::Finished { .. } => {}
        };
    }

    pub fn child_process(&mut self) -> Option<&mut Handle> {
        match self {
            Self::Burning { handle, .. } => Some(handle),
            Self::Verifying { handle, .. } => Some(handle),
            Self::Finished { .. } => None,
        }
    }

    fn into_finished(self, now: Instant, error: Option<ErrorType>) -> ChildState {
        match self {
            ChildState::Burning { write_hist, .. } => ChildState::Finished {
                finish_time: now,
                error,
                write_hist,
                verify_hist: None,
            },
            ChildState::Verifying {
                write_hist,
                verify_hist,
                ..
            } => ChildState::Finished {
                finish_time: now,
                error,
                write_hist,
                verify_hist: Some(verify_hist),
            },
            fin => fin,
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("User sent quit signal")]
pub struct Quit;
