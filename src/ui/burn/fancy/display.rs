use std::time::Instant;

use crossterm::event::EventStream;
use futures::StreamExt;
use ratatui::{
    backend::Backend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::Text,
    widgets::{Block, Borders, Paragraph, Wrap},
    Terminal,
};
use tokio::{select, time};
use tracing::debug;

use crate::{
    logging::get_bug_report_msg,
    ui::burn::{
        fancy::{
            state::UIEvent,
            widgets::{DiskList, DiskListEntry},
        },
        start::BeginParams,
    },
    writer_process::{self, state_tracking::WriterState, Handle},
};

use super::{
    state::{Quit, State},
    widgets::{SpeedChart, WriterProgressBar, WritingInfoTable},
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
    pub fn new(
        params: &BeginParams,
        handle: writer_process::Handle,
        terminal: &'a mut Terminal<B>,
    ) -> Self {
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
    disks: Rect,
    toplevel_status: Rect,
    progress: Rect,
    graph: Rect,
    args_display: Rect,
}

impl From<Rect> for ComputedLayout {
    fn from(root: Rect) -> Self {
        let root = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(10), Constraint::Length(1)])
            .split(root);

        let infopane_and_rightpane = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(16), Constraint::Percentage(75)])
            .split(root[0]);

        let within_rightpane = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(7),
                Constraint::Length(7),
            ])
            .split(infopane_and_rightpane[1]);

        Self {
            toplevel_status: root[1],
            disks: infopane_and_rightpane[0],
            graph: within_rightpane[1],
            progress: within_rightpane[0],
            args_display: within_rightpane[2],
        }
    }
}

pub fn draw(
    state: &mut State,
    terminal: &mut Terminal<impl ratatui::backend::Backend>,
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

        f.render_stateful_widget(speed_chart, layout.graph, &mut state.graph_state);
        f.render_widget(
            progress_bar.as_gauge().label(progress_bar.label()),
            layout.progress,
        );

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

        f.render_widget(
            Paragraph::new(Text::raw("↑/↓ (select disk)   n (new disk)"))
                .style(Style::new().bg(Color::LightBlue)),
            layout.toplevel_status,
        );

        debug!(?layout.disks, "test");
        let disks_block = Block::default().borders(Borders::RIGHT);
        let actual_disks = disks_block.inner(layout.disks);
        f.render_widget(disks_block, layout.disks);
        f.render_widget(
            DiskList {
                disks: &[DiskListEntry {
                    name: "/dev/whatever",
                    state: &state.child,
                }],
            },
            actual_disks,
        );
    })?;

    Ok(())
}
