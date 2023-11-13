use std::time::Instant;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use tracing::info;

use crate::{
    device::WriteTarget,
    ui::burn::start::InputFileParams,
    writer_process::{ipc::StatusMessage, state_tracking::WriterState},
};

use super::widgets::SpeedChartState;

#[derive(Debug, PartialEq, Clone)]
pub enum UIEvent {
    SleepTimeout,
    RecvChildStatus(Instant, Option<StatusMessage>),
    RecvTermEvent(Event),
}

#[derive(Debug, Clone)]
pub struct State {
    pub input_filename: String,
    pub writer: Option<ActiveWriter>,
}

impl State {
    pub fn initial(now: Instant, params: &InputFileParams) -> Self {
        State {
            input_filename: params.file.to_string_lossy().to_string(),
            writer: None,
        }
    }

    #[tracing::instrument(skip_all, level = "debug", fields(ev))]
    pub fn on_event(self, ev: UIEvent) -> anyhow::Result<Self> {
        Ok(match ev {
            UIEvent::SleepTimeout => self,
            UIEvent::RecvChildStatus(t, m) => todo!(),
            UIEvent::RecvTermEvent(e) => self.on_term_event(e)?,
        })
    }

    #[tracing::instrument(skip_all, level = "debug", fields(ev))]
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
}

#[derive(Debug, Clone)]
pub struct ActiveWriter {
    pub target_filename: String,
    pub child: WriterState,
    pub graph_state: SpeedChartState,
}

impl ActiveWriter {
    pub fn new(now: Instant, params: &InputFileParams, target: WriteTarget) -> Self {
        Self {
            target_filename: target.devnode.to_string_lossy().to_string(),
            child: WriterState::initial(
                Instant::now(),
                !params.compression.is_identity(),
                params.size.as_u64(),
            ),
            graph_state: SpeedChartState::default(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("User sent quit signal")]
pub struct Quit;
