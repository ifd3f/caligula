use crossterm::event::EventStream;
use futures::StreamExt;
use ratatui::{
    prelude::*,
    widgets::{StatefulWidget, Widget},
    Terminal,
};
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
        terminal
            .draw(|f| {
                f.render_stateful_widget(Form, f.size(), &mut form);
            })
            .unwrap();
        while let Some(event) = events.next().await {}
    }
}

fn render(form: &mut FormData, terminal: &mut Terminal<impl Backend>) {}

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
    compression: MenuState<CompressionFormat>,
    hash: HashState,
    target: TargetState,
}

impl FormData {
    fn new(args: &BurnArgs) -> Self {
        let compression = MenuState::new(
            compression::AVAILABLE_FORMATS
                .iter()
                .map(|c| MenuItem::item(format!("{c}"), c.clone()))
                .collect(),
        );
        let hash = match &args.hash {
            cli::HashArg::Ask => HashState::Unspecified,
            cli::HashArg::Skip => HashState::Skip,
            cli::HashArg::Hash { alg, expected_hash } => HashState::Hash {
                hash: base16::encode_lower(&expected_hash).into(),
                alg: Some(*alg),
            },
        };
        Self {
            input_file: args.input.to_string_lossy().to_string(),
            compression,
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
        let (overall_layout, row_layout) = if area.width < 18 {
            (
                Layout::vertical([
                    Constraint::Length(1),
                    Constraint::Length(1),
                    Constraint::Length(1),
                    Constraint::Fill(1),
                    Constraint::Length(1),
                ])
                .margin(1),
                Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]),
            )
        } else {
            (
                Layout::vertical([
                    Constraint::Length(2),
                    Constraint::Length(2),
                    Constraint::Length(2),
                    Constraint::Fill(1),
                    Constraint::Length(1),
                ])
                .margin(1),
                Layout::horizontal([Constraint::Length(12), Constraint::Fill(1)]).margin(1),
            )
        };
        let [input_filename_area, compression_area, hash_area, target_area, submit_area] =
            overall_layout.areas(area);

        {
            let [label, text] = row_layout.areas(input_filename_area);
            Text::raw("Input").right_aligned().render(label, buf);
            Text::raw(&state.data.input_file).render(text, buf);
        }

        {
            let [label, input] = row_layout.areas(compression_area);
            Text::raw("Compression").right_aligned().render(label, buf);
            let menu = Menu::<CompressionFormat>::new();
            StatefulWidget::render(menu, area, buf, &mut state.data.compression);
        }

        /*
        Text::raw("Compression").right_aligned();
        Text::raw("Hash").right_aligned(); */
    }
}

#[derive(Debug)]
enum FormFocus {
    Compression,
    NoHash,
    Hash,
    HashAlg,
    Target,
    Submit,
}

enum HashState {
    Unspecified,
    Skip,
    Hash { hash: Input, alg: Option<HashAlg> },
}

#[derive(Debug)]
struct TargetState {
    pub path: String,
}
