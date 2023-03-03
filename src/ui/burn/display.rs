use std::time::Instant;

use bytesize::ByteSize;
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures::StreamExt;
use tokio::{select, time};
use tracing::debug;
use tui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    symbols,
    widgets::{
        Axis, Block, Borders, Cell, Chart, Dataset, Gauge, GraphType, Paragraph, Row, Table,
    },
    Terminal,
};

use crate::{
    burn::{self, ipc::StatusMessage, Handle},
    cli::Args,
    device::BurnTarget,
};

use super::history::History;

pub struct BurningDisplay<'a, B>
where
    B: tui::backend::Backend,
{
    terminal: &'a mut Terminal<B>,
    input_filename: String,
    target_filename: String,
    state: State,
    history: History,
}

impl<'a, B> BurningDisplay<'a, B>
where
    B: tui::backend::Backend,
{
    pub fn new(
        handle: burn::Handle,
        target: BurnTarget,
        args: &'a Args,
        terminal: &'a mut Terminal<B>,
    ) -> Self {
        let max_bytes = ByteSize::b(handle.initial_info().input_file_bytes);
        let history = History::new(Instant::now(), max_bytes);
        Self {
            state: State::Burning { handle },
            target_filename: target.devnode.to_string_lossy().to_string(),
            input_filename: args.input.to_string_lossy().to_string(),
            terminal,
            history,
        }
    }

    pub async fn show(&mut self) -> anyhow::Result<()> {
        let mut interval = time::interval(time::Duration::from_millis(250));
        let mut events = EventStream::new();

        loop {
            match &mut self.state {
                State::Burning { handle } => select! {
                    _ = interval.tick() => {
                        debug!("Got interval tick");
                    }
                    event = events.next() => {
                        debug!(event = format!("{event:?}"), "Got terminal event");
                        if let Some(ev) = event {
                            if self.handle_event(ev?) {
                                return Ok(());
                            }
                        } else {
                            return Ok(());
                        }
                    }
                    msg = handle.next_message() => {
                        debug!(msg = format!("{msg:?}"), "Got child process message");

                        if let Some(m) = msg? {
                            self.on_message(m)
                        } else {
                            let now = Instant::now();
                            self.history.finished_at(now);
                            self.state = State::Complete {
                                finish_time: now
                            };
                        }
                    }
                },
                State::Complete { .. } => select! {
                    _ = interval.tick() => {
                        debug!("Got interval tick");
                    }
                    event = events.next() => {
                        debug!(event = format!("{event:?}"), "Got terminal event");
                        if let Some(ev) = event {
                            if self.handle_event(ev?) {
                                return Ok(());
                            }
                        } else {
                            return Ok(());
                        }
                    }
                },
            }

            self.draw()?;
        }
    }

    fn handle_event(&mut self, ev: Event) -> bool {
        match ev {
            Event::Key(KeyEvent {
                code: KeyCode::Char('c'),
                modifiers: KeyModifiers::CONTROL,
                kind: KeyEventKind::Press,
                ..
            }) => {
                debug!("Got CTRL-C, quitting");
                return true;
            }
            _ => {}
        }
        return false;
    }

    fn on_message(&mut self, msg: StatusMessage) {
        let now = Instant::now();
        match msg {
            StatusMessage::TotalBytes(b) => {
                self.history.push(now, b as u64);
            }
            _ => {}
        }
    }

    fn draw(&mut self) -> anyhow::Result<()> {
        let final_time = match self.state {
            State::Burning { .. } => Instant::now(),
            State::Complete { finish_time } => finish_time,
        };

        let progress = self
            .history
            .make_progress_bar(self.state.bar_text())
            .gauge_style(Style::default().fg(self.state.bar_color()));

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
        .widths(&[Constraint::Length(16), Constraint::Min(20)])
        .block(Block::default().title("Stats").borders(Borders::ALL));

        self.terminal.draw(|f| {
            let layout = ComputedLayout::from(f.size());

            f.render_widget(progress, layout.progress);
            f.render_widget(chart, layout.graph);
            f.render_widget(info_table, layout.args_display);
        })?;
        Ok(())
    }
}

enum State {
    Burning { handle: Handle },
    Complete { finish_time: Instant },
}
impl State {
    fn bar_text(&self) -> &'static str {
        match self {
            State::Burning { .. } => "Burning...",
            State::Complete { .. } => "Done!",
        }
    }

    fn bar_color(&self) -> Color {
        match self {
            State::Burning { .. } => Color::Yellow,
            State::Complete { .. } => Color::Green,
        }
    }
}

struct ComputedLayout {
    progress: Rect,
    graph: Rect,
    args_display: Rect,
    estimation: Rect,
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

        let info_children = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(info_pane);

        Self {
            graph: root[1],
            progress: root[0],
            args_display: info_children[0],
            estimation: info_children[1],
        }
    }
}
