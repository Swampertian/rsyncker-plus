use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::hostkey::{self, Unknown};
use crate::job::{Job, Mode};
use crate::paths::shorten_home;
use crate::rsync::{self, Change, Item};
use crate::state::{Outcome, RunState};
use crate::tui::mouse::{Hits, Target};
use crate::tui::screens::job_form::JobForm;
use crate::tui::task::{Task, Update};
use crate::tui::view::{centered, key_hints};

#[derive(Debug)]
pub struct Preview {
    pub job: Job,
    pub name: String,
    pub source: String,
    pub destination: String,
    pub mirror: bool,
    pub items: Vec<Item>,
    pub result: Option<Result<RunState, String>>,
    pub scroll: usize,
    pub save: Option<Box<JobForm>>,
    pub host: Option<Unknown>,
    task: Option<Task>,
}

impl Preview {
    pub fn start(job: &Job, save: Option<Box<JobForm>>) -> Self {
        Self {
            job: job.clone(),
            name: job.name.clone(),
            source: job.source_label(),
            destination: job.destination_label(),
            mirror: job.mode == Mode::Mirror,
            items: Vec::new(),
            result: None,
            scroll: 0,
            save,
            host: None,
            task: Some(Task::preview(job.clone())),
        }
    }

    pub fn trust_host(self) -> Result<Self, (Self, String)> {
        let Some(unknown) = &self.host else {
            return Ok(self);
        };
        if let Err(error) = hostkey::trust(unknown) {
            let message = format!("{error:#}");
            return Err((self, message));
        }
        Ok(Self::start(&self.job, self.save))
    }

    pub fn update(&mut self) {
        let Some(task) = &self.task else {
            return;
        };
        for update in task.updates() {
            match update {
                Update::Line(line) => self.items.extend(rsync::parse_item(&line)),
                Update::UnknownHost(unknown) => self.host = Some(unknown),
                Update::Finished(result) => {
                    self.result = Some(result);
                    self.task = None;
                }
            }
        }
    }

    pub fn is_done(&self) -> bool {
        self.result.is_some()
    }

    pub fn can_save(&self) -> bool {
        self.save.is_some() && matches!(self.result, Some(Ok(_)))
    }

    pub fn error(&self) -> Option<String> {
        match &self.result {
            Some(Err(error)) => Some(error.clone()),
            Some(Ok(state)) if state.outcome != Outcome::Success => Some(state.message.clone().unwrap_or_default()),
            _ => None,
        }
    }

    pub fn count(&self, change: Change) -> usize {
        self.items.iter().filter(|item| item.change == change).count()
    }

    pub fn scroll_by(&mut self, delta: isize) {
        let max = self.items.len().saturating_sub(1) as isize;
        self.scroll = (self.scroll as isize + delta).clamp(0, max.max(0)) as usize;
    }

    pub fn cancel(&mut self) {
        if let Some(task) = self.task.take() {
            task.cancel();
        }
    }
}

pub fn render(frame: &mut Frame, preview: &Preview, hits: &mut Hits) {
    let screen = frame.area();
    let area = centered(screen, screen.width.saturating_sub(4).min(110), screen.height.saturating_sub(2));
    let color = if preview.mirror { Color::Yellow } else { Color::Cyan };
    let block = Block::bordered().title(format!(" Prévia: {} ", preview.name)).border_style(Style::new().fg(color));
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);

    let [paths, summary, warning, list, error, keys] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    frame.render_widget(Line::from(format!(" {} → {}", preview.source, preview.destination)).dark_gray(), paths);

    if let Some(unknown) = &preview.host {
        render_host(frame, area, unknown, hits);
        return;
    }

    let summary_line = if preview.is_done() {
        Line::from(vec![
            " ".into(),
            format!("{} novo(s)", preview.count(Change::Created)).green(),
            "   ".into(),
            format!("{} alterado(s)", preview.count(Change::Updated)).yellow(),
            "   ".into(),
            format!("{} apagado(s)", preview.count(Change::Deleted)).red(),
        ])
    } else {
        Line::from(format!(" calculando… {} item(ns) até agora", preview.items.len())).cyan()
    };
    frame.render_widget(summary_line, summary);

    if preview.mirror {
        frame.render_widget(Line::from(" ⚠ espelho: itens marcados com − serão APAGADOS no destino").yellow().bold(), warning);
    }

    let visible = list.height as usize;
    let lines: Vec<Line> = if preview.items.is_empty() && preview.is_done() && preview.error().is_none() {
        vec![Line::from(" nada a fazer: o destino já está em dia").dark_gray()]
    } else {
        preview.items.iter().skip(preview.scroll).take(visible).map(item_line).collect()
    };
    frame.render_widget(Paragraph::new(lines), list);

    if let Some(message) = preview.error() {
        frame.render_widget(Line::from(format!(" ✗ {message}")).red(), error);
    }

    let mut items = vec![("↑↓", "rolar", None)];
    if preview.can_save() {
        items.push(("y", "salvar", Some(Target::Confirm)));
    }
    items.push(("Esc", if preview.save.is_some() { "voltar" } else { "fechar" }, Some(Target::Reject)));
    frame.render_widget(key_hints(keys, &items, hits), keys);
}

fn render_host(frame: &mut Frame, outer: Rect, unknown: &Unknown, hits: &mut Hits) {
    let height = 11 + unknown.fingerprints.len() as u16;
    let area = centered(outer, 78, height);
    let block = Block::bordered().title(" Host desconhecido ").border_style(Style::new().fg(Color::Yellow));
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);

    let mut lines =
        vec![Line::from(format!(" Primeira conexão com {}.", unknown.host)).bold(), Line::default(), Line::from(" Impressão digital da chave do servidor:")];
    lines.extend(unknown.fingerprints.iter().map(|f| Line::from(format!("   {f}")).cyan().bold()));
    lines.extend([
        Line::default(),
        Line::from(" Confira no próprio servidor antes de confiar:").dark_gray(),
        Line::from("   ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub").dark_gray(),
        Line::from(format!(" Ao confiar, a chave é gravada em {}.", shorten_home(&unknown.file))).dark_gray(),
    ]);
    frame.render_widget(Paragraph::new(lines), inner);

    let keys = Rect { y: inner.y + inner.height.saturating_sub(1), height: 1, ..inner };
    let items = [("y", "confiar e continuar", Some(Target::Confirm)), ("Esc", "voltar", Some(Target::Reject))];
    frame.render_widget(key_hints(keys, &items, hits), keys);
}

fn item_line(item: &Item) -> Line<'static> {
    let (mark, color) = match item.change {
        Change::Created => ("+", Color::Green),
        Change::Updated => ("~", Color::Yellow),
        Change::Deleted => ("−", Color::Red),
    };
    Line::from(vec![Span::styled(format!(" {mark} "), Style::new().fg(color).bold()), Span::raw(item.path.clone())])
}
