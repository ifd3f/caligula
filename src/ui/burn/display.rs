use std::time::Instant;

use bytesize::ByteSize;
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures::StreamExt;
use tokio::{select, time};
use tracing::debug;
use tui::{
    backend::Backend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Cell, Row, Table},
    Terminal,
};

use crate::{
    burn::{self, ipc::StatusMessage, Handle},
    cli::Args,
    device::BurnTarget,
};

use super::history::History;

pub struct UI<'a, B>
where
    B: Backend,
{
    terminal: &'a mut Terminal<B>,
    events: EventStream,
    state: State,
}

impl<'a, B> UI<'a, B>
where
    B: Backend,
{
    pub fn new(
        handle: burn::Handle,
        terminal: &'a mut Terminal<B>,
        target: BurnTarget,
        args: &'a Args,
    ) -> Self {
        Self {
            terminal,
            events: EventStream::new(),
            state: State {
                input_filename: args.input.to_string_lossy().to_string(),
                target_filename: target.devnode.to_string_lossy().to_string(),
                history: History::new(
                    Instant::now(),
                    ByteSize::b(handle.initial_info().input_file_bytes),
                ),
                child: ChildState::Burning { handle },
            },
        }
    }

    pub async fn show(mut self) -> anyhow::Result<()> {
        loop {
            let loop_result: anyhow::Result<()> = self.get_and_handle_events().await;

            if let Err(e) = loop_result {
                match e.downcast::<Quit>()? {
                    Quit => return Ok(()),
                }
            }
        }
    }

    async fn get_and_handle_events(&mut self) -> anyhow::Result<()> {
        let msg = {
            let handle = self.state.child.child_process();
            if let Some(handle) = handle {
                child_active(&mut self.events, handle).await
            } else {
                child_dead(&mut self.events).await
            }?
        };
        self.state.on_message(msg)?;
        self.state.draw(self.terminal)?;
        Ok(())
    }
}

async fn child_dead(events: &mut EventStream) -> anyhow::Result<UIEvent> {
    Ok(UIEvent::TermEvent(events.next().await.unwrap()?))
}

async fn child_active(events: &mut EventStream, handle: &mut Handle) -> anyhow::Result<UIEvent> {
    let sleep = tokio::time::sleep(time::Duration::from_millis(250));
    select! {
        _ = sleep => {
            return Ok(UIEvent::Sleep);
        }
        msg = handle.next_message() => {
            return Ok(UIEvent::Child(msg?));
        }
        event = events.next() => {
            return Ok(UIEvent::TermEvent(event.unwrap()?));
        }
    }
}

enum UIEvent {
    Sleep,
    Child(Option<StatusMessage>),
    TermEvent(Event),
}

struct State {
    input_filename: String,
    target_filename: String,
    history: History,
    child: ChildState,
}

enum ChildState {
    Burning {
        handle: Handle,
    },
    Verifying {
        handle: Handle,
    },
    Finished {
        finish_time: Instant,
        error: Option<String>,
    },
}

impl State {
    fn on_message(&mut self, msg: UIEvent) -> anyhow::Result<()> {
        match msg {
            UIEvent::Sleep => {}
            UIEvent::Child(m) => self.on_child_status(m),
            UIEvent::TermEvent(e) => self.on_term_event(e)?,
        };
        Ok(())
    }

    fn on_term_event(&mut self, ev: Event) -> anyhow::Result<()> {
        match ev {
            Event::Key(KeyEvent {
                code: KeyCode::Char('c'),
                modifiers: KeyModifiers::CONTROL,
                kind: KeyEventKind::Press,
                ..
            }) => {
                debug!("Got CTRL-C, quitting");
                Err(Quit)?
            }
            _ => Ok(()),
        }
    }

    fn on_child_status(&mut self, msg: Option<StatusMessage>) {
        let now = Instant::now();
        match msg {
            Some(StatusMessage::TotalBytes(b)) => {
                self.history.push(now, b as u64);
            }
            None => {
                self.child = ChildState::Finished {
                    finish_time: now,
                    error: None,
                }
            }
            _ => {}
        }
    }

    fn draw(&mut self, terminal: &mut Terminal<impl tui::backend::Backend>) -> anyhow::Result<()> {
        let final_time = match self.child {
            ChildState::Finished { finish_time, .. } => finish_time,
            _ => Instant::now(),
        };

        let progress = self
            .history
            .make_progress_bar(self.child.bar_text())
            .gauge_style(self.child.bar_style());

        let chart = self.history.make_speed_chart(final_time);

        let info_table = Table::new(vec![
            Row::new([
                Cell::from("Input"),
                Cell::from(self.input_filename.as_str()),
            ]),
            Row::new([
                Cell::from("Output"),
                Cell::from(self.target_filename.as_str()),
            ]),
            Row::new([
                Cell::from("Total Speed"),
                Cell::from(format!("{}", self.history.total_avg_speed(final_time))),
            ]),
            Row::new([
                Cell::from("Current Speed"),
                Cell::from(format!("{}", self.history.last_speed())),
            ]),
            Row::new([
                Cell::from("ETA"),
                Cell::from(format!("{}", self.history.estimated_time_left(final_time))),
            ]),
        ])
        .style(Style::default())
        .widths(&[Constraint::Length(16), Constraint::Percentage(100)])
        .block(Block::default().title("Stats").borders(Borders::ALL));

        terminal.draw(|f| {
            let layout = ComputedLayout::from(f.size());

            f.render_widget(progress, layout.progress);
            f.render_widget(chart, layout.graph);
            f.render_widget(info_table, layout.args_display);
        })?;
        Ok(())
    }
}

impl ChildState {
    fn bar_text(&self) -> &'static str {
        match self {
            Self::Burning { .. } => "Burning...",
            Self::Verifying { .. } => "Verifying...",
            Self::Finished { error, .. } => match error {
                Some(_) => "Error!",
                None => "Complete!",
            },
        }
    }

    fn bar_style(&self) -> Style {
        match self {
            Self::Burning { .. } => Style::default().fg(Color::Yellow).bg(Color::Black),
            Self::Verifying { .. } => Style::default().fg(Color::Blue).bg(Color::Yellow),
            Self::Finished { error, .. } => match error {
                Some(_) => Style::default().fg(Color::Red).bg(Color::Black),
                None => Style::default().fg(Color::Green).bg(Color::Green),
            },
        }
    }

    fn child_process(&mut self) -> Option<&mut Handle> {
        match self {
            Self::Burning { handle, .. } => Some(handle),
            Self::Verifying { handle, .. } => Some(handle),
            Self::Finished { .. } => None,
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

#[derive(Debug, thiserror::Error)]
#[error("User sent quit signal")]
struct Quit;
