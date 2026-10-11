use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::Line;
use ratatui::widgets::{Block, Clear, Gauge, Paragraph};

use crate::paths::Paths;
use crate::rsync::{self, Change, Progress};
use crate::state::{Outcome, RunState};
use crate::tui::mouse::{Hits, Target};
use crate::tui::task::{Task, Update};
use crate::tui::view::{centered, format_bytes, key_hints};

const LOG_LIMIT: usize = 500;

#[derive(Debug)]
pub struct RunView {
    pub name: String,
    pub progress: Option<Progress>,
    pub log: Vec<String>,
    pub result: Option<Result<RunState, String>>,
    pub cancelling: bool,
    task: Option<Task>,
}

impl RunView {
    pub fn start(paths: &Paths, name: &str) -> Self {
        Self {
            name: name.to_string(),
            progress: None,
            log: Vec::new(),
            result: None,
            cancelling: false,
            task: Some(Task::run(paths.clone(), name.to_string())),
        }
    }

    pub fn update(&mut self) -> bool {
        let Some(task) = &self.task else {
            return false;
        };
        for update in task.updates() {
            match update {
                Update::Line(line) => self.on_line(&line),
                Update::Finished(result) => {
                    self.result = Some(result);
                    self.task = None;
                    return true;
                }
            }
        }
        false
    }

    fn on_line(&mut self, line: &str) {
        if let Some(progress) = rsync::parse_progress(line) {
            self.progress = Some(progress);
        } else if let Some(item) = rsync::parse_item(line) {
            let mark = match item.change {
                Change::Created => "+",
                Change::Updated => "~",
                Change::Deleted => "−",
            };
            self.log.push(format!("{mark} {}", item.path));
            if self.log.len() > LOG_LIMIT {
                self.log.remove(0);
            }
        }
    }

    pub fn is_running(&self) -> bool {
        self.result.is_none()
    }

    pub fn cancel(&mut self) {
        if let Some(task) = &self.task {
            task.cancel();
            self.cancelling = true;
        }
    }
}

pub fn render(frame: &mut Frame, view: &RunView, hits: &mut Hits) {
    let screen = frame.area();
    let area = centered(screen, screen.width.saturating_sub(4).min(110), screen.height.saturating_sub(2));
    let title = if view.is_running() { format!(" Rodando: {} ", view.name) } else { format!(" Concluído: {} ", view.name) };
    let block = Block::bordered().title(title).border_style(Style::new().fg(Color::Cyan));
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);

    let [gauge, status, log, keys] = Layout::vertical([Constraint::Length(1), Constraint::Length(2), Constraint::Min(1), Constraint::Length(1)]).areas(inner);

    let percent = match (&view.result, &view.progress) {
        (Some(Ok(state)), _) if state.outcome != Outcome::Failed => 100,
        (_, Some(progress)) => progress.percent.min(100),
        _ => 0,
    };
    let label = match &view.progress {
        Some(progress) if view.is_running() => format!("{percent}%  ·  {}  ·  {}  ·  {} restante", format_bytes(progress.bytes), progress.rate, progress.eta),
        _ => format!("{percent}%"),
    };
    frame.render_widget(Gauge::default().percent(u16::from(percent)).label(label).gauge_style(Style::new().fg(Color::Cyan).bg(Color::DarkGray)), gauge);

    frame.render_widget(Paragraph::new(status_lines(view)), status);

    let visible = log.height as usize;
    let lines: Vec<Line> = view.log.iter().skip(view.log.len().saturating_sub(visible)).map(|line| Line::from(format!(" {line}"))).collect();
    frame.render_widget(Paragraph::new(lines).dark_gray(), log);

    let items = if view.is_running() { [("Esc", "cancelar", Some(Target::Cancel))] } else { [("Esc", "fechar", Some(Target::Cancel))] };
    frame.render_widget(key_hints(keys, &items, hits), keys);
}

fn status_lines(view: &RunView) -> Vec<Line<'static>> {
    match &view.result {
        None if view.cancelling => vec![Line::from(" cancelando…").yellow()],
        None => vec![Line::from(format!(" {} item(ns) processado(s)", view.log.len())).dark_gray()],
        Some(Err(error)) => vec![Line::from(format!(" ✗ {error}")).red()],
        Some(Ok(state)) => {
            let totals =
                format!(" {} arquivo(s), {} copiados, {} apagados", state.files_transferred, format_bytes(state.bytes_transferred), state.files_deleted);
            let headline = match state.outcome {
                Outcome::Success => Line::from(" ✓ concluído").green().bold(),
                Outcome::Warning => Line::from(" ! concluído com avisos").yellow().bold(),
                Outcome::Failed => Line::from(format!(" ✗ falhou: {}", state.message.clone().unwrap_or_default())).red().bold(),
            };
            vec![headline, Line::from(totals)]
        }
    }
}
