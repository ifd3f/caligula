use std::time::Instant;

use bytesize::ByteSize;
use crossterm::event::KeyCode;
use ratatui::{
    layout::{Alignment, Constraint, Rect},
    style::{Color, Style, Stylize},
    symbols,
    text::Span,
    widgets::{
        Axis, Block, BorderType, Borders, Cell, Chart, Clear, Dataset, Gauge, GraphType, Paragraph,
        Row, StatefulWidget, Table, Widget,
    },
};

use crate::ui::writer_tracking::WriterState;

pub struct SpeedChart<'a> {
    pub state: &'a WriterState,
    pub final_time: Instant,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct SpeedChartState {
    /// Due to sample aliasing, the maximum displayed Y value might increase or
    /// decrease. This keeps track of the maximum Y value ever observed, to prevent
    /// the chart limits from rapidly changing from the aliasing.
    max_y_limit: f64,
}

impl StatefulWidget for SpeedChart<'_> {
    type State = SpeedChartState;

    fn render(self, area: Rect, buf: &mut ratatui::prelude::Buffer, state: &mut Self::State) {
        let write_data = self.state.write_hist();
        let max_time = f64::max(
            self.final_time
                .duration_since(write_data.start())
                .as_secs_f64(),
            3.0,
        );
        let window = max_time / area.width as f64;

        let write_speeds: Vec<(f64, f64)> = write_data.speeds(window).collect();
        let verify_speeds: Option<Vec<(f64, f64)>> = self.state.verify_hist().map(|verify_data| {
            verify_data
                .speeds(window)
                .map(|(x, y)| (x + write_data.last_datapoint().0, y))
                .collect()
        });

        // update max y-axis
        state.max_y_limit = write_speeds
            .iter()
            .chain(verify_speeds.iter().flatten())
            .map(|&(_x, y)| y)
            .fold(state.max_y_limit, f64::max);

        // Calculate ticks
        let (x_ticks, y_ticks) = calculate_ticks(area, max_time, state.max_y_limit);

        // Generate datasets
        let dataset_style = Dataset::default()
            .marker(symbols::Marker::Braille)
            .graph_type(GraphType::Line);

        let mut datasets = vec![dataset_style
            .clone()
            .name("Write")
            .style(Style::default().fg(Color::Yellow))
            .data(&write_speeds)];

        if let Some(vdata) = &verify_speeds {
            datasets.push(
                dataset_style
                    .name("Verify")
                    .style(Style::default().fg(Color::Blue))
                    .data(vdata),
            );
        }

        // Finally, build the chart!
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
                    .bounds([0.0, state.max_y_limit])
                    .labels(y_ticks)
                    .labels_alignment(Alignment::Right),
            );

        chart.render(area, buf)
    }
}

fn calculate_ticks(
    area: Rect,
    max_time: f64,
    highest_value: f64,
) -> (Vec<Span<'static>>, Vec<Span<'static>>) {
    let n_x_ticks = (area.width / 16).min(9);
    let n_y_ticks = (area.height / 4).min(5);
    let x_ticks: Vec<_> = (0..=n_x_ticks)
        .map(|i| {
            let x = i as f64 * max_time / n_x_ticks as f64;
            Span::from(format!("{x:.1}s"))
        })
        .collect();
    let y_ticks: Vec<_> = (0..=n_y_ticks)
        .map(|i| {
            let y = i as f64 * highest_value / n_y_ticks as f64;
            let bytes = ByteSize::b(y as u64);
            Span::from(format!("{bytes}/s"))
        })
        .collect();
    (x_ticks, y_ticks)
}

pub struct WriterProgressBar {
    bytes_written: u64,
    display_total_bytes: Option<u64>,
    ratio: f64,
    label_state: &'static str,
    style: Style,
}

