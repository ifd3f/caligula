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
        let mut interval = time::interval(time::Duration::from_secs(1));
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
                            self.state = State::Complete;
                        }
                    }
                },
                State::Complete => select! {
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
            StatusMessage::TotalBytesWritten(b) => {
                self.history.push(now, b as u64);
            }
            _ => {}
        }
    }

    fn bytes_written(&self) -> ByteSize {
        self.history.bytes_written()
    }

    fn draw(&mut self) -> anyhow::Result<()> {
        let progress = self.history.make_progress_bar();
        let chart = self.history.make_speed_chart();

        let info_table = Table::new(vec![
            Row::new([
                Cell::from("Input"),
                Cell::from(self.input_filename.as_str()),
            ]),
            Row::new([
                Cell::from("Output"),
                Cell::from(self.target_filename.as_str()),
            ]),
        ])
        .style(Style::default())
        .widths(&[Constraint::Length(7), Constraint::Min(10)])
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
    Complete,
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
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(info_pane);

        Self {
            graph: root[1],
            progress: root[0],
            args_display: info_children[0],
            estimation: info_children[1],
        }
    }
}
