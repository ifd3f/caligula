use tokio::time;
use tui::{
    widgets::{Block, Borders},
    Terminal,
};

use crate::{burn::Writing, cli::Args};

pub struct BurningDisplay<'a, B>
where
    B: tui::backend::Backend,
{
    writing: Writing,
    args: &'a Args,
    terminal: &'a mut Terminal<B>,
}

impl<'a, B> BurningDisplay<'a, B>
where
    B: tui::backend::Backend,
{
    pub fn new(writing: Writing, args: &'a Args, terminal: &'a mut Terminal<B>) -> Self {
        Self {
            writing,
            args,
            terminal,
        }
    }

    pub async fn show(&mut self) -> anyhow::Result<()> {
        let mut interval = time::interval(time::Duration::from_secs(1));

        interval.tick().await;

        self.terminal.draw(|f| {
            let size = f.size();

            let size = f.size();
            let block = Block::default().title("Block").borders(Borders::ALL);
            f.render_widget(block, size);
        })?;

        todo!()
    }
}
