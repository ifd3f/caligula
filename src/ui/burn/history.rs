use std::{fmt::Display, time::Instant};

use bytesize::ByteSize;
use tui::{
    layout::Alignment,
    style::{Color, Style},
    symbols,
    text::Span,
    widgets::{Axis, Block, Borders, Chart, Dataset, GraphType},
};

use crate::ui::utils::ByteSpeed;

pub struct History {
    max_bytes: ByteSize,
    raw_write: Vec<(f64, ByteSize)>,
    raw_verify: Vec<(f64, ByteSize)>,
    write_speed_data: Vec<(f64, f64)>,
    verify_speed_data: Vec<(f64, f64)>,
    start: Instant,
}

impl History {
    pub fn new(start: Instant, max_bytes: ByteSize) -> Self {
        Self {
            max_bytes,
            raw_write: Vec::new(),
            raw_verify: Vec::new(),
            write_speed_data: vec![(0.0, 0.0)],
            verify_speed_data: vec![(0.0, 0.0)],
            start,
        }
    }

    pub fn push_writing(&mut self, time: Instant, bytes: u64) {
        let secs = time.duration_since(self.start).as_secs_f64();
        let (last_time, last_bw) = self.last_datapoint();
        let dt = secs - last_time;
        let diff = bytes - last_bw.0;
        let speed = diff as f64 / dt;

        self.raw_write.push((secs, ByteSize::b(bytes)));
        self.write_speed_data.push((secs, speed));
    }

    pub fn push_verifying(&mut self, time: Instant, bytes: u64) {
        let secs = time.duration_since(self.start).as_secs_f64();
        let (last_time, last_bw) = self.last_datapoint();
        let dt = secs - last_time;
        let diff = bytes - last_bw.0;
        let speed = diff as f64 / dt;

        self.raw_write.push((secs, ByteSize::b(bytes)));
        self.write_speed_data.push((secs, speed));
    }

    pub fn finished_at(&mut self, time: Instant) {
        self.push_writing(time, self.max_bytes.0);
    }

    pub fn last_datapoint(&self) -> (f64, ByteSize) {
        self.raw_write
            .last()
            .map(|x| x.clone())
            .unwrap_or((0.0, ByteSize::b(0)))
    }

    pub fn bytes_written(&self) -> ByteSize {
        self.last_datapoint().1
    }

    pub fn total_avg_speed(&self, final_time: Instant) -> ByteSpeed {
        let s = self.bytes_written();
        let dt = final_time.duration_since(self.start).as_secs_f64();
        let speed = s.0 as f64 / dt;
        ByteSpeed(if speed.is_nan() { 0.0 } else { speed })
    }

    pub fn estimated_time_left(&self, final_time: Instant) -> EstimatedTime {
        let speed = self.total_avg_speed(final_time).0;
        let bytes_left = self.max_bytes().0 - self.bytes_written().0;
        let secs_left = bytes_left as f64 / speed;
        EstimatedTime::from(secs_left)
    }

    pub fn last_speed_data(&self) -> (f64, f64) {
        self.write_speed_data
            .last()
            .map(|x| x.clone())
            .unwrap_or((0.0, 0.0))
    }

    pub fn last_speed(&self) -> ByteSpeed {
        let (_, s) = self.last_speed_data();
        ByteSpeed(s)
    }

    pub fn max_speed(&self) -> ByteSpeed {
        ByteSpeed(
            self.write_speed_data
                .iter()
                .map(|x| x.1)
                .fold(0.0, f64::max),
        )
    }

    pub fn make_speed_chart(&self, final_time: Instant) -> Chart {
        let max_speed = self.max_speed();
        let max_time = f64::max(final_time.duration_since(self.start).as_secs_f64(), 3.0);

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
                let y = i as f64 * max_speed.0 / n_y_ticks as f64;
                let bytes = ByteSize::b(y as u64);
                Span::from(format!("{bytes}/s"))
            })
            .collect();

        let bytes_written_dataset = Dataset::default()
            .name("Bytes written")
            .graph_type(GraphType::Scatter)
            .marker(symbols::Marker::Braille)
            .style(Style::default().fg(Color::Green).bg(Color::Green))
            .graph_type(GraphType::Line)
            .data(&self.write_speed_data);

        let chart = Chart::new(vec![bytes_written_dataset])
            .block(Block::default().title("Speed").borders(Borders::ALL))
            .x_axis(
                Axis::default()
                    .bounds([0.0, max_time])
                    .labels(x_ticks)
                    .labels_alignment(Alignment::Right),
            )
            .y_axis(
                Axis::default()
                    .bounds([0.0, max_speed.0])
                    .labels(y_ticks)
                    .labels_alignment(Alignment::Right),
            );

        chart
    }

    pub fn max_bytes(&self) -> ByteSize {
        self.max_bytes
    }
}

pub enum EstimatedTime {
    Known(f64),
    Unknown,
}

impl From<f64> for EstimatedTime {
    fn from(value: f64) -> Self {
        if value.is_finite() {
            Self::Known(value)
        } else {
            Self::Unknown
        }
    }
}

impl Display for EstimatedTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EstimatedTime::Known(x) => write!(f, "{x:.1}s"),
            EstimatedTime::Unknown => write!(f, "[unknown]"),
        }
    }
}
