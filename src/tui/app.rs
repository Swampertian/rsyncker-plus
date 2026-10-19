use std::collections::HashMap;
use std::mem;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Result;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::widgets::TableState;
use ratatui::{DefaultTerminal, Frame};

use crate::daemon::systemd::{self, UnitStatus};
use crate::job::Job;
use crate::manage;
use crate::paths::Paths;
use crate::remote::Side;
use crate::state::RunState;
use crate::tui::mouse::{Hits, Target};
use crate::tui::screens::discover::{self, Discover};
use crate::tui::screens::job_form::{FormEvent, JobForm};
use crate::tui::screens::preview::{self, Preview};
use crate::tui::screens::run::{self, RunView};
use crate::tui::screens::{confirm, dashboard, job_form};
use crate::tui::view::Status;

const REFRESH_EVERY: Duration = Duration::from_secs(3);
const POLL: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    Up,
    Down,
    New,
    Edit,
    Toggle,
    Remove,
    RunNow,
    Preview,
    Refresh,
}

impl Action {
    pub fn from_key(key: KeyEvent) -> Option<Self> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        Some(match key.code {
            KeyCode::Char('c') if ctrl => Action::Quit,
            KeyCode::Char('q') | KeyCode::Esc => Action::Quit,
            KeyCode::Up | KeyCode::Char('k') => Action::Up,
            KeyCode::Down | KeyCode::Char('j') => Action::Down,
            KeyCode::Char('n') => Action::New,
            KeyCode::Char('e') | KeyCode::Enter => Action::Edit,
            KeyCode::Char(' ') => Action::Toggle,
            KeyCode::Char('x') | KeyCode::Delete => Action::Remove,
            KeyCode::Char('r') => Action::RunNow,
            KeyCode::Char('d') => Action::Preview,
            KeyCode::F(5) => Action::Refresh,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Row {
    pub job: Job,
    pub last_run: Option<RunState>,
    pub running: bool,
    pub next_run: Option<String>,
}

impl Row {
    pub fn status(&self) -> Status {
        if self.running {
            Status::Running
        } else if self.job.is_local(Side::Destination) && !self.job.destination.is_dir() {
            Status::DestinationMissing
        } else {
            self.last_run.as_ref().map_or(Status::Never, |r| Status::Last(r.outcome))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Error,
}

#[derive(Debug, Clone)]
pub struct Message {
    pub level: Level,
    pub text: String,
}

#[derive(Debug, Clone)]
pub enum Pending {
    Remove(String),
}

#[derive(Debug)]
pub enum Screen {
    Dashboard,
    Form(Box<JobForm>),
    Confirm { title: String, message: String, pending: Pending },
    Preview(Box<Preview>),
    Run(Box<RunView>),
    Discover(Box<Discover>),
}

pub struct App {
    paths: Paths,
    exe: PathBuf,
    pub screen: Screen,
    pub rows: Vec<Row>,
    pub table: TableState,
    pub message: Option<Message>,
    pub quit: bool,
    pub hits: Hits,
    last_refresh: Instant,
}

impl App {
    pub fn new(paths: Paths, exe: PathBuf) -> Self {
        let mut app = Self {
            paths,
            exe,
            screen: Screen::Dashboard,
            rows: Vec::new(),
            table: TableState::default(),
            message: None,
            quit: false,
            hits: Hits::default(),
            last_refresh: Instant::now(),
        };
        app.refresh();
        app
    }

    pub fn run(mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        while !self.quit {
            terminal.draw(|frame| self.render(frame))?;
            if event::poll(POLL)? {
                match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => self.on_key(key),
                    Event::Mouse(mouse) => self.on_mouse(mouse),
                    _ => {}
                }
            }
            self.tick();
            if self.last_refresh.elapsed() >= REFRESH_EVERY {
                self.refresh();
            }
        }
        Ok(())
    }

    pub fn render(&mut self, frame: &mut Frame) {
        self.hits.clear();
        dashboard::render(frame, &self.rows, &mut self.table, self.message.as_ref(), &mut self.hits);
        if !matches!(self.screen, Screen::Dashboard) {
            self.hits.clear();
        }
        match &self.screen {
            Screen::Dashboard => {}
            Screen::Form(form) => job_form::render(frame, form, &mut self.hits),
            Screen::Confirm { title, message, .. } => confirm::render(frame, title, message, &mut self.hits),
            Screen::Preview(preview) => preview::render(frame, preview, &mut self.hits),
            Screen::Run(view) => run::render(frame, view, &mut self.hits),
            Screen::Discover(view) => discover::render(frame, view, &mut self.hits),
        }
    }

    pub fn tick(&mut self) {
        match &mut self.screen {
            Screen::Preview(preview) => {
                preview.update();
                if preview.save.is_some() && preview.host.is_none() && matches!(preview.result, Some(Err(_))) {
                    self.back_to_form_with_error();
                }
            }
            Screen::Run(view) => {
                if view.update() {
                    self.refresh();
                }
            }
            Screen::Discover(view) => view.update(),
            _ => {}
        }
    }

    fn back_to_form_with_error(&mut self) {
        let Screen::Preview(preview) = mem::replace(&mut self.screen, Screen::Dashboard) else {
            return;
        };
        let error = preview.error();
        if let Some(mut form) = preview.save {
            form.error = error;
            self.screen = Screen::Form(form);
        }
    }

    pub fn refresh(&mut self) {
        self.last_refresh = Instant::now();
        let entries = match manage::list(&self.paths) {
            Ok(entries) => entries,
            Err(error) => {
                self.error(format!("{error:#}"));
                return;
            }
        };
        let names: Vec<String> = entries.iter().map(|e| e.job.name.clone()).collect();
        let mut units: HashMap<String, UnitStatus> = systemd::status(&names);
        self.rows = entries
            .into_iter()
            .map(|entry| {
                let unit = units.remove(&entry.job.name).unwrap_or_default();
                Row { job: entry.job, last_run: entry.last_run, running: unit.running, next_run: unit.next_run }
            })
            .collect();
        let selected = match (self.table.selected(), self.rows.len()) {
            (_, 0) => None,
            (Some(i), len) => Some(i.min(len - 1)),
            (None, _) => Some(0),
        };
        self.table.select(selected);
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        match &mut self.screen {
            Screen::Dashboard => {
                if let Some(action) = Action::from_key(key) {
                    self.message = None;
                    self.dispatch(action);
                }
            }
            Screen::Form(form) => match form.on_key(key) {
                FormEvent::None => {}
                FormEvent::Cancel => self.back(),
                FormEvent::Submit => self.submit_form(),
                FormEvent::Discover => self.open_discover(),
            },
            Screen::Discover(view) => match key.code {
                KeyCode::Up | KeyCode::Char('k') => view.move_by(-1),
                KeyCode::Down | KeyCode::Char('j') => view.move_by(1),
                KeyCode::Enter => self.yes(),
                KeyCode::Esc | KeyCode::Char('q') => self.back(),
                _ => {}
            },
            Screen::Confirm { .. } => match key.code {
                KeyCode::Char('y') | KeyCode::Char('s') => self.yes(),
                KeyCode::Char('n') | KeyCode::Esc => self.back(),
                _ => {}
            },
            Screen::Preview(preview) => match key.code {
                KeyCode::Up | KeyCode::Char('k') => preview.scroll_by(-1),
                KeyCode::Down | KeyCode::Char('j') => preview.scroll_by(1),
                KeyCode::PageUp => preview.scroll_by(-10),
                KeyCode::PageDown => preview.scroll_by(10),
                KeyCode::Char('y') | KeyCode::Char('s') => self.yes(),
                KeyCode::Char('n') | KeyCode::Char('q') | KeyCode::Esc => self.back(),
                _ => {}
            },
            Screen::Run(_) => {
                let ctrl_c = key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL);
                if ctrl_c || matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
                    self.back();
                }
            }
        }
    }

    fn yes(&mut self) {
        match &self.screen {
            Screen::Confirm { .. } => self.confirm(),
            Screen::Discover(_) => {
                if let Screen::Discover(view) = mem::replace(&mut self.screen, Screen::Dashboard) {
                    self.screen = Screen::Form(view.choose());
                }
            }
            Screen::Preview(preview) if preview.host.is_some() => {
                let Screen::Preview(preview) = mem::replace(&mut self.screen, Screen::Dashboard) else {
                    return;
                };
                match preview.trust_host() {
                    Ok(restarted) => self.screen = Screen::Preview(Box::new(restarted)),
                    Err((mut preview, error)) => {
                        preview.host = None;
                        preview.result = Some(Err(error));
                        self.screen = Screen::Preview(Box::new(preview));
                    }
                }
            }
            Screen::Preview(preview) if preview.can_save() => {
                let Screen::Preview(preview) = mem::replace(&mut self.screen, Screen::Dashboard) else {
                    return;
                };
                if let Some(form) = preview.save {
                    match form.to_job() {
                        Ok(job) => self.save(form, job),
                        Err(error) => self.error(format!("{error:#}")),
                    }
                }
            }
            _ => {}
        }
    }

    fn back(&mut self) {
        match mem::replace(&mut self.screen, Screen::Dashboard) {
            Screen::Preview(mut preview) => {
                preview.cancel();
                if let Some(form) = preview.save {
                    self.screen = Screen::Form(form);
                }
            }
            Screen::Run(mut view) if view.is_running() => {
                view.cancel();
                self.screen = Screen::Run(view);
            }
            Screen::Discover(view) => self.screen = Screen::Form(view.form),
            _ => {}
        }
    }

    pub fn on_mouse(&mut self, mouse: MouseEvent) {
        match mouse.kind {
            MouseEventKind::ScrollUp => self.scroll(-1),
            MouseEventKind::ScrollDown => self.scroll(1),
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(target) = self.hits.at(mouse.column, mouse.row) {
                    self.click(target);
                }
            }
            _ => {}
        }
    }

    fn scroll(&mut self, delta: isize) {
        match &mut self.screen {
            Screen::Dashboard if delta < 0 => self.dispatch(Action::Up),
            Screen::Dashboard => self.dispatch(Action::Down),
            Screen::Form(form) => form.move_focus(delta),
            Screen::Preview(preview) => preview.scroll_by(delta * 3),
            Screen::Discover(view) => view.move_by(delta),
            Screen::Confirm { .. } | Screen::Run(_) => {}
        }
    }

    pub fn click(&mut self, target: Target) {
        match target {
            Target::Row(i) if matches!(self.screen, Screen::Discover(_)) => {
                if let Screen::Discover(view) = &mut self.screen {
                    if view.selected == i {
                        self.yes();
                    } else {
                        view.selected = i;
                    }
                }
            }
            Target::Row(i) => {
                self.message = None;
                if self.table.selected() == Some(i) {
                    self.dispatch(Action::Edit);
                } else {
                    self.table.select(Some(i));
                }
            }
            Target::Action(action) => {
                self.message = None;
                self.dispatch(action);
            }
            Target::Field(i) => {
                if let Screen::Form(form) = &mut self.screen {
                    form.click(i);
                }
            }
            Target::Submit => self.submit_form(),
            Target::Discover => self.open_discover(),
            Target::Cancel | Target::Reject => self.back(),
            Target::Confirm => self.yes(),
        }
    }

    fn selected_job(&self) -> Option<&Job> {
        self.table.selected().and_then(|i| self.rows.get(i)).map(|r| &r.job)
    }

    pub fn dispatch(&mut self, action: Action) {
        match action {
            Action::Quit => self.quit = true,
            Action::Up => self.table.select_previous(),
            Action::Down => {
                if self.table.selected().is_some_and(|i| i + 1 < self.rows.len()) {
                    self.table.select_next();
                }
            }
            Action::Refresh => self.refresh(),
            Action::New => self.screen = Screen::Form(Box::new(JobForm::new())),
            Action::Edit => {
                if let Some(job) = self.selected_job() {
                    self.screen = Screen::Form(Box::new(JobForm::edit(job)));
                }
            }
            Action::Toggle => {
                if let Some(job) = self.selected_job().cloned() {
                    match manage::set_enabled(&self.paths, &job.name, !job.enabled, &self.exe) {
                        Ok(updated) if updated.enabled => self.info(format!("'{}' ativado", job.name)),
                        Ok(_) => self.info(format!("'{}' desativado", job.name)),
                        Err(error) => self.error(format!("{error:#}")),
                    }
                    self.refresh();
                }
            }
            Action::Remove => {
                if let Some(job) = self.selected_job() {
                    self.screen = Screen::Confirm {
                        title: "Remover job".to_string(),
                        message: format!("Remover o job '{}' e seu timer? Os arquivos no destino não são apagados.", job.name),
                        pending: Pending::Remove(job.name.clone()),
                    };
                }
            }
            Action::RunNow => {
                if let Some(job) = self.selected_job() {
                    self.screen = Screen::Run(Box::new(RunView::start(&self.paths, &job.name)));
                }
            }
            Action::Preview => {
                if let Some(job) = self.selected_job() {
                    self.screen = Screen::Preview(Box::new(Preview::start(job, None)));
                }
            }
        }
    }

    fn submit_form(&mut self) {
        let Screen::Form(mut form) = mem::replace(&mut self.screen, Screen::Dashboard) else {
            return;
        };
        let job = match form.to_job() {
            Ok(job) => job,
            Err(error) => {
                form.error = Some(format!("{error:#}"));
                self.screen = Screen::Form(form);
                return;
            }
        };
        if form.is_new() || form.turns_into_mirror() || form.changes_remote(&job) {
            self.screen = Screen::Preview(Box::new(Preview::start(&job, Some(form))));
            return;
        }
        self.save(form, job);
    }

    fn open_discover(&mut self) {
        let Screen::Form(mut form) = mem::replace(&mut self.screen, Screen::Dashboard) else {
            return;
        };
        if !form.picks_endpoint() {
            form.focus = 1;
        }
        self.screen = Screen::Discover(Box::new(Discover::start(form)));
    }

    fn save(&mut self, mut form: Box<JobForm>, job: Job) {
        let result = if form.is_new() { manage::add(&self.paths, job, &self.exe) } else { manage::update(&self.paths, job, &self.exe) };
        match result {
            Ok(saved) => {
                let verb = if form.is_new() { "criado" } else { "atualizado" };
                self.info(format!("job '{}' {verb}", saved.name));
                self.refresh();
                if let Some(i) = self.rows.iter().position(|r| r.job.name == saved.name) {
                    self.table.select(Some(i));
                }
            }
            Err(error) => {
                form.error = Some(format!("{error:#}"));
                self.screen = Screen::Form(form);
            }
        }
    }

    fn confirm(&mut self) {
        let Screen::Confirm { pending, .. } = mem::replace(&mut self.screen, Screen::Dashboard) else {
            return;
        };
        let Pending::Remove(name) = pending;
        match manage::remove(&self.paths, &name) {
            Ok(()) => self.info(format!("job '{name}' removido")),
            Err(error) => self.error(format!("{error:#}")),
        }
        self.refresh();
    }

    fn info(&mut self, text: String) {
        self.message = Some(Message { level: Level::Info, text });
    }

    fn error(&mut self, text: String) {
        self.message = Some(Message { level: Level::Error, text });
    }
}
