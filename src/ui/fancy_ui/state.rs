use std::time::Instant;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use tracing::info;

use crate::{
    ui::{start::BeginParams, writer_tracking::WriterState},
    writer_process::ipc::StatusMessage,
};

use super::widgets::{QuitModal, QuitModalResult, SpeedChartState};

#[derive(Debug, PartialEq, Clone)]
pub enum UIEvent {
    SleepTimeout,
    RecvChildStatus(Instant, Option<StatusMessage>),
    RecvTermEvent(Event),
}

#[derive(Debug, Clone)]
pub struct State {
    pub input_filename: String,
    pub target_filename: String,
    pub child: WriterState,
    pub graph_state: SpeedChartState,
    pub quit_modal: Option<QuitModal>,
}

impl State {
    pub fn initial(now: Instant, params: &BeginParams, input_file_bytes: u64) -> Self {
        State {
            input_filename: params.input_file.to_string_lossy().to_string(),
            target_filename: params.target.devnode.to_string_lossy().to_string(),
            child: WriterState::initial(now, !params.compression.is_identity(), input_file_bytes),
            graph_state: SpeedChartState::default(),
            quit_modal: None,
        }
    }

    #[tracing::instrument(skip_all, level = "debug", fields(ev))]
    pub fn on_event(self, ev: UIEvent) -> anyhow::Result<Self> {
        Ok(match ev {
            UIEvent::SleepTimeout => self,
            UIEvent::RecvChildStatus(t, m) => Self {
                child: self.child.on_status(t, m),
                ..self
            },
            UIEvent::RecvTermEvent(e) => self.on_term_event(e)?,
        })
    }

    #[tracing::instrument(skip_all, level = "debug", fields(ev))]
    fn on_term_event(self, ev: Event) -> anyhow::Result<Self> {
        match ev {
            Event::Key(KeyEvent {
                kind: KeyEventKind::Press,
                code,
                modifiers,
                ..
            }) => self.handle_key_down((code, modifiers)),
            _ => Ok(self),
        }
    }

    fn handle_key_down(mut self, (kc, km): (KeyCode, KeyModifiers)) -> anyhow::Result<Self> {
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
                info!("Got request to quit, spawning prompt");
                self.quit_modal = Some(QuitModal::new());
                Ok(self)
            }
            _ => Ok(self),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("User sent quit signal")]
pub struct Quit;
