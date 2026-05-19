use std::time::Instant;

use ratatui::{
    Terminal,
    layout::{Constraint, Direction, Layout, Rect},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use super::{
    state::State,
    widgets::{SpeedChart, WriterProgressBar, WritingInfoTable},
};
use crate::{facade::WVState, logging::LogPaths};

struct ComputedLayout {
    progress: Rect,
    graph: Rect,
    args_display: Rect,
    quit_modal: Rect,
}

impl From<Rect> for ComputedLayout {
    fn from(area: Rect) -> Self {
        let root = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(10),
                Constraint::Length(10),
            ])
            .split(area);

        let info_pane = root[2];

        let quit_modal = centered_rect(area, 40, 4);

        Self {
            graph: root[1],
            progress: root[0],
            args_display: info_pane,
            quit_modal,
        }
    }
}

/// Given an outer rect and desired inner rect dimensions, returns the inner
/// rect.
fn centered_rect(r: Rect, w: u16, h: u16) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(h),
            Constraint::Fill(1),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(w),
            Constraint::Fill(1),
        ])
        .split(popup_layout[1])[1]
}

/// Draw the TUI.
pub fn draw(
    state: &mut State,
    child: &WVState,
    terminal: &mut Terminal<impl ratatui::backend::Backend>,
    log_paths: &LogPaths,
) -> std::io::Result<()> {
    terminal.autoresize()?;

    let progress_bar = WriterProgressBar::from_writer(child);

    let final_time = match child {
        WVState::Finished { finish_time, .. } => *finish_time,
        _ => Instant::now(),
    };

    let error = match &child {
        WVState::Finished { result: error, .. } => error.as_ref().err(),
        _ => None,
    };

    let info_table = WritingInfoTable {
        input_filename: &state.input_filename,
        target_filename: &state.target_filename,
        state: child,
    };

    let speed_chart = SpeedChart {
        state: child,
        final_time,
    };

    terminal.draw(|f| {
        let layout = ComputedLayout::from(f.size());

        f.render_widget(progress_bar.render(), layout.progress);
        f.render_stateful_widget(speed_chart, layout.graph, &mut state.graph_state);

        if let Some(error) = error {
            f.render_widget(
                Paragraph::new(format!("{error}\n{}", log_paths.get_bug_report_msg()))
                    .block(
                        Block::default()
                            .title("!!! ERROR !!!")
                            .borders(Borders::ALL),
                    )
                    .wrap(Wrap { trim: true }),
                layout.args_display,
            )
        } else {
            f.render_widget(info_table, layout.args_display);
        }

        if let Some(qm) = state.quit_modal {
            f.render_widget(qm, layout.quit_modal)
        }
    })?;

    Ok(())
}
