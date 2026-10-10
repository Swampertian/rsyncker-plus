use ratatui::layout::Rect;
use ratatui::style::{Color, Style};

use crate::job::{Mode, Schedule};
use crate::state::Outcome;

pub fn mode_label(mode: Mode) -> &'static str {
    match mode {
        Mode::Copy => "cópia",
        Mode::Mirror => "espelho",
    }
}

pub fn schedule_label(schedule: Schedule) -> &'static str {
    match schedule {
        Schedule::None => "manual",
        Schedule::Hourly => "a cada hora",
        Schedule::Daily => "diário",
        Schedule::Weekly => "semanal",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Running,
    DestinationMissing,
    Never,
    Last(Outcome),
}

pub fn status_label(status: Status) -> (&'static str, Style) {
    match status {
        Status::Running => ("⟳ rodando", Style::new().fg(Color::Cyan)),
        Status::DestinationMissing => ("⚠ sem destino", Style::new().fg(Color::Red)),
        Status::Never => ("· nunca", Style::new().fg(Color::DarkGray)),
        Status::Last(Outcome::Success) => ("✓ ok", Style::new().fg(Color::Green)),
        Status::Last(Outcome::Warning) => ("! aviso", Style::new().fg(Color::Yellow)),
        Status::Last(Outcome::Failed) => ("✗ erro", Style::new().fg(Color::Red)),
    }
}

pub fn short_next_run(raw: &str) -> String {
    let parts: Vec<&str> = raw.split_whitespace().collect();
    match parts.as_slice() {
        [_, date, time, ..] if time.len() >= 5 => format!("{date} {}", &time[..5]),
        _ => raw.to_string(),
    }
}

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{bytes} B") } else { format!("{value:.1} {}", UNITS[unit]) }
}

pub fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect { x: area.x + (area.width - width) / 2, y: area.y + (area.height - height) / 2, width, height }
}
