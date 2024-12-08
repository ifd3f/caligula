use std::{sync::Arc, time::Instant};

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
    logging::LogPaths,
    ui::{herder::WriterHandle, start::BeginParams, writer_tracking::WriterState},
};

use super::{
    state::{Quit, State, UIEvent},
    widgets::{SpeedChart, WriterProgressBar, WritingInfoTable},
};

pub struct FancyUI<'a, B>
where
    B: Backend,
{
    terminal: &'a mut Terminal<B>,
    events: EventStream,
    handle: Option<WriterHandle>,
    state: State,
    log_paths: Arc<LogPaths>,
}

impl<'a, B> FancyUI<'a, B>
where
    B: Backend,
{
    #[tracing::instrument(skip_all)]
    pub fn new(
        params: &BeginParams,
        handle: WriterHandle,
        terminal: &'a mut Terminal<B>,
        log_paths: Arc<LogPaths>,
    ) -> Self {
        let input_file_bytes = handle.initial_info().input_file_bytes;
        Self {
            terminal,
            handle: Some(handle),
            events: EventStream::new(),
            state: State::initial(Instant::now(), params, input_file_bytes),
            log_paths,
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

        draw(&mut self.state, self.terminal, &self.log_paths)?;
        Ok(self)
    }
}

async fn get_event_child_dead(ui_events: &mut EventStream) -> anyhow::Result<UIEvent> {
    Ok(UIEvent::RecvTermEvent(ui_events.next().await.unwrap()?))
}

#[tracing::instrument(skip_all, level = "trace")]
async fn get_event_child_active(
    ui_events: &mut EventStream,
    child_events: &mut WriterHandle,
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
    quit_modal: Rect,
}

impl From<Rect> for ComputedLayout {
    fn from(area: Rect) -> Self {
        let root = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(10),
                Constraint::Length(10),
            ])
            .split(area);

        let info_pane = root[2];

        let quit_modal = centered_rect(area, 40, 4);

        Self {
            graph: root[1],
            progress: root[0],
            args_display: info_pane,
            quit_modal,
        }
    }
}

/// Given an outer rect and desired inner rect dimensions, returns the inner rect.
fn centered_rect(r: Rect, w: u16, h: u16) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(h),
            Constraint::Fill(1),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(w),
            Constraint::Fill(1),
        ])
        .split(popup_layout[1])[1]
}

pub fn draw(
    state: &mut State,
    terminal: &mut Terminal<impl ratatui::backend::Backend>,
    log_paths: &LogPaths,
) -> anyhow::Result<()> {
    let progress_bar = WriterProgressBar::from_writer(&state.child);

    let final_time = match state.child {
        WriterState::Finished { finish_time, .. } => finish_time,
        _ => Instant::now(),
    };

    let error = match &state.child {
        WriterState::Finished { error, .. } => error.as_ref(),
        _ => None,
    };

    let info_table = WritingInfoTable {
        input_filename: &state.input_filename,
        target_filename: &state.target_filename,
        state: &state.child,
    };

    let speed_chart = SpeedChart {
        state: &state.child,
        final_time,
    };

    terminal.draw(|f| {
        let layout = ComputedLayout::from(f.size());

        f.render_widget(progress_bar.render(), layout.progress);
        f.render_stateful_widget(speed_chart, layout.graph, &mut state.graph_state);

        if let Some(error) = error {
            f.render_widget(
                Paragraph::new(format!("{error}\n{}", log_paths.get_bug_report_msg()))
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

        if let Some(qm) = state.quit_modal {
            f.render_widget(qm, layout.quit_modal)
        }
    })?;
    Ok(())
}
