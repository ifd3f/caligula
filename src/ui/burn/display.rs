use std::{sync::Arc, time::Instant};

use bytesize::ByteSize;
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures::StreamExt;
use tokio::{select, time};
use tracing::{debug, info, trace};
use tui::{
    backend::Backend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Cell, Gauge, Row, Table},
    Terminal,
};

use crate::{
    burn::{self, ipc::StatusMessage, Handle},
    cli::Args,
    device::BurnTarget,
};

use super::history::{ByteSeries, History};

pub struct UI<'a, B>
where
    B: Backend,
{
    terminal: &'a mut Terminal<B>,
    events: EventStream,
    state: State,
}

impl<'a, B> UI<'a, B>
where
    B: Backend,
{
    pub fn new(
        handle: burn::Handle,
        terminal: &'a mut Terminal<B>,
        target: BurnTarget,
        args: &'a Args,
    ) -> Self {
        Self {
            terminal,
            events: EventStream::new(),
            state: State {
                input_filename: args.input.to_string_lossy().to_string(),
                target_filename: target.devnode.to_string_lossy().to_string(),
                child: ChildState::Burning {
                    handle,
                    write_hist: ByteSeries::new(
                        Instant::now(),
                        ByteSize::b(handle.initial_info().input_file_bytes),
                    ),
                },
            },
        }
    }

    pub async fn show(mut self) -> anyhow::Result<()> {
        loop {
            match self.get_and_handle_events().await {
                Ok(s) => self = s,
                Err(e) => match e.downcast::<Quit>()? {
                    Quit => break,
                },
            }
        }
        Ok(())
    }

    async fn get_and_handle_events(mut self) -> anyhow::Result<UI<'a, B>> {
        let msg = {
            let handle = self.state.child.child_process();
            if let Some(handle) = handle {
                child_active(&mut self.events, handle).await
            } else {
                child_dead(&mut self.events).await
            }?
        };
        self.state = self.state.on_event(msg)?;
        self.state.draw(&mut self.terminal)?;
        Ok(self)
    }
}

async fn child_dead(events: &mut EventStream) -> anyhow::Result<UIEvent> {
    Ok(UIEvent::TermEvent(events.next().await.unwrap()?))
}

