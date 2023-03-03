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
    burn::{self, ipc::StatusMessage},
    cli::Args,
    device::BurnTarget,
};

use super::history::History;

pub async fn show<'a>(
    handle: burn::Handle,
    target: BurnTarget,
    args: &'a Args,
    terminal: &'a mut Terminal<impl tui::backend::Backend>,
) -> anyhow::Result<()> {
    let ui = UI::new(
        ByteSize::b(handle.initial_info().input_file_bytes),
        target,
        args,
    );
    let ctx = Ctx {
        handle,
        terminal,
        ui,
        events: EventStream::new(),
    };

    ctx.run().await
}

struct Ctx<'a, B>
where
    B: Backend,
{
    handle: burn::Handle,
    terminal: &'a mut Terminal<B>,
    events: EventStream,
    ui: UI,
}

struct UI {
    input_filename: String,
    target_filename: String,
    history: History,
    state: State,
}

impl<'a, B> Ctx<'a, B>
where
    B: Backend,
{
    async fn run(mut self) -> anyhow::Result<()> {
        loop {
            let loop_result: anyhow::Result<()> = async {
                if let State::Finished { .. } = self.ui.state {
                    self.child_dead().await?;
                } else {
                    self.child_active().await?;
                }
                Ok(())
            }
            .await;

            if let Err(e) = loop_result {
                match e.downcast::<Quit>()? {
                    Quit => return Ok(()),
                }
            }
        }
    }

    async fn child_active(&mut self) -> anyhow::Result<()> {
        let sleep = tokio::time::sleep(time::Duration::from_millis(250));
        select! {
            _ = sleep => {}
            msg = self.handle.next_message() => {
                self.ui.on_message(msg?);
            }
            event = self.events.next() => {
                self.ui.on_term_event(event.unwrap()?)?;
            }
        };
        self.ui.draw(self.terminal)?;
        Ok(())
    }

    async fn child_dead(&mut self) -> anyhow::Result<()> {
        let event = self.events.next().await;
        self.ui.on_term_event(event.unwrap()?)?;
        self.ui.draw(self.terminal)?;
        Ok(())
    }
}

impl UI {
    fn new(max_bytes: ByteSize, target: BurnTarget, args: &Args) -> Self {
        let history = History::new(Instant::now(), max_bytes);
        Self {
            target_filename: target.devnode.to_string_lossy().to_string(),
            input_filename: args.input.to_string_lossy().to_string(),
            history,
            state: State::Burning,
        }
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

    fn on_message(&mut self, msg: Option<StatusMessage>) {
        let now = Instant::now();
        let msg = match msg {
            Some(m) => m,
            None => {
                self.history.finished_at(now);
                self.state = State::Finished {
                    finish_time: now,
                    error: None,
                };
                return;
            }
        };
        match msg {
            StatusMessage::TotalBytes(b) => {
                self.history.push(now, b as u64);
            }
            _ => {}
        }
    }

    fn draw(&mut self, terminal: &mut Terminal<impl tui::backend::Backend>) -> anyhow::Result<()> {
        let final_time = match self.state {
            State::Finished { finish_time, .. } => finish_time,
            _ => Instant::now(),
        };

        let progress = self
            .history
            .make_progress_bar(self.state.bar_text())
            .gauge_style(self.state.bar_style());

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

enum State {
    Burning,
    Verifying,
    Finished {
        finish_time: Instant,
        error: Option<String>,
    },
}

impl State {
    fn bar_text(&self) -> &'static str {
        match self {
            State::Burning => "Burning...",
            State::Verifying => "Verifying...",
            State::Finished { error, .. } => match error {
                Some(_) => "Error!",
                None => "Complete!",
            },
        }
    }

    fn bar_style(&self) -> Style {
        match self {
            State::Burning => Style::default().fg(Color::Yellow).bg(Color::Black),
            State::Verifying => Style::default().fg(Color::Blue).bg(Color::Yellow),
            State::Finished { error, .. } => match error {
                Some(_) => Style::default().fg(Color::Red).bg(Color::Black),
                None => Style::default().fg(Color::Green).bg(Color::Green),
            },
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
