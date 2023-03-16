use std::time::Instant;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use tracing::{info, trace};

use crate::burn::{ipc::StatusMessage, state_tracking::ChildState};

use super::history::UIState;

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
    pub ui_state: UIState,
}

impl State {
    pub fn on_event(self, ev: UIEvent) -> anyhow::Result<Self> {
        trace!("Handling {ev:?}");

        Ok(match ev {
            UIEvent::SleepTimeout => self,
            UIEvent::RecvChildStatus(t, m) => Self {
                child: self.child.on_status(t, m),
                ..self
            },
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
}

#[derive(Debug, thiserror::Error)]
#[error("User sent quit signal")]
pub struct Quit;
