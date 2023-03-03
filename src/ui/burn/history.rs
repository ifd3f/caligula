use std::time::Instant;

use bytesize::ByteSize;
use tui::{
    layout::Alignment,
    style::{Color, Style},
    symbols,
    text::Span,
    widgets::{Axis, BarChart, Block, Borders, Chart, Dataset, Gauge, GraphType, Widget},
};

pub struct History {
    max_bytes: ByteSize,
    raw: Vec<(f64, ByteSize)>,
    speed_data: Vec<(f64, f64)>,
    start: Instant,
}

impl History {
    pub fn new(start: Instant, max_bytes: ByteSize) -> Self {
        Self {
            max_bytes,
            raw: Vec::new(),
            speed_data: vec![(0.0, 0.0)],
            start,
        }
    }

    pub fn push(&mut self, time: Instant, bytes: u64) {
        let secs = time.duration_since(self.start).as_secs_f64();
        let (last_time, last_bw) = self.last_datapoint();
        let dt = secs - last_time;
        let diff = bytes - last_bw.0;
        let speed = diff as f64 / dt;

        self.raw.push((secs, ByteSize::b(bytes)));
        self.speed_data.push((secs, speed));
    }

    pub fn last_datapoint(&self) -> (f64, ByteSize) {
        self.raw
            .last()
            .map(|x| x.clone())
            .unwrap_or((0.0, ByteSize::b(0)))
    }

    pub fn bytes_written(&self) -> ByteSize {
        self.last_datapoint().1
    }

    pub fn latest_time_secs(&self) -> f64 {
        self.last_datapoint().0
    }

    pub fn max_speed(&self) -> f64 {
        self.speed_data.iter().map(|x| x.1).fold(0.0, f64::max)
    }

    pub fn make_progress_bar(&self, done: bool) -> impl Widget {
        let bw = self.bytes_written();
        let max = self.max_bytes();

        Gauge::default()
            .label(format!("{} / {}", bw, max))
            .gauge_style(Style::default().fg(if done { Color::Green } else { Color::Yellow }))
            .ratio((bw.0 as f64) / (max.0 as f64))
    }

    pub fn make_speed_chart(&self) -> Chart {
        let max_speed = self.max_speed();
        let max_time = f64::max(self.latest_time_secs(), 3.0);

        let n_x_ticks = 5;
        let n_y_ticks = 4;

        let x_ticks: Vec<_> = (0..=n_x_ticks)
            .map(|i| {
                let x = i as f64 * max_time / n_x_ticks as f64;
                Span::from(format!("{x:.1}s"))
            })
            .collect();

        let y_ticks: Vec<_> = (0..=n_y_ticks)
            .map(|i| {
                let y = i as f64 * max_speed / n_y_ticks as f64;
                let bytes = ByteSize::b(y as u64);
                Span::from(format!("{bytes}/s"))
            })
            .collect();

        let bytes_written_dataset = Dataset::default()
            .name("Bytes written")
            .graph_type(GraphType::Scatter)
            .marker(symbols::Marker::Braille)
            .style(Style::default().fg(Color::Green))
            .graph_type(GraphType::Line)
            .data(&self.speed_data);

        let chart = Chart::new(vec![bytes_written_dataset])
            .block(Block::default().title("Speed").borders(Borders::ALL))
            .x_axis(
                Axis::default()
                    .title("Time")
                    .bounds([0.0, max_time])
                    .labels(x_ticks)
                    .labels_alignment(Alignment::Right),
            )
            .y_axis(
                Axis::default()
                    .bounds([0.0, max_speed])
                    .labels(y_ticks)
                    .labels_alignment(Alignment::Right),
            );

        chart
    }

    pub fn max_bytes(&self) -> ByteSize {
        self.max_bytes
    }
}
