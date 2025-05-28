use crossterm::event::{Event, EventStream, KeyCode};
use futures::StreamExt;
use ratatui::{
    prelude::*,
    widgets::{StatefulWidget, Widget},
    Terminal,
};
use strum::EnumCount;
use tui_input::Input;
use tui_menu::{Menu, MenuItem, MenuState};

use crate::{
    compression::{self, CompressionFormat},
    hash::HashAlg,
    ui::cli::{self, BurnArgs},
};

pub async fn run_setup(args: BurnArgs, terminal: &mut Terminal<impl Backend>) {
    let mut form = FormState::new(&args);
    let mut events = EventStream::new();
    loop {
        render(&mut form, terminal);
        while let Some(event) = events.next().await {
            let event = event.expect("Failed to get event");
            match event {
                Event::Key(key_event) => match key_event.code {
                    KeyCode::Esc => return,
                    _ => (),
                },
                _ => (),
            }
            render(&mut form, terminal);
        }
    }
}

fn render(form: &mut FormState, terminal: &mut Terminal<impl Backend>) {
    terminal
        .draw(|f| {
            f.render_stateful_widget(Form, f.area(), form);
        })
        .unwrap();
}

#[derive(Debug)]
struct Form;

struct FormState {
    focus: FormFocus,
    data: FormData,
}

impl FormState {
    fn new(args: &BurnArgs) -> Self {
        let data = FormData::new(args);
        let focus = FormFocus::Compression;
        Self { focus, data }
    }
}

struct FormData {
    input_file: String,
    compression: Option<CompressionFormat>,
    hash: HashState,
    target: TargetState,
}

impl FormData {
    fn new(args: &BurnArgs) -> Self {
        let hash = match &args.hash {
            cli::HashArg::Ask => HashState::default(),
            cli::HashArg::Skip => HashState::skip(),
            cli::HashArg::Hash { alg, expected_hash } => {
                HashState::from_provided(base16::encode_lower(&expected_hash).into(), Some(*alg))
            }
        };
        Self {
            input_file: args.input.to_string_lossy().to_string(),
            compression: args.compression.associated_format(),
            hash,
            target: TargetState {
                path: "".to_string(),
            },
        }
    }
}

impl StatefulWidget for Form {
    type State = FormState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State)
    where
        Self: Sized,
    {
        tracing::info!("{:?}", area);
        let (overall_layout, row_layout) = if area.width < 80 {
            (
                Layout::vertical([
                    Constraint::Length(1),
                    Constraint::Length(1),
                    Constraint::Length(1),
                    Constraint::Fill(1),
                    Constraint::Length(1),
                ]),
                Layout::vertical([
                    Constraint::Length(1),
                    Constraint::Length(0),
                    Constraint::Fill(1),
                ]),
            )
        } else {
            (
                Layout::vertical([
                    Constraint::Length(2),
                    Constraint::Length(2),
                    Constraint::Length(2),
                    Constraint::Fill(1),
                    Constraint::Length(1),
                ]),
                Layout::horizontal([
                    Constraint::Length(12),
                    Constraint::Length(1),
                    Constraint::Fill(1),
                ]),
            )
        };
        let [input_filename_area, compression_area, hash_area, target_area, submit_area] =
            overall_layout.areas(area);

        let label_style = Style::new().fg(Color::White).bold();
        let field_unfocused = Style::new().fg(Color::Black).bg(Color::LightBlue);
        let field_focused = Style::new().fg(Color::Black).bg(Color::Magenta);
        let field_disabled = Style::new().fg(Color::White).bg(Color::DarkGray);

        {
            let [label, _margin, input] = row_layout.areas(input_filename_area);
            Text::from("Input").right_aligned().render(label, buf);
            Text::raw(&state.data.input_file)
                .style(field_disabled)
                .render(input, buf);
        }

        {
            let [label, _margin, input] = row_layout.areas(compression_area);
            Text::raw("Compression").right_aligned().render(label, buf);
            Text::raw(format!("{:?}", state.data.compression))
                .style(if state.focus == FormFocus::Compression {
                    field_focused
                } else {
                    field_unfocused
                })
                .render(input, buf);
        }

        {
            let [label, _margin, input] = row_layout.areas(hash_area);
            Text::raw("Hash").right_aligned().render(label, buf);
            Text::raw("[ ] None")
                .style(if state.focus == FormFocus::Hash {
                    field_focused
                } else {
                    field_unfocused
                })
                .render(input, buf); */
        }
    }
}

#[derive(Copy, Debug, Clone, EnumCount, PartialEq, Eq)]
#[repr(u8)]
enum FormFocus {
    Compression = 0,
    Hash,
    Target,
    Submit,
}

impl FormFocus {
    pub fn next(&self) -> Self {
        unsafe { std::mem::transmute((*self as u8 + 1) % Self::COUNT as u8) }
    }

    pub fn prev(&self) -> Self {
        unsafe { std::mem::transmute((*self as u8 + Self::COUNT as u8 - 1) % Self::COUNT as u8) }
    }
}

#[derive(Default)]
struct HashState {
    select: Option<HashSelect>,
    hash: Input,
    alg: Option<HashAlg>,
}

impl HashState {
    fn from_provided(hash: String, alg: Option<HashAlg>) -> Self {
        Self {
            select: Some(HashSelect::Hash),
            hash: hash.into(),
            alg,
        }
    }

    fn skip() -> HashState {
        Self {
            select: Some(HashSelect::Skip),
            hash: "".into(),
            alg: None,
        }
    }
}

enum HashSelect {
    Skip,
    Hash,
}

#[derive(Debug)]
struct TargetState {
    pub path: String,
}