impl WriterProgressBar {
    pub fn from_writer(state: &WriterState) -> WriterProgressBar {
        match state {
            WriterState::Writing(st) => WriterProgressBar {
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
            } => WriterProgressBar::from_simple(
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
            } => WriterProgressBar::from_simple(
                write_hist.bytes_encountered(),
                *total_write_bytes,
                if error.is_some() {
                    "Error!"
                } else {
                    "Done! Press q to quit."
                },
                if error.is_some() {
                    Style::default().fg(Color::White).bg(Color::Red)
                } else {
                    Style::default().fg(Color::Green).bg(Color::Black)
                },
            ),
        }
    }

    fn from_simple(bytes_written: u64, max: u64, label_state: &'static str, style: Style) -> Self {
        Self {
            bytes_written,
            display_total_bytes: Some(max),
            ratio: bytes_written as f64 / max as f64,
            label_state,
            style,
        }
    }

    /// This function clamps the ratio to [0, 1].
    ///
    /// Unfortunately, it is sometimes outside of [0, 1]. The most common example is when
    /// we write a non-block-aligned file, in which case bytes_written > max because we
    /// compensate by writing the partial block.
    pub fn ratio(&self) -> f64 {
        self.ratio.clamp(0.0, 1.0)
    }

    pub fn render(&self) -> Gauge {
        if let Some(max) = self.display_total_bytes {
            Gauge::default()
                .label(format!(
                    "{} {} / {} ({:.1} %)",
                    self.label_state,
                    ByteSize::b(self.bytes_written),
                    ByteSize::b(max),
                    self.ratio() * 100.0
                ))
                .ratio(self.ratio())
                .gauge_style(self.style)
        } else {
            Gauge::default()
                .label(format!(
                    "{} {} / ???",
                    self.label_state,
                    ByteSize::b(self.bytes_written),
                ))
                .ratio(self.ratio())
                .gauge_style(self.style)
        }
    }
}

pub struct WritingInfoTable<'a> {
    pub input_filename: &'a str,
    pub target_filename: &'a str,
    pub state: &'a WriterState,
}

impl WritingInfoTable<'_> {
    fn make_info_table(&self) -> Table<'_> {
        let wdata = self.state.write_hist();

        let mut rows = vec![
            Row::new([Cell::from("Input"), Cell::from(self.input_filename)]),
            Row::new([Cell::from("Output"), Cell::from(self.target_filename)]),
            Row::new([
                Cell::from("Avg. Write"),
                Cell::from(format!("{}", wdata.total_avg_speed())),
            ]),
        ];

        match &self.state {
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

        Table::new(rows, [Constraint::Length(16), Constraint::Percentage(100)])
            .style(Style::default())
            .block(Block::default().title("Stats").borders(Borders::ALL))
    }
}

impl Widget for WritingInfoTable<'_> {
    fn render(self, area: Rect, buf: &mut ratatui::prelude::Buffer) {
        Widget::render(self.make_info_table(), area, buf)
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct QuitModal {
    _private: (),
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum QuitModalResult {
    /// Quit the program.
    Quit,

    /// Stay in the program.
    Stay,
}

impl QuitModal {
    pub fn new() -> Self {
        Self { _private: () }
    }

    /// Handle a key down event. If this would conclude the modal, returns the result.
    /// Otherwise, if an indecisive keystroke was detected and we are to stay inside the
    /// modal, returns None.
    pub fn handle_key_down(self, kc: KeyCode) -> Option<QuitModalResult> {
        use KeyCode::*;
        use QuitModalResult::*;
        match kc {
            Esc => Some(Stay),
            Char('q') | Char('Q') => Some(Quit),
            _ => None,
        }
    }
}

impl Widget for QuitModal {
    fn render(self, area: Rect, buf: &mut ratatui::prelude::Buffer)
    where
        Self: Sized,
    {
        let prompt =
            Paragraph::new("Are you sure you want to quit?\nPress q again to quit, ESC to stay")
                .alignment(Alignment::Center)
                .style(Style::new().yellow())
                .block(
                    Block::new()
                        .bg(Color::Red)
                        .border_style(Style::new().white())
                        .border_type(BorderType::Plain)
                        .borders(Borders::ALL),
                );

        Clear.render(area, buf);
        prompt.render(area, buf);
    }
}
