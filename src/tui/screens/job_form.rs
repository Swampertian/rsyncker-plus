use anyhow::{Context, Result, bail};
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::job::{self, Job, Mode, Schedule};
use crate::paths::expand_home;
use crate::tui::input::TextInput;
use crate::tui::view::{centered, mode_label, schedule_label};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Source,
    Destination,
    Name,
    Mode,
    Schedule,
    Excludes,
    Bwlimit,
}

pub const FIELDS: [Field; 7] = [Field::Source, Field::Destination, Field::Name, Field::Mode, Field::Schedule, Field::Excludes, Field::Bwlimit];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormEvent {
    None,
    Cancel,
    Submit,
}

#[derive(Debug, Clone)]
pub struct JobForm {
    pub original: Option<Job>,
    pub source: TextInput,
    pub destination: TextInput,
    pub name: TextInput,
    pub mode: Mode,
    pub schedule: Schedule,
    pub excludes: TextInput,
    pub bwlimit: TextInput,
    pub focus: usize,
    pub error: Option<String>,
    name_edited: bool,
}

impl JobForm {
    pub fn new() -> Self {
        Self {
            original: None,
            source: TextInput::default(),
            destination: TextInput::default(),
            name: TextInput::default(),
            mode: Mode::Copy,
            schedule: Schedule::Daily,
            excludes: TextInput::default(),
            bwlimit: TextInput::default(),
            focus: 0,
            error: None,
            name_edited: false,
        }
    }

    pub fn edit(job: &Job) -> Self {
        Self {
            original: Some(job.clone()),
            source: TextInput::new(&job.source.to_string_lossy()),
            destination: TextInput::new(&job.destination.to_string_lossy()),
            name: TextInput::new(&job.name),
            mode: job.mode,
            schedule: job.schedule,
            excludes: TextInput::new(&job.excludes.join(", ")),
            bwlimit: TextInput::new(&job.bwlimit_kbps.map(|v| v.to_string()).unwrap_or_default()),
            focus: 0,
            error: None,
            name_edited: true,
        }
    }

    pub fn is_new(&self) -> bool {
        self.original.is_none()
    }

    pub fn focused(&self) -> Field {
        FIELDS[self.focus]
    }

    pub fn on_key(&mut self, key: KeyEvent) -> FormEvent {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => return FormEvent::Cancel,
            KeyCode::Char('s') if ctrl => return FormEvent::Submit,
            KeyCode::Enter if self.focus == FIELDS.len() - 1 => return FormEvent::Submit,
            KeyCode::Enter | KeyCode::Tab | KeyCode::Down => self.move_focus(1),
            KeyCode::BackTab | KeyCode::Up => self.move_focus(-1),
            _ => self.edit_field(key),
        }
        FormEvent::None
    }

    fn move_focus(&mut self, delta: isize) {
        let len = FIELDS.len() as isize;
        self.focus = (self.focus as isize + delta).rem_euclid(len) as usize;
    }

    fn edit_field(&mut self, key: KeyEvent) {
        let forward = matches!(key.code, KeyCode::Right | KeyCode::Char(' '));
        let backward = key.code == KeyCode::Left;
        match self.focused() {
            Field::Mode if forward || backward => {
                self.mode = match self.mode {
                    Mode::Copy => Mode::Mirror,
                    Mode::Mirror => Mode::Copy,
                };
            }
            Field::Schedule if forward || backward => {
                let all = [Schedule::None, Schedule::Hourly, Schedule::Daily, Schedule::Weekly];
                let i = all.iter().position(|s| *s == self.schedule).unwrap_or(0);
                let next = if forward { i + 1 } else { i + all.len() - 1 };
                self.schedule = all[next % all.len()];
            }
            Field::Mode | Field::Schedule => {}
            Field::Name if !self.is_new() => {}
            Field::Name => {
                if self.name.handle(key) {
                    self.name_edited = !self.name.value().is_empty();
                }
            }
            Field::Source => {
                if self.source.handle(key) && self.is_new() && !self.name_edited {
                    let suggestion = job::name_from_path(&expand_home(self.source.value()));
                    self.name.set(&suggestion);
                }
            }
            Field::Destination => {
                self.destination.handle(key);
            }
            Field::Excludes => {
                self.excludes.handle(key);
            }
            Field::Bwlimit => {
                self.bwlimit.handle(key);
            }
        }
    }

    pub fn to_job(&self) -> Result<Job> {
        let source = self.source.value().trim();
        let destination = self.destination.value().trim();
        if source.is_empty() {
            bail!("informe a origem");
        }
        if destination.is_empty() {
            bail!("informe o destino");
        }
        let name = self.name.value().trim();
        job::validate_name(name)?;

        let mut job = Job::new(name, expand_home(source), expand_home(destination), self.mode);
        job.schedule = self.schedule;
        job.excludes = self.excludes.value().split(',').map(str::trim).filter(|p| !p.is_empty()).map(String::from).collect();
        let bwlimit = self.bwlimit.value().trim();
        job.bwlimit_kbps = if bwlimit.is_empty() { None } else { Some(bwlimit.parse().context("limite de banda deve ser um número (KB/s)")?) };
        if let Some(original) = &self.original {
            job.enabled = original.enabled;
        }
        Ok(job)
    }

    pub fn turns_into_mirror(&self) -> bool {
        self.mode == Mode::Mirror && self.original.as_ref().is_none_or(|o| o.mode != Mode::Mirror)
    }
}

