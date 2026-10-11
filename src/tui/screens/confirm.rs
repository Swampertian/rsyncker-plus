use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};

use crate::tui::mouse::{Hits, Target};
use crate::tui::view::{centered, key_hints};

const WIDTH: u16 = 72;

pub fn render(frame: &mut Frame, title: &str, message: &str, hits: &mut Hits) {
    let text_width = (WIDTH - 2) as usize;
    let text_lines: u16 = message.lines().map(|line| (line.chars().count().max(1)).div_ceil(text_width) as u16).sum();
    let area = centered(frame.area(), WIDTH, text_lines + 4);
    let block = Block::bordered().title(format!(" {title} ")).border_style(Style::new().fg(Color::Yellow));
    let inner = block.inner(area);
    let [body, keys] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);

    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(message).wrap(Wrap { trim: false }), body);
    let items = [("y", "confirmar", Some(Target::Confirm)), ("n/Esc", "cancelar", Some(Target::Reject))];
    frame.render_widget(key_hints(keys, &items, hits), keys);
}
