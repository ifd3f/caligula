use crate::device::{Removable, WriteTarget};
use ratatui::{
    prelude::{Buffer, Constraint, Layout, Rect},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, Widget},
};

pub struct OutfileSelector {
    targets: Vec<WriteTarget>,
    show_removables_only: bool,
}

impl OutfileSelector {
    fn make_table(&self) -> Table<'_> {
        let header = Row::new(["Disk", "Model", "Size", "Removable", "Type"]);

        let rows = self.displayed_devices().map(|t| {
            Row::new([
                Cell::from(t.name.clone()),
                Cell::from(t.model.to_string()),
                Cell::from(t.size.to_string()),
                Cell::from(match t.removable {
                    Removable::Yes => "✅️",
                    Removable::No => "❌",
                    Removable::Unknown => "?",
                }),
                Cell::from(t.target_type.to_string()),
            ])
        });

        Table::new([header].into_iter().chain(rows))
    }

    fn help_widget(&self) -> Paragraph<'_> {
        let removables_only_section = if self.show_removables_only {
            "show all devices: [h]"
        } else {
            "only show removable: [h]"
        };

        Paragraph::new(format!("refresh: [r] | {removables_only_section}"))
    }

    pub fn displayed_devices(&self) -> impl Iterator<Item = &WriteTarget> {
        self.targets
            .iter()
            .filter(|d| !self.show_removables_only || d.removable == Removable::Yes)
    }
}

impl Widget for OutfileSelector {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::new().borders(Borders::all()).title("Pick a target");
        let layout = Layout::default()
            .constraints([Constraint::Percentage(100), Constraint::Length(1)])
            .split(block.inner(area));
        let table_area = layout[0];
        let help_area = layout[1];

        block.render(area, buf);
        self.make_table().render(table_area, buf);
        self.help_widget().render(help_area, buf);
    }
}