impl Default for JobForm {
    fn default() -> Self {
        Self::new()
    }
}

const LABEL_WIDTH: u16 = 14;

pub fn render(frame: &mut Frame, form: &JobForm) {
    let area = centered(frame.area(), 76, 19);
    let title = if form.is_new() { " Novo job " } else { " Editar job " };
    let block = Block::bordered().title(title).border_style(Style::new().fg(Color::Cyan));
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);

    let value_width = inner.width.saturating_sub(LABEL_WIDTH + 1) as usize;
    let mut lines = Vec::new();
    let mut cursor = None;

    for (i, field) in FIELDS.iter().enumerate() {
        let focused = i == form.focus;
        let label_style = if focused { Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD) } else { Style::new().fg(Color::Gray) };
        let label = Span::styled(format!("{:<width$}", label_of(*field), width = LABEL_WIDTH as usize), label_style);
        let value = match field {
            Field::Mode => choice(mode_label(form.mode), focused),
            Field::Schedule => choice(schedule_label(form.schedule), focused),
            _ => {
                let input = input_of(form, *field);
                let (visible, col) = input.window(value_width);
                let editable = !matches!(field, Field::Name) || form.is_new();
                if focused && editable {
                    cursor = Some(Position::new(inner.x + LABEL_WIDTH + 1 + col as u16, inner.y + lines.len() as u16));
                }
                let style = if editable { Style::new() } else { Style::new().fg(Color::DarkGray) };
                Span::styled(visible, style)
            }
        };
        lines.push(Line::from(vec![label, " ".into(), value]));
        if let Some(hint) = hint_of(*field) {
            lines.push(Line::from(format!("{:width$} {hint}", "", width = LABEL_WIDTH as usize)).dark_gray());
        }
    }

    lines.push(Line::default());
    if form.mode == Mode::Mirror {
        lines.push(Line::from("⚠ espelho apaga no destino o que não existir na origem").yellow());
    }
    if let Some(error) = &form.error {
        lines.push(Line::from(format!("✗ {error}")).red());
    }

    let help_area = Rect { y: inner.y + inner.height.saturating_sub(1), height: 1, ..inner };
    frame.render_widget(Paragraph::new(lines), inner);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            "Tab/↑↓".bold(),
            " campos  ".into(),
            "←→/espaço".bold(),
            " opções  ".into(),
            "Ctrl+S".bold(),
            " salvar  ".into(),
            "Esc".bold(),
            " cancelar".into(),
        ]))
        .dark_gray(),
        help_area,
    );
    if let Some(position) = cursor {
        frame.set_cursor_position(position);
    }
}

fn label_of(field: Field) -> &'static str {
    match field {
        Field::Source => "Origem",
        Field::Destination => "Destino",
        Field::Name => "Nome",
        Field::Mode => "Modo",
        Field::Schedule => "Agenda",
        Field::Excludes => "Ignorar",
        Field::Bwlimit => "Banda (KB/s)",
    }
}

fn hint_of(field: Field) -> Option<&'static str> {
    match field {
        Field::Destination => Some("a pasta precisa existir"),
        Field::Excludes => Some("separados por vírgula: .cache, *.tmp"),
        Field::Bwlimit => Some("vazio = sem limite"),
        _ => None,
    }
}

fn input_of(form: &JobForm, field: Field) -> &TextInput {
    match field {
        Field::Source => &form.source,
        Field::Destination => &form.destination,
        Field::Name => &form.name,
        Field::Excludes => &form.excludes,
        _ => &form.bwlimit,
    }
}

fn choice(label: &str, focused: bool) -> Span<'static> {
    let text = format!("◀ {label} ▶");
    if focused { Span::styled(text, Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)) } else { Span::raw(text) }
}
