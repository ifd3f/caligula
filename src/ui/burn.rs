use tokio::{select, time};
use tracing::debug;
use tui::{
    widgets::{Block, Borders},
    Terminal,
};

use crate::{
    burn::{self, ipc::StatusMessage, Handle},
    cli::Args,
};

pub struct BurningDisplay<'a, B>
where
    B: tui::backend::Backend,
{
    args: &'a Args,
    terminal: &'a mut Terminal<B>,
    state: State,
}

impl<'a, B> BurningDisplay<'a, B>
where
    B: tui::backend::Backend,
{
    pub fn new(handle: burn::Handle, args: &'a Args, terminal: &'a mut Terminal<B>) -> Self {
        Self {
            state: State::Burning(BurningState { handle }),
            args,
            terminal,
        }
    }

    pub async fn show(&mut self) -> anyhow::Result<()> {
        let mut interval = time::interval(time::Duration::from_secs(1));

        loop {
            match &mut self.state {
                State::Burning(s) => select! {
                    _ = interval.tick() => {}
                    msg = s.handle.next_message() => {
                        debug!(msg = format!("{msg:?}"), "Got message");

                        if let Some(m) = msg? {
                            self.on_message(m).await
                        } else {
                            self.state = State::Complete;
                        }
                    }
                },
                State::Complete => {}
            }

            self.draw()?;
        }
    }

    async fn on_message(&mut self, msg: StatusMessage) {}

    fn draw(&mut self) -> anyhow::Result<()> {
        self.terminal.draw(|f| {
            let size = f.size();
            let block = Block::default().title("Block").borders(Borders::ALL);
            f.render_widget(block, size);
        })?;
        Ok(())
    }
}

enum State {
    Burning(BurningState),
    Complete,
}

struct BurningState {
    handle: Handle,
}
