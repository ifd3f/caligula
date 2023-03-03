use std::time::Instant;

use bytesize::ByteSize;
use crossterm::event::{Event, EventStream, KeyEvent, KeyModifiers, KeyEventKind, KeyEventState, KeyCode};
use futures::StreamExt;
use tokio::{select, signal, time};
use tracing::debug;
use tui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    symbols,
    widgets::{Axis, BarChart, Block, Borders, Chart, Dataset, Gauge, GraphType},
    Terminal,
};

use crate::{
    burn::{self, ipc::StatusMessage, Handle},
    cli::Args,
    device::BurnTarget,
};

pub struct BurningDisplay<'a, B>
where
    B: tui::backend::Backend,
{
    args: &'a Args,
    terminal: &'a mut Terminal<B>,
    target: BurnTarget,
    state: State,
    start: Instant,
    bytes_total: ByteSize,
    bytes_history: Vec<(Instant, ByteSize)>,
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
        let bytes_total = ByteSize::b(handle.initial_info().input_file_bytes);
        Self {
            state: State::Burning { handle },
            target,
            args,
            terminal,
            start: Instant::now(),
            bytes_total,
            bytes_history: Vec::new(),
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
                let bytes = ByteSize::b(b as u64);
                self.bytes_history.push((now, bytes));
            }
            _ => {}
        }
    }

    fn bytes_written(&self) -> ByteSize {
        self.bytes_history
            .last()
            .map(|(_, bytes)| bytes.clone())
            .unwrap_or(ByteSize::b(0))
    }

    fn draw(&mut self) -> anyhow::Result<()> {
        let bytes_written = self.bytes_written();
        let start = self.start;
        let history: Vec<_> = self
            .bytes_history
            .iter()
            .map(|(t, b)| {
                let s = t.duration_since(start).as_secs_f64();
                let b = b.as_u64() as f64;
                (s, b)
            })
            .collect();

        self.terminal.draw(|f| {
            let layout = ComputedLayout::from(f.size());

            f.render_widget(
                Gauge::default()
                    .block(Block::default().title("Progress"))
                    .label(format!("{} / {}", bytes_written, self.bytes_total))
                    .gauge_style(Style::default().fg(Color::Green))
                    .ratio((bytes_written.as_u64() as f64) / (self.bytes_total.as_u64() as f64)),
                layout.progress,
            );

            let written_dataset = Dataset::default()
                .name("Bytes written")
                .graph_type(GraphType::Line)
                .marker(symbols::Marker::Block)
                .style(Style::default().fg(Color::Yellow))
                .graph_type(GraphType::Line)
                .data(&history);
            f.render_widget(
                Chart::new(vec![written_dataset])
                    .block(Block::default().title("Speed").borders(Borders::ALL))
                    .x_axis(Axis::default().title("Time").bounds([
                        0.0,
                        history.iter().copied().map(|(x, _)| x).fold(5.0, f64::max),
                    ]))
                    .y_axis(
                        Axis::default()
                            .title("Bytes written")
                            .bounds([0.0, self.bytes_total.as_u64() as f64]),
                    ),
                layout.graph,
            );
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
    info: Rect,
    graph: Rect,
}

impl From<Rect> for ComputedLayout {
    fn from(value: Rect) -> Self {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(2)
            .constraints([
                Constraint::Min(10),
                Constraint::Length(2),
                Constraint::Length(5),
            ])
            .split(value);

        Self {
            graph: chunks[0],
            progress: chunks[1],
            info: chunks[2],
        }
    }
}
