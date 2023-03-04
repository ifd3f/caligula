use std::time::Instant;

use bytesize::ByteSize;
use tui::{
    backend::Backend,
    layout::{Alignment, Rect},
    style::{Color, Style},
    symbols,
    text::Span,
    widgets::{Axis, Block, Borders, Chart, Dataset, Gauge, GraphType},
    Frame,
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

    pub fn draw_progress(&self, frame: &mut Frame<impl Backend>, area: Rect, final_time: Instant) {
        let (bw, max, label, style) = match self {
            History::Burning { write } => (
                write.bytes_written(),
                write.max_bytes(),
                "Burning...",
                Style::default().fg(Color::Yellow).bg(Color::Black),
            ),
            History::Verifying { verify, .. } => (
                verify.bytes_written(),
                verify.max_bytes(),
                "Verifying...",
                Style::default().fg(Color::Blue).bg(Color::Yellow),
            ),
            History::Finished { write, error, .. } => (
                write.bytes_written(),
                write.max_bytes(),
                if *error { "Error!" } else { "Done!" },
                Style::default().fg(Color::Green).bg(Color::Black),
            ),
        };

        let gauge = Gauge::default()
            .label(format!("{} {} / {}", label, bw, max))
            .ratio((bw.0 as f64) / (max.0 as f64))
            .gauge_style(style);

        frame.render_widget(gauge, area);
    }

    pub fn draw_speed_chart(
        &self,
        frame: &mut Frame<impl Backend>,
        area: Rect,
        final_time: Instant,
    ) {
        let wdata = self.write_data();
        let max_time = f64::max(final_time.duration_since(wdata.start()).as_secs_f64(), 3.0);
        let window = max_time / frame.size().width as f64;

        let wspeeds: Vec<(f64, f64)> = wdata.speeds(window).collect();
        let vspeeds: Option<Vec<(f64, f64)>> = self.verify_data().map(|vdata| {
            vdata
                .speeds(window)
                .into_iter()
                .map(|(x, y)| (x + wdata.last_datapoint().0, y))
                .collect()
        });

        let max_speed = if let Some(vs) = &vspeeds {
            wspeeds
                .iter()
                .chain(vs.iter())
                .map(|x| x.1)
                .fold(0.0, f64::max)
        } else {
            wspeeds.iter().map(|x| x.1).fold(0.0, f64::max)
        };

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

        let mut datasets = vec![Dataset::default()
            .name("Write")
            .graph_type(GraphType::Scatter)
            .marker(symbols::Marker::Braille)
            .style(Style::default().fg(Color::Green))
            .graph_type(GraphType::Line)
            .data(&wspeeds)];

        if let Some(vdata) = &vspeeds {
            datasets.push(
                Dataset::default()
                    .name("Verify")
                    .graph_type(GraphType::Scatter)
                    .marker(symbols::Marker::Braille)
                    .style(Style::default().fg(Color::Blue))
                    .graph_type(GraphType::Line)
                    .data(&vdata),
            );
        }

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
                    .bounds([0.0, max_speed])
                    .labels(y_ticks)
                    .labels_alignment(Alignment::Right),
            );

        frame.render_widget(chart, area);
    }
}
