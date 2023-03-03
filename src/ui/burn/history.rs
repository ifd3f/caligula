use std::time::Instant;

use bytesize::ByteSize;
use tui::{
    style::{Color, Style},
    symbols,
    text::Span,
    widgets::{Axis, Block, Borders, Chart, Dataset, Gauge, GraphType},
};

pub struct History {
    max_bytes: ByteSize,
    raw: Vec<(f64, ByteSize)>,
    cum_data: Vec<(f64, f64)>,
    speed_data: Vec<(f64, f64)>,
    start: Instant,
}

impl History {
    pub fn new(start: Instant, max_bytes: ByteSize) -> Self {
        Self {
            max_bytes,
            raw: Vec::new(),
            cum_data: Vec::new(),
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
        self.cum_data.push((secs, bytes as f64));
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

    pub fn make_progress_bar(&self) -> Gauge {
        let bw = self.bytes_written();
        let max = self.max_bytes();

        Gauge::default()
            .label(format!("{} / {}", bw, max))
            .gauge_style(Style::default().fg(Color::Green))
            .ratio((bw.0 as f64) / (max.0 as f64))
    }

    pub fn make_speed_chart(&self) -> Chart {
        let bytes_written_dataset = Dataset::default()
            .name("Bytes written")
            .graph_type(GraphType::Scatter)
            .marker(symbols::Marker::Dot)
            .style(Style::default().fg(Color::Yellow))
            .graph_type(GraphType::Line)
            .data(&self.speed_data);

        let chart = Chart::new(vec![bytes_written_dataset])
            .block(Block::default().title("Speed").borders(Borders::ALL))
            .x_axis(
                Axis::default()
                    .title("Time")
                    .bounds([0.0, f64::max(self.latest_time_secs(), 3.0)]),
            )
            .y_axis(
                Axis::default()
                    .title("Bytes written")
                    .bounds([0.0, self.max_speed()]),
            );

        chart
    }

    pub fn max_bytes(&self) -> ByteSize {
        self.max_bytes
    }
}
