use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use tracing::info;

use crate::orchestrator::{WriteVerifyParams, WriterState};

use super::widgets::{QuitModal, QuitModalResult, SpeedChartState};

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
    pub fn initial(params: &WriteVerifyParams) -> Self {
        State {
            input_filename: params.input_file.to_string_lossy().to_string(),
            target_filename: params.target.devnode.to_string_lossy().to_string(),
            graph_state: SpeedChartState::default(),
            quit_modal: None,
        }
    }

    #[tracing::instrument(skip_all, level = "debug", fields(ev))]
    pub fn on_event(self, child: &WriterState, ev: UIEvent) -> anyhow::Result<Self> {
        Ok(match ev {
            UIEvent::SleepTimeout => self,
            UIEvent::RecvTermEvent(e) => self.on_term_event(child, e)?,
        })
    }

    #[tracing::instrument(skip_all, level = "debug", fields(ev))]
    fn on_term_event(
        self,
        child: &WriterState,
        ev: Result<Event, (String, std::io::ErrorKind)>,
    ) -> anyhow::Result<Self> {
        match ev {
            Ok(Event::Key(KeyEvent {
                kind: KeyEventKind::Press,
                code,
                modifiers,
                ..
            })) => self.handle_key_down(child, code, modifiers),
            Err((msg, kind)) => {
                tracing::error!("Error getting term event ({kind}): {msg}");
                Err(Quit)?
            }
            _ => Ok(self),
        }
    }

    fn handle_key_down(
        mut self,
        child: &WriterState,
        kc: KeyCode,
        km: KeyModifiers,
    ) -> anyhow::Result<Self> {
        if let Some(qm) = &self.quit_modal {
            return match qm.handle_key_down(kc) {
                Some(QuitModalResult::Quit) => Err(Quit.into()),
                Some(QuitModalResult::Stay) => Ok(Self {
                    quit_modal: None,
                    ..self
                }),
                None => Ok(self),
            };
        }

        match (kc, km) {
            (KeyCode::Char('c'), KeyModifiers::CONTROL)
            | (KeyCode::Esc, _)
            | (KeyCode::Char('q'), _) => {
                if child.is_finished() {
                    info!("Writing and verification finished; quitting immediately");
                    Err(Quit.into())
                } else {
                    info!("Got request to quit, spawning prompt");
                    self.quit_modal = Some(QuitModal::new());
                    Ok(self)
                }
            }
            _ => Ok(self),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("User sent quit signal")]
pub struct Quit;
