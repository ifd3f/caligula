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

#[derive(Debug, PartialEq, Clone)]
pub struct UIState {
    graph_max_speed: f64,
}

pub enum History<'a> {
    Burning {
        max_bytes: Option<u64>,
        input_file_bytes: u64,
        write: &'a ByteSeries,
        read: &'a ByteSeries,
    },
    Verifying {
        max_bytes: u64,
        write: &'a ByteSeries,
        verify: &'a ByteSeries,
    },
    Finished {
        write: &'a ByteSeries,
        verify: Option<&'a ByteSeries>,
        max_bytes: u64,
        when: Instant,
        error: bool,
    },
}

impl<'a> From<&'a ChildState> for History<'a> {
    fn from(value: &'a ChildState) -> Self {
        match value {
            ChildState::Burning {
                write_hist,
                read_hist,
                max_bytes,
                input_file_bytes,
                ..
            } => Self::Burning {
                max_bytes: *max_bytes,
                input_file_bytes: *input_file_bytes,
                write: write_hist,
                read: read_hist,
            },
            ChildState::Verifying {
                max_bytes,
                write_hist,
                verify_hist,
                ..
            } => Self::Verifying {
                max_bytes: *max_bytes,
                write: write_hist,
                verify: verify_hist,
            },
            ChildState::Finished {
                max_bytes,
                finish_time,
                error,
                write_hist,
                verify_hist,
            } => Self::Finished {
                max_bytes: *max_bytes,
                write: write_hist,
                verify: verify_hist.as_ref(),
                when: finish_time.clone(),
                error: error.is_some(),
            },
        }
    }
}

impl History<'_> {
    pub fn write_data(&self) -> &ByteSeries {
        match self {
            History::Burning { write, .. } => write,
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

    pub fn draw_progress(&self, frame: &mut Frame<impl Backend>, area: Rect) {
        let bar = match self {
            History::Burning {
                write,
                read,
                max_bytes,
                input_file_bytes,
                ..
            } => {
                let ratio = match max_bytes {
                    Some(mb) => write.bytes_encountered() as f64 / *mb as f64,
                    None => read.bytes_encountered() as f64 / *input_file_bytes as f64,
                };
                StateProgressBar {
                    bytes_written: write.bytes_encountered(),
                    label_state: "Burning...",
                    style: Style::default().fg(Color::Yellow),
                    ratio,
                    display_max_bytes: *max_bytes,
                }
            }
            History::Verifying {
                verify, max_bytes, ..
            } => StateProgressBar::from_simple(
                verify.bytes_encountered(),
                *max_bytes,
                "Verifying...",
                Style::default().fg(Color::Blue).bg(Color::Yellow),
            ),
            History::Finished {
                write,
                error,
                max_bytes,
                ..
            } => StateProgressBar::from_simple(
                write.bytes_encountered(),
                *max_bytes,
                if *error { "Error!" } else { "Done!" },
                if *error {
                    Style::default().fg(Color::White).bg(Color::Red)
                } else {
                    Style::default().fg(Color::Green).bg(Color::Black)
                },
            ),
        };

        frame.render_widget(bar.render(), area);
    }
}

impl UIState {
    pub fn draw_speed_chart(
        &mut self,
        history: &History,
        frame: &mut Frame<impl Backend>,
        area: Rect,
        final_time: Instant,
    ) {
        let wdata = history.write_data();
        let max_time = f64::max(final_time.duration_since(wdata.start()).as_secs_f64(), 3.0);
        let window = max_time / frame.size().width as f64;

        let wspeeds: Vec<(f64, f64)> = wdata.speeds(window).collect();
        let vspeeds: Option<Vec<(f64, f64)>> = history.verify_data().map(|vdata| {
            vdata
                .speeds(window)
                .into_iter()
                .map(|(x, y)| (x + wdata.last_datapoint().0, y))
                .collect()
        });

        // update max y-axis
        self.graph_max_speed = if let Some(vs) = &vspeeds {
            wspeeds
                .iter()
                .chain(vs.iter())
                .map(|x| x.1)
                .fold(self.graph_max_speed, f64::max)
        } else {
            wspeeds
                .iter()
                .map(|x| x.1)
                .fold(self.graph_max_speed, f64::max)
        };

        let n_x_ticks = (frame.size().width / 16).min(9);
        let n_y_ticks = (frame.size().height / 4).min(5);

        let x_ticks: Vec<_> = (0..=n_x_ticks)
            .map(|i| {
                let x = i as f64 * max_time / n_x_ticks as f64;
                Span::from(format!("{x:.1}s"))
            })
            .collect();

        let y_ticks: Vec<_> = (0..=n_y_ticks)
            .map(|i| {
                let y = i as f64 * self.graph_max_speed / n_y_ticks as f64;
                let bytes = ByteSize::b(y as u64);
                Span::from(format!("{bytes}/s"))
            })
            .collect();

        let mut datasets = vec![Dataset::default()
            .name("Write")
            .graph_type(GraphType::Scatter)
            .marker(symbols::Marker::Braille)
            .style(Style::default().fg(Color::Yellow))
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
                    .bounds([0.0, self.graph_max_speed])
                    .labels(y_ticks)
                    .labels_alignment(Alignment::Right),
            );

        frame.render_widget(chart, area);
    }
}

struct StateProgressBar {
    bytes_written: u64,
    display_max_bytes: Option<u64>,
    ratio: f64,
    label_state: &'static str,
    style: Style,
}

impl StateProgressBar {
    fn from_simple(bytes_written: u64, max: u64, label_state: &'static str, style: Style) -> Self {
        Self {
            bytes_written,
            display_max_bytes: Some(max),
            ratio: bytes_written as f64 / max as f64,
            label_state,
            style,
        }
    }

    fn render(&self) -> Gauge {
        if let Some(max) = self.display_max_bytes {
            Gauge::default()
                .label(format!(
                    "{} {} / {}",
                    self.label_state,
                    ByteSize::b(self.bytes_written),
                    ByteSize::b(max)
                ))
                .ratio(self.ratio)
                .gauge_style(self.style)
        } else {
            Gauge::default()
                .label(format!(
                    "{} {} / ???",
                    self.label_state,
                    ByteSize::b(self.bytes_written),
                ))
                .ratio(self.ratio)
                .gauge_style(self.style)
        }
    }
}

impl Default for UIState {
    fn default() -> Self {
        Self {
            graph_max_speed: 0.0,
        }
    }
}
