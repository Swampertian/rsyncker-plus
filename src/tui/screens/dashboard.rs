use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Paragraph, Row as TableRow, Table, TableState};

use crate::state;
use crate::tui::app::{Action, Level, Message, Row};
use crate::tui::mouse::{Hits, Target};
use crate::tui::view::{format_bytes, key_hints, mode_label, short_next_run, status_label, trigger_label};

pub fn render(frame: &mut Frame, rows: &[Row], table: &mut TableState, message: Option<&Message>, hits: &mut Hits) {
    let [header, body, details, status, footer] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(5), Constraint::Length(6), Constraint::Length(1), Constraint::Length(1)]).areas(frame.area());

    frame.render_widget(Line::from(vec![" rsyncker-plus ".bold().black().on_cyan(), format!("  {} job(s)", rows.len()).dark_gray()]), header);

    if rows.is_empty() {
        render_empty(frame, body);
    } else {
        render_table(frame, body, rows, table);
        add_row_hits(hits, body, rows.len(), table.offset());
    }

    let selected = table.selected().and_then(|i| rows.get(i));
    render_details(frame, details, selected);

    if let Some(message) = message {
        let style = match message.level {
            Level::Info => Style::new().fg(Color::Green),
            Level::Error => Style::new().fg(Color::Red),
        };
        frame.render_widget(Line::styled(format!(" {}", message.text), style), status);
    }

    frame.render_widget(footer_line(footer, hits), footer);
}

fn add_row_hits(hits: &mut Hits, body: Rect, count: usize, offset: usize) {
    let visible = body.height.saturating_sub(3);
    for line in 0..visible {
        let index = offset + line as usize;
        if index >= count {
            break;
        }
        let area = Rect { x: body.x + 1, y: body.y + 2 + line, width: body.width.saturating_sub(2), height: 1 };
        hits.add(area, Target::Row(index));
    }
}

fn render_empty(frame: &mut Frame, area: Rect) {
    let text = vec![
        Line::default(),
        Line::from("Nenhum diretório configurado.").bold(),
        Line::from(vec!["Pressione ".into(), "n".bold().cyan(), " para criar a primeira sincronização.".into()]),
    ];
    frame.render_widget(Paragraph::new(text).centered().block(Block::bordered().title(" Jobs ")), area);
}

fn render_table(frame: &mut Frame, area: Rect, rows: &[Row], table: &mut TableState) {
    let now = state::now();
    let header = TableRow::new(["Status", "Job", "Modo", "Agenda", "Último run", "Próximo run", "Origem → Destino"])
        .style(Style::new().add_modifier(Modifier::BOLD).fg(Color::Gray));

    let lines = rows.iter().map(|row| {
        let job = &row.job;
        let (status_text, status_style) = status_label(row.status());
        let last = row.last_run.as_ref().map_or_else(|| "-".to_string(), |r| state::format_ago(r.finished_at, now));
        let (agenda, next) = if job.enabled {
            (trigger_label(job), row.next_run.as_deref().map_or_else(|| "-".to_string(), short_next_run))
        } else {
            ("desativado".to_string(), "-".to_string())
        };
        let paths = format!("{} → {}", job.source_label(), job.destination_label());
        let style = if job.enabled { Style::new() } else { Style::new().fg(Color::DarkGray) };
        TableRow::new([
            Cell::from(Span::styled(status_text, status_style)),
            Cell::from(job.name.clone()).bold(),
            Cell::from(mode_label(job.mode)),
            Cell::from(agenda),
            Cell::from(last),
            Cell::from(next),
            Cell::from(paths),
        ])
        .style(style)
    });

    let widths = [
        Constraint::Length(14),
        Constraint::Length(16),
        Constraint::Length(8),
        Constraint::Length(21),
        Constraint::Length(11),
        Constraint::Length(17),
        Constraint::Min(20),
    ];
    let widget = Table::new(lines, widths)
        .header(header)
        .block(Block::bordered().title(" Jobs "))
        .row_highlight_style(Style::new().add_modifier(Modifier::REVERSED))
        .highlight_symbol("▶ ");
    frame.render_stateful_widget(widget, area, table);
}

fn render_details(frame: &mut Frame, area: Rect, row: Option<&Row>) {
    let block = Block::bordered().title(" Detalhes ");
    let Some(row) = row else {
        frame.render_widget(block, area);
        return;
    };
    let job = &row.job;
    let label = |text: &'static str| Span::styled(format!("{text:<10}"), Style::new().fg(Color::Gray));
    let excludes = if job.excludes.is_empty() { "-".to_string() } else { job.excludes.join(", ") };
    let mut lines = vec![
        Line::from(vec![label("Origem"), job.source_label().into()]),
        Line::from(vec![label("Destino"), job.destination_label().into()]),
        Line::from(vec![label("Ignorar"), excludes.into()]),
    ];
    let last = match &row.last_run {
        None => Line::from(vec![label("Último"), "nunca rodou".dark_gray()]),
        Some(run) => {
            let mut spans = vec![
                label("Último"),
                format!("{} arquivo(s), {} copiados, {} apagados", run.files_transferred, format_bytes(run.bytes_transferred), run.files_deleted).into(),
            ];
            if let Some(message) = run.message.as_deref().filter(|m| !m.is_empty()) {
                spans.push(format!("  {message}").red());
            }
            Line::from(spans)
        }
    };
    lines.push(last);
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn footer_line(area: Rect, hits: &mut Hits) -> Line<'static> {
    let keys = [
        ("n", "novo", Action::New),
        ("e", "editar", Action::Edit),
        ("espaço", "ativar/desativar", Action::Toggle),
        ("r", "rodar agora", Action::RunNow),
        ("d", "prévia", Action::Preview),
        ("x", "remover", Action::Remove),
        ("q", "sair", Action::Quit),
    ];
    let items: Vec<_> = keys.iter().map(|(key, label, action)| (*key, *label, Some(Target::Action(*action)))).collect();
    key_hints(area, &items, hits)
}
