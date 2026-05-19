//! UI state along with its reactor.

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use tracing::info;

use super::widgets::{QuitModal, QuitModalResult, SpeedChartState};
use crate::facade::{WVState, WriteVerifyWorkflow};

#[derive(Debug, PartialEq, Clone)]
pub enum UIEvent {
    SleepTimeout,
    RecvTermEvent(Result<Event, (String, std::io::ErrorKind)>),
}

#[derive(Debug, Clone)]
pub struct State {
    pub input_filename: String,
    pub target_filename: String,
    pub graph_state: SpeedChartState,
    pub quit_modal: Option<QuitModal>,
}

impl State {
    pub fn initial(params: &WriteVerifyWorkflow) -> Self {
        State {
            input_filename: params.input_file.to_string_lossy().to_string(),
            target_filename: params.target.devnode.to_string_lossy().to_string(),
            graph_state: SpeedChartState::default(),
            quit_modal: None,
        }
    }

    /// Handle the UI event, given the current state of the writer process.
    ///
    /// Returns new version of [`Self`], or [`None`] to signal completion.
    #[tracing::instrument(level = "trace", skip_all, fields(ev))]
    pub fn on_event(self, child: &WVState, ev: UIEvent) -> Option<Self> {
        match ev {
            UIEvent::SleepTimeout => Some(self),
            UIEvent::RecvTermEvent(e) => self.on_term_event(child, e),
        }
    }

    #[tracing::instrument(level = "debug", skip_all, fields(ev))]
    fn on_term_event(
        self,
        child: &WVState,
        ev: Result<Event, (String, std::io::ErrorKind)>,
    ) -> Option<Self> {
        match ev {
            Ok(Event::Key(KeyEvent {
                kind: KeyEventKind::Press,
                code,
                modifiers,
                ..
            })) => self.handle_key_down(child, code, modifiers),
            Err((msg, kind)) => {
                tracing::error!("Error getting term event ({kind}): {msg}");
                None
            }
            _ => Some(self),
        }
    }

    fn handle_key_down(mut self, child: &WVState, kc: KeyCode, km: KeyModifiers) -> Option<Self> {
        if let Some(qm) = &self.quit_modal {
            return match qm.handle_key_down(kc) {
                Some(QuitModalResult::Quit) => None,
                Some(QuitModalResult::Stay) => Some(Self {
                    quit_modal: None,
                    ..self
                }),
                None => Some(self),
            };
        }

        match (kc, km) {
            (KeyCode::Char('c'), KeyModifiers::CONTROL)
            | (KeyCode::Esc, _)
            | (KeyCode::Char('q'), _) => {
                if child.is_finished() {
                    info!("Writing and verification finished; quitting immediately");
                    None
                } else {
                    info!("Got request to quit, spawning prompt");
                    self.quit_modal = Some(QuitModal::new());
                    Some(self)
                }
            }
            _ => Some(self),
        }
    }
}