async fn child_active(events: &mut EventStream, handle: &mut Handle) -> anyhow::Result<UIEvent> {
    let sleep = tokio::time::sleep(time::Duration::from_millis(250));
    select! {
        _ = sleep => {
            return Ok(UIEvent::Sleep);
        }
        msg = handle.next_message() => {
            return Ok(UIEvent::Child(Instant::now(), msg?));
        }
        event = events.next() => {
            return Ok(UIEvent::TermEvent(event.unwrap()?));
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
enum UIEvent {
    Sleep,
    Child(Instant, Option<StatusMessage>),
    TermEvent(Event),
}

struct State {
    input_filename: String,
    target_filename: String,
    child: ChildState,
}

pub enum ChildState {
    Burning {
        handle: Handle,
        write_hist: ByteSeries,
    },
    Verifying {
        handle: Handle,
        write_hist: ByteSeries,
        verify_hist: ByteSeries,
    },
    Finished {
        finish_time: Instant,
        error: Option<String>,
        write_hist: ByteSeries,
        verify_hist: Option<ByteSeries>,
    },
}

impl State {
    fn on_event(self, ev: UIEvent) -> anyhow::Result<Self> {
        trace!("Handling {ev:?}");

        Ok(match ev {
            UIEvent::Sleep => self,
            UIEvent::Child(t, m) => self.on_child_status(t, m),
            UIEvent::TermEvent(e) => self.on_term_event(e)?,
        })
    }

    fn on_term_event(self, ev: Event) -> anyhow::Result<Self> {
        match ev {
            Event::Key(KeyEvent {
                code: KeyCode::Char('c'),
                modifiers: KeyModifiers::CONTROL,
                kind: KeyEventKind::Press,
                ..
            }) => {
                info!("Got CTRL-C, quitting");
                Err(Quit)?
            }
            _ => Ok(self),
        }
    }

    fn on_child_status(mut self, now: Instant, msg: Option<StatusMessage>) -> Self {
        match msg {
            Some(StatusMessage::TotalBytes(b)) => {
                match &self.child {
                    ChildState::Burning { .. } => {
                        self.history.push_writing(now, b as u64);
                    }
                    ChildState::Verifying { .. } => {
                        self.history.push_verifying(now, b as u64);
                    }
                    _ => {}
                }
                self
            }
            Some(StatusMessage::FinishedWriting { verifying }) => {
                debug!(verifying, "Got FinishedWriting");
                let child = match self.child {
                    ChildState::Burning { handle, write_hist } => {
                        write_hist.finished_verifying_at(now);
                        if verifying {
                            info!(verifying, "Transition to verifying");
                            ChildState::Verifying {
                                handle,
                                write_hist,
                                verify_hist: ByteSeries::new(
                                    now,
                                    ByteSize::b(handle.initial_info().input_file_bytes),
                                ),
                            }
                        } else {
                            info!(verifying, "Transition to finished");
                            ChildState::Finished {
                                finish_time: now,
                                error: None,
                                write_hist,
                                verify_hist: None,
                            }
                        }
                    }
                    c => c,
                };
                Self { child, ..self }
            }
            None => Self {
                child: self.child.into_finished(now, None),
                ..self
            },
            _ => self,
        }
    }

    fn draw(&self, terminal: &mut Terminal<impl tui::backend::Backend>) -> anyhow::Result<()> {
        let history = self.child.history();

        let final_time = match self.child {
            ChildState::Finished { finish_time, .. } => finish_time,
            _ => Instant::now(),
        };

        let progress = {
            let bw = history.bytes_written();
            let max = history.max_bytes();
            Gauge::default()
                .label(format!("{} {} / {}", self.child.bar_text(), bw, max))
                .ratio((bw.0 as f64) / (max.0 as f64))
                .gauge_style(self.child.bar_style())
        };

        let chart = history.make_speed_chart(final_time);

        let info_table = Table::new(vec![
            Row::new([
                Cell::from("Input"),
                Cell::from(self.input_filename.as_str()),
            ]),
            Row::new([
                Cell::from("Output"),
                Cell::from(self.target_filename.as_str()),
            ]),
            Row::new([
                Cell::from("Total Speed"),
                Cell::from(format!(
                    "{}",
                    self.child.history().write.total_avg_speed(final_time)
                )),
            ]),
            Row::new([
                Cell::from("Current Speed"),
                Cell::from(format!("{}", self.child.history().write.last_speed())),
            ]),
            Row::new([
                Cell::from("ETA"),
                Cell::from(format!(
                    "{}",
                    self.child.history().write.estimated_time_left(final_time)
                )),
            ]),
        ])
        .style(Style::default())
        .widths(&[Constraint::Length(16), Constraint::Percentage(100)])
        .block(Block::default().title("Stats").borders(Borders::ALL));

        terminal.draw(|f| {
            let layout = ComputedLayout::from(f.size());

            f.render_widget(progress, layout.progress);
            f.render_widget(chart, layout.graph);
            f.render_widget(info_table, layout.args_display);
        })?;
        Ok(())
    }
}

impl ChildState {
    fn child_process(&mut self) -> Option<&mut Handle> {
        match self {
            Self::Burning { handle, .. } => Some(handle),
            Self::Verifying { handle, .. } => Some(handle),
            Self::Finished { .. } => None,
        }
    }

    fn into_finished(self, now: Instant, error: Option<String>) -> ChildState {
        match self {
            ChildState::Burning { handle, write_hist } => ChildState::Finished {
                finish_time: now,
                error,
                write_hist,
                verify_hist: None,
            },
            ChildState::Verifying {
                handle,
                write_hist,
                verify_hist,
            } => ChildState::Finished {
                finish_time: now,
                error,
                write_hist,
                verify_hist: Some(verify_hist),
            },
            fin => fin,
        }
    }
}

struct ComputedLayout {
    progress: Rect,
    graph: Rect,
    args_display: Rect,
}

impl From<Rect> for ComputedLayout {
    fn from(value: Rect) -> Self {
        let root = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(10),
                Constraint::Length(10),
            ])
            .split(value);

        let info_pane = root[2];

        Self {
            graph: root[1],
            progress: root[0],
            args_display: info_pane,
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("User sent quit signal")]
struct Quit;
