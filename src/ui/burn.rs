use std::time::Instant;

use bytesize::ByteSize;
use crossterm::event::EventStream;
use futures::StreamExt;
use tokio::{select, time};
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
    handle: burn::Handle,
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
            handle,
            state: State::Burning,
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
                State::Burning => select! {
                    _ = interval.tick() => {
                        debug!("Got interval tick");
                    }
                    event = events.next() => {
                        debug!(event = format!("{event:?}"), "Got terminal event")
                    }
                    msg = self.handle.next_message() => {
                        debug!(msg = format!("{msg:?}"), "Got child process message");

                        if let Some(m) = msg? {
                            self.on_message(m).await
                        } else {
                            self.state = State::Complete;
                        }
                    }
                },
                State::Complete => {}
            }

            self.draw()?;
        }
    }

    async fn on_message(&mut self, msg: StatusMessage) {
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
                        history.iter().copied().map(|(x, _)| x).fold(0.0, f64::max),
                    ]))
                    .y_axis(Axis::default().title("Bytes written").bounds([
                        0.0,
                        history.iter().copied().map(|(_, y)| y).fold(0.0, f64::max),
                    ])),
                layout.graph,
            );
        })?;
        Ok(())
    }
}

enum State {
    Burning,
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
                Constraint::Min(5),
            ])
            .split(value);

        Self {
            graph: chunks[0],
            progress: chunks[1],
            info: chunks[2],
        }
    }
}
