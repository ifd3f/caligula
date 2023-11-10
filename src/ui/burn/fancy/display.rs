use std::time::Instant;

use crossterm::event::EventStream;
use futures::StreamExt;
use ratatui::{
    backend::Backend,
    layout::{Constraint, Direction, Layout, Rect},
    widgets::{Block, Borders, Paragraph, Wrap},
    Terminal,
};
use tokio::{select, time};

use crate::{
    writer_process::{self, state_tracking::WriterState, Handle},
    logging::get_bug_report_msg,
    ui::burn::{fancy::state::UIEvent, start::BeginParams},
};

use super::{
    state::{Quit, State},
    widgets::{make_info_table, make_progress_bar},
};

pub struct FancyUI<'a, B>
where
    B: Backend,
{
    terminal: &'a mut Terminal<B>,
    events: EventStream,
    handle: Option<writer_process::Handle>,
    state: State,
}

impl<'a, B> FancyUI<'a, B>
where
    B: Backend,
{
    #[tracing::instrument(skip_all)]
    pub fn new(params: &BeginParams, handle: writer_process::Handle, terminal: &'a mut Terminal<B>) -> Self {
        let input_file_bytes = handle.initial_info().input_file_bytes;
        Self {
            terminal,
            handle: Some(handle),
            events: EventStream::new(),
            state: State::initial(Instant::now(), &params, input_file_bytes),
        }
    }

    #[tracing::instrument(skip_all, level = "debug")]
    pub async fn show(mut self) -> anyhow::Result<()> {
        loop {
            match self.get_and_handle_events().await {
                Ok(s) => self = s,
                Err(e) => match e.downcast::<Quit>()? {
                    Quit => break,
                },
            }
        }
        Ok(())
    }

    #[tracing::instrument(skip_all, level = "trace")]
    async fn get_and_handle_events(mut self) -> anyhow::Result<FancyUI<'a, B>> {
        let msg = {
            if let Some(handle) = &mut self.handle {
                get_event_child_active(&mut self.events, handle).await
            } else {
                get_event_child_dead(&mut self.events).await
            }?
        };
        self.state = self.state.on_event(msg)?;

        // Drop handle/process if process died
        if self.state.child.is_finished() {
            self.handle = None;
        }

        draw(&mut self.state, &mut self.terminal)?;
        Ok(self)
    }
}

async fn get_event_child_dead(ui_events: &mut EventStream) -> anyhow::Result<UIEvent> {
    Ok(UIEvent::RecvTermEvent(ui_events.next().await.unwrap()?))
}

#[tracing::instrument(skip_all, level = "trace")]
async fn get_event_child_active(
    ui_events: &mut EventStream,
    child_events: &mut Handle,
) -> anyhow::Result<UIEvent> {
    let sleep = tokio::time::sleep(time::Duration::from_millis(250));
    select! {
        _ = sleep => {
            return Ok(UIEvent::SleepTimeout);
        }
        msg = child_events.next_message() => {
            return Ok(UIEvent::RecvChildStatus(Instant::now(), msg?));
        }
        event = ui_events.next() => {
            return Ok(UIEvent::RecvTermEvent(event.unwrap()?));
        }
    }
}

struct ComputedLayout {
    progress: Rect,
    graph: Rect,
    args_display: Rect,
}

impl From<Rect> for ComputedLayout {
    fn from(value: Rect) -> Self {
        let root = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(10),
                Constraint::Length(10),
            ])
            .split(value);

        let info_pane = root[2];

        Self {
            graph: root[1],
            progress: root[0],
            args_display: info_pane,
        }
    }
}

pub fn draw(
    state: &mut State,
    terminal: &mut Terminal<impl ratatui::backend::Backend>,
) -> anyhow::Result<()> {
    let progress_bar = make_progress_bar(&state.child);

    let final_time = match state.child {
        WriterState::Finished { finish_time, .. } => finish_time,
        _ => Instant::now(),
    };

    let error = match &state.child {
        WriterState::Finished { error, .. } => error.as_ref(),
        _ => None,
    };

    let info_table = make_info_table(&state.input_filename, &state.target_filename, &state.child);

    terminal.draw(|f| {
        let layout = ComputedLayout::from(f.size());

        f.render_widget(progress_bar.render(), layout.progress);

        state
            .ui_state
            .draw_speed_chart(&state.child, f, layout.graph, final_time);

        if let Some(error) = error {
            f.render_widget(
                Paragraph::new(format!("{error}\n{}", get_bug_report_msg()))
                    .block(
                        Block::default()
                            .title("!!! ERROR !!!")
                            .borders(Borders::ALL),
                    )
                    .wrap(Wrap { trim: true }),
                layout.args_display,
            )
        } else {
            f.render_widget(info_table, layout.args_display);
        }
    })?;
    Ok(())
}
