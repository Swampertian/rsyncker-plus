use std::sync::mpsc::{self, Receiver};
use std::thread;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::discover::{self, Device, Found};
use crate::remote::Protocol;
use crate::tui::mouse::{Hits, Target};
use crate::tui::screens::job_form::JobForm;
use crate::tui::view::{centered, key_hints};

#[derive(Debug)]
pub struct Discover {
    pub form: Box<JobForm>,
    pub devices: Vec<Device>,
    pub warning: Option<String>,
    pub selected: usize,
    pending: Option<Receiver<Found>>,
}

impl Discover {
    pub fn start(form: Box<JobForm>) -> Self {
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let _ = sender.send(discover::discover());
        });
        Self::with(form, receiver)
    }

    pub fn with(form: Box<JobForm>, pending: Receiver<Found>) -> Self {
        Self { form, devices: Vec::new(), warning: None, selected: 0, pending: Some(pending) }
    }

    pub fn update(&mut self) {
        let Some(pending) = &self.pending else {
            return;
        };
        match pending.try_recv() {
            Ok(found) => {
                self.devices = found.devices;
                self.warning = found.warning;
                self.pending = None;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.warning = Some("busca interrompida".to_string());
                self.pending = None;
            }
        }
    }

    pub fn is_searching(&self) -> bool {
        self.pending.is_some()
    }

    pub fn move_by(&mut self, delta: isize) {
        let max = self.devices.len().saturating_sub(1) as isize;
        self.selected = (self.selected as isize + delta).clamp(0, max.max(0)) as usize;
    }

    pub fn choose(mut self) -> Box<JobForm> {
        if let Some(device) = self.devices.get(self.selected) {
            self.form.set_focused(&device.endpoint());
        }
        self.form
    }
}

pub fn render(frame: &mut Frame, view: &Discover, hits: &mut Hits) {
    let area = centered(frame.area(), 84, 20);
    let block = Block::bordered().title(" Dispositivos na rede ").border_style(Style::new().fg(Color::Cyan));
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();
    if view.is_searching() {
        lines.push(Line::from(" buscando via mDNS…").cyan());
    } else if view.devices.is_empty() {
        lines.push(Line::from(" nenhum dispositivo com SSH ou rsync encontrado").dark_gray());
        lines.push(Line::from(" digite o endereço direto: usuário@host:/caminho ou rsync://host/módulo").dark_gray());
    }
    let list_top = inner.y + lines.len() as u16;
    let visible = inner.height.saturating_sub(lines.len() as u16 + 3) as usize;
    let offset = view.selected.saturating_sub(visible.saturating_sub(1));
    for (i, device) in view.devices.iter().enumerate().skip(offset).take(visible) {
        hits.add(Rect { y: list_top + (i - offset) as u16, height: 1, ..inner }, Target::Row(i));
        lines.push(device_line(device, i == view.selected));
    }

    if let Some(warning) = &view.warning {
        let warning_area = Rect { y: inner.y + inner.height.saturating_sub(2), height: 1, ..inner };
        frame.render_widget(Line::from(format!(" ! {warning}")).yellow(), warning_area);
    }
    frame.render_widget(Paragraph::new(lines), inner);

    let keys = Rect { y: inner.y + inner.height.saturating_sub(1), height: 1, ..inner };
    let items = [("↑↓", "escolher", None), ("Enter", "usar", Some(Target::Confirm)), ("Esc", "voltar", Some(Target::Reject))];
    frame.render_widget(key_hints(keys, &items, hits), keys);
}

fn device_line(device: &Device, selected: bool) -> Line<'static> {
    let kind = match device.protocol {
        Protocol::Ssh => "ssh",
        Protocol::Daemon => "rsync",
    };
    let origin = if device.from_config { "~/.ssh/config".to_string() } else { device.address.clone() };
    let base = if selected { Style::new().add_modifier(Modifier::REVERSED) } else { Style::new() };
    Line::from(vec![
        Span::styled(format!(" {:<6}", kind), base.fg(Color::Cyan)),
        Span::styled(format!("{:<30}", device.name), base.add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:<26}", device.host), base),
        Span::styled(origin, base.fg(Color::DarkGray)),
    ])
}
