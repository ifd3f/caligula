use futures::{Stream, StreamExt as _, stream};
use ratatui::{Terminal, prelude::Backend};

use self::state::UIEvent;
use crate::{
    logging::LogPaths,
    orchestrator::{WriteVerifyParams, WriterState, watch::Watch},
    ui::fancy_ui::{display::FancyUI, state::State},
};
use std::time::Duration;

mod display;
mod state;
mod widgets;

/// How often we refresh the display
const REFRESH_PERIOD: Duration = Duration::from_millis(250);

pub struct Params<'a, B, T>
where
    B: Backend + 'a,
    T: Stream<Item = std::io::Result<crossterm::event::Event>> + 'a,
{
    pub terminal: &'a mut Terminal<B>,
    pub begin: &'a WriteVerifyParams,
    pub child_state: Watch<WriterState>,
    pub terminal_events: T,
    pub log_paths: &'a LogPaths,
}

/// Run the fancy TUI.
#[tracing::instrument(skip_all)]
pub async fn run<'a, B, T>(params: Params<'a, B, T>) -> anyhow::Result<()>
where
    B: Backend,
    T: Stream<Item = std::io::Result<crossterm::event::Event>> + 'a,
{
    let terminal_events =
        params
            .terminal_events
            .map(|e: std::io::Result<crossterm::event::Event>| {
                UIEvent::RecvTermEvent(e.map_err(|e| (e.to_string(), e.kind())))
            });
    let timeout_events =
        stream::unfold(tokio::time::interval(REFRESH_PERIOD), |mut i| async move {
            i.tick().await;
            Some((UIEvent::SleepTimeout, i))
        });
    let events = Box::pin(stream::select(terminal_events, timeout_events));

    let ui = FancyUI {
        terminal: params.terminal,
        events,
        child_state: params.child_state,
        state: State::initial(params.begin),
        log_paths: params.log_paths,
    };

    ui.show().await
}
