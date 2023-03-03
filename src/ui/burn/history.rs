use std::{fmt::Display, time::Instant};

use bytesize::ByteSize;
use tui::{
    layout::Alignment,
    style::{Color, Style},
    symbols,
    text::Span,
    widgets::{Axis, Block, Borders, Chart, Dataset, Gauge, GraphType},
};

use crate::ui::utils::ByteSpeed;

use super::display::ChildState;

pub enum History<'a> {
    Burning {
        write: &'a ByteSeries,
    },
    Verifying {
        write: &'a ByteSeries,
        verify: &'a ByteSeries,
    },
    Finished {
        write: &'a ByteSeries,
        verify: Option<&'a ByteSeries>,
        when: Instant,
        error: bool,
    },
}

impl<'a> From<&'a ChildState> for History<'a> {
    fn from(value: &'a ChildState) -> Self {
        match value {
            ChildState::Burning { write_hist, .. } => Self::Burning { write: write_hist },
            ChildState::Verifying {
                write_hist,
                verify_hist,
                ..
            } => Self::Verifying {
                write: write_hist,
                verify: verify_hist,
            },
            ChildState::Finished {
                finish_time,
                error,
                write_hist,
                verify_hist,
            } => Self::Finished {
                write: write_hist,
                verify: verify_hist.as_ref(),
                when: finish_time.clone(),
                error: error.is_some(),
            },
        }
    }
}

impl<'a> History<'a> {
    pub fn make_progress(&self) -> Gauge {
        let (bw, max, label, style) = match self {
            History::Burning { write } => (
                write.bytes_written(),
                write.max_bytes(),
                "Burning...",
                Style::default().fg(Color::Yellow).bg(Color::Black),
            ),
            History::Verifying { write, verify } => (
                verify.bytes_written(),
                verify.max_bytes(),
                "Verifying...",
                Style::default().fg(Color::Blue).bg(Color::Yellow),
            ),
            History::Finished {
                write,
                verify,
                error,
                ..
            } => (
                write.bytes_written(),
                write.max_bytes(),
                if *error { "Error!" } else { "Done!" },
                Style::default().fg(Color::Green).bg(Color::Black),
            ),
        };

        Gauge::default()
            .label(format!("{} {} / {}", label, bw, max))
            .ratio((bw.0 as f64) / (max.0 as f64))
            .gauge_style(style)
    }

    pub fn make_speed_chart(&self, final_time: Instant) -> Chart<'_> {
        let max_speed = self.write.max_speed();
        let max_time = f64::max(
            final_time.duration_since(self.write.start).as_secs_f64(),
            3.0,
        );

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
            .data(&self.write.speed_data);

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

pub struct ByteSeries {
    max_bytes: ByteSize,
    raw: Vec<(f64, ByteSize)>,
    speed_data: Vec<(f64, f64)>,
    start: Instant,
}

impl ByteSeries {
    pub fn new(start: Instant, max_bytes: ByteSize) -> Self {
        Self {
            start,
            max_bytes,
            raw: vec![],
            speed_data: vec![],
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

    pub fn finished_verifying_at(&mut self, time: Instant) {
        self.push(time, self.max_bytes.0);
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
        self.speed_data
            .last()
            .map(|x| x.clone())
            .unwrap_or((0.0, 0.0))
    }

    pub fn last_speed(&self) -> ByteSpeed {
        let (_, s) = self.last_speed_data();
        ByteSpeed(s)
    }

    pub fn max_speed(&self) -> ByteSpeed {
        ByteSpeed(self.speed_data.iter().map(|x| x.1).fold(0.0, f64::max))
    }

    pub fn max_bytes(&self) -> ByteSize {
        self.max_bytes
    }
}
