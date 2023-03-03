use std::time::Instant;

use bytesize::ByteSize;
use tui::{
    layout::Alignment,
    style::{Color, Style},
    symbols,
    text::Span,
    widgets::{Axis, Block, Borders, Chart, Dataset, Gauge, GraphType},
};

use super::{byteseries::ByteSeries, state::ChildState};

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
    pub fn write_data(&self) -> &ByteSeries {
        match self {
            History::Burning { write } => write,
            History::Verifying { write, .. } => write,
            History::Finished { write, .. } => write,
        }
    }

    pub fn verify_data(&self) -> Option<&ByteSeries> {
        match self {
            History::Burning { .. } => None,
            History::Verifying { verify, .. } => Some(verify),
            History::Finished { verify, .. } => verify.as_ref().copied(),
        }
    }

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
        let wdata = self.write_data();

        let max_speed = wdata.max_speed();
        let max_time = f64::max(final_time.duration_since(wdata.start()).as_secs_f64(), 3.0);

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

        let datasets = vec![Dataset::default()
            .name("Bytes written")
            .graph_type(GraphType::Scatter)
            .marker(symbols::Marker::Braille)
            .style(Style::default().fg(Color::Green).bg(Color::Green))
            .graph_type(GraphType::Line)
            .data(&wdata.speed_data())];

        let chart = Chart::new(datasets)
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
