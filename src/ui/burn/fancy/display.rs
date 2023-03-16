use std::time::Instant;

use crossterm::event::EventStream;
use futures::StreamExt;
use tokio::{select, time};
use tui::{
    backend::Backend,
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap},
    Terminal,
};

use crate::{
    burn::{self, state_tracking::ChildState, Handle},
    logging::get_bug_report_msg,
    ui::burn::{fancy::state::UIEvent, start::BeginParams},
};

use super::{
    history::History,
    state::{Quit, State},
};

pub struct FancyUI<'a, B>
where
    B: Backend,
{
    terminal: &'a mut Terminal<B>,
    events: EventStream,
    handle: Option<burn::Handle>,
    state: State,
}

impl<'a, B> FancyUI<'a, B>
where
    B: Backend,
{
    pub fn new(params: &BeginParams, handle: burn::Handle, terminal: &'a mut Terminal<B>) -> Self {
        let input_file_bytes = handle.initial_info().input_file_bytes;
        Self {
            terminal,
            handle: Some(handle),
            events: EventStream::new(),
            state: State::initial(&params, input_file_bytes),
        }
    }

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

    async fn get_and_handle_events(mut self) -> anyhow::Result<FancyUI<'a, B>> {
        let msg = {
            if let Some(handle) = &mut self.handle {
                child_active(&mut self.events, handle).await
            } else {
                child_dead(&mut self.events).await
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

async fn child_dead(events: &mut EventStream) -> anyhow::Result<UIEvent> {
    Ok(UIEvent::RecvTermEvent(events.next().await.unwrap()?))
}

async fn child_active(events: &mut EventStream, handle: &mut Handle) -> anyhow::Result<UIEvent> {
    let sleep = tokio::time::sleep(time::Duration::from_millis(250));
    select! {
        _ = sleep => {
            return Ok(UIEvent::SleepTimeout);
        }
        msg = handle.next_message() => {
            return Ok(UIEvent::RecvChildStatus(Instant::now(), msg?));
        }
        event = events.next() => {
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
    terminal: &mut Terminal<impl tui::backend::Backend>,
) -> anyhow::Result<()> {
    let history = History::from(&state.child);
    let wdata = history.write_data();

    let final_time = match state.child {
        ChildState::Finished { finish_time, .. } => finish_time,
        _ => Instant::now(),
    };

    let error = match &state.child {
        ChildState::Finished { error, .. } => error.as_ref(),
        _ => None,
    };

    let mut rows = vec![
        Row::new([
            Cell::from("Input"),
            Cell::from(state.input_filename.as_str()),
        ]),
        Row::new([
            Cell::from("Output"),
            Cell::from(state.target_filename.as_str()),
        ]),
        Row::new([
            Cell::from("Avg. Write"),
            Cell::from(format!("{}", wdata.total_avg_speed())),
        ]),
    ];

    match &state.child {
        ChildState::Burning {
            max_bytes,
            read_hist,
            input_file_bytes,
            ..
        } => {
            rows.push(Row::new([
                Cell::from("ETA Write"),
                Cell::from(format!(
                    "{}",
                    match max_bytes {
                        Some(m) => wdata.estimated_time_left(*m),
                        None => read_hist.estimated_time_left(*input_file_bytes),
                    }
                )),
            ]));
        }
        ChildState::Verifying {
            verify_hist: vdata,
            max_bytes,
            ..
        } => {
            rows.push(Row::new([
                Cell::from("Avg. Verify"),
                Cell::from(format!("{}", vdata.total_avg_speed())),
            ]));
            rows.push(Row::new([
                Cell::from("ETA verify"),
                Cell::from(format!("{}", vdata.estimated_time_left(*max_bytes))),
            ]));
        }
        ChildState::Finished {
            verify_hist: vdata, ..
        } => {
            if let Some(vdata) = vdata {
                rows.push(Row::new([
                    Cell::from("Avg. Verify"),
                    Cell::from(format!("{}", vdata.total_avg_speed())),
                ]));
            }
        }
    }

    let info_table = Table::new(rows)
        .style(Style::default())
        .widths(&[Constraint::Length(16), Constraint::Percentage(100)])
        .block(Block::default().title("Stats").borders(Borders::ALL));

    terminal.draw(|f| {
        let layout = ComputedLayout::from(f.size());

        history.draw_progress(f, layout.progress);
        state
            .ui_state
            .draw_speed_chart(&history, f, layout.graph, final_time);

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
