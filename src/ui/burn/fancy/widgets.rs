use std::time::Instant;

use bytesize::ByteSize;
use ratatui::{
    layout::{Alignment, Constraint, Rect},
    style::{Color, Style},
    symbols,
    text::Span,
    widgets::{Axis, Block, Borders, Cell, Chart, Dataset, Gauge, GraphType, Row, Table},
    Frame,
};

use crate::writer_process::state_tracking::WriterState;

#[derive(Debug, PartialEq, Clone)]
pub struct UIState {
    graph_max_speed: f64,
}

pub fn make_progress_bar(state: &WriterState) -> StateProgressBar {
    match state {
        WriterState::Writing(st) => StateProgressBar {
            bytes_written: st.write_hist.bytes_encountered(),
            label_state: "Burning...",
            style: Style::default().fg(Color::Yellow),
            ratio: st.approximate_ratio(),
            display_total_bytes: st.total_raw_bytes,
        },
        WriterState::Verifying {
            verify_hist,
            total_write_bytes,
            ..
        } => StateProgressBar::from_simple(
            verify_hist.bytes_encountered(),
            *total_write_bytes,
            "Verifying...",
            Style::default().fg(Color::Blue).bg(Color::Yellow),
        ),
        WriterState::Finished {
            write_hist,
            error,
            total_write_bytes,
            ..
        } => StateProgressBar::from_simple(
            write_hist.bytes_encountered(),
            *total_write_bytes,
            if error.is_some() { "Error!" } else { "Done!" },
            if error.is_some() {
                Style::default().fg(Color::White).bg(Color::Red)
            } else {
                Style::default().fg(Color::Green).bg(Color::Black)
            },
        ),
    }
}

impl UIState {
    pub fn draw_speed_chart(
        &mut self,
        state: &WriterState,
        frame: &mut Frame<'_>,
        area: Rect,
        final_time: Instant,
    ) {
        let wdata = state.write_hist();
        let max_time = f64::max(final_time.duration_since(wdata.start()).as_secs_f64(), 3.0);
        let window = max_time / frame.size().width as f64;

        let wspeeds: Vec<(f64, f64)> = wdata.speeds(window).collect();
        let vspeeds: Option<Vec<(f64, f64)>> = state.verify_hist().map(|vdata| {
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

pub struct StateProgressBar {
    bytes_written: u64,
    display_total_bytes: Option<u64>,
    ratio: f64,
    label_state: &'static str,
    style: Style,
}

impl StateProgressBar {
    fn from_simple(bytes_written: u64, max: u64, label_state: &'static str, style: Style) -> Self {
        Self {
            bytes_written,
            display_total_bytes: Some(max),
            ratio: bytes_written as f64 / max as f64,
            label_state,
            style,
        }
    }

    pub fn render(&self) -> Gauge {
        if let Some(max) = self.display_total_bytes {
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

pub fn make_info_table<'a>(
    input_filename: &'a str,
    target_filename: &'a str,
    state: &'a WriterState,
) -> Table<'a> {
    let wdata = state.write_hist();

    let mut rows = vec![
        Row::new([Cell::from("Input"), Cell::from(input_filename)]),
        Row::new([Cell::from("Output"), Cell::from(target_filename)]),
        Row::new([
            Cell::from("Avg. Write"),
            Cell::from(format!("{}", wdata.total_avg_speed())),
        ]),
    ];

    match &state {
        WriterState::Writing(st) => {
            rows.push(Row::new([
                Cell::from("ETA Write"),
                Cell::from(format!("{}", st.eta_write())),
            ]));
        }
        WriterState::Verifying {
            verify_hist: vdata,
            total_write_bytes,
            ..
        } => {
            rows.push(Row::new([
                Cell::from("Avg. Verify"),
                Cell::from(format!("{}", vdata.total_avg_speed())),
            ]));
            rows.push(Row::new([
                Cell::from("ETA verify"),
                Cell::from(format!("{}", vdata.estimated_time_left(*total_write_bytes))),
            ]));
        }
        WriterState::Finished {
            verify_hist: vdata, ..
        } => {
            if let Some(vdata) = vdata {
                rows.push(Row::new([
                    Cell::from("Avg. Verify"),
                    Cell::from(format!("{}", vdata.total_avg_speed())),
                ]));
            }
        }
    }

    Table::new(rows)
        .style(Style::default())
        .widths(&[Constraint::Length(16), Constraint::Percentage(100)])
        .block(Block::default().title("Stats").borders(Borders::ALL))
}
