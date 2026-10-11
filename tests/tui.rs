use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use rsync_tui::job::{Job, Mode, Schedule};
use rsync_tui::paths::Paths;
use rsync_tui::tui::app::{Action, App, Screen};
use rsync_tui::tui::input::TextInput;
use rsync_tui::tui::mouse::{Hits, Target};
use rsync_tui::tui::screens::job_form::{FIELDS, Field, FormEvent, JobForm};
use rsync_tui::tui::view;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn type_text(form: &mut JobForm, text: &str) {
    for c in text.chars() {
        form.on_key(key(KeyCode::Char(c)));
    }
}

fn focus(form: &mut JobForm, field: Field) {
    while form.focused() != field {
        form.on_key(key(KeyCode::Tab));
    }
}

#[test]
fn keys_map_to_actions() {
    assert_eq!(Action::from_key(key(KeyCode::Char('n'))), Some(Action::New));
    assert_eq!(Action::from_key(key(KeyCode::Char(' '))), Some(Action::Toggle));
    assert_eq!(Action::from_key(key(KeyCode::Char('x'))), Some(Action::Remove));
    assert_eq!(Action::from_key(key(KeyCode::Char('r'))), Some(Action::RunNow));
    assert_eq!(Action::from_key(key(KeyCode::Down)), Some(Action::Down));
    assert_eq!(Action::from_key(ctrl('c')), Some(Action::Quit));
    assert_eq!(Action::from_key(key(KeyCode::Char('z'))), None);
}

#[test]
fn text_input_edits_at_cursor() {
    let mut input = TextInput::new("ação");
    input.handle(key(KeyCode::Left));
    input.handle(key(KeyCode::Backspace));
    assert_eq!(input.value(), "aço");
    input.handle(key(KeyCode::Home));
    input.handle(key(KeyCode::Char('X')));
    assert_eq!(input.value(), "Xaço");
    assert_eq!(input.cursor(), 1);
    assert!(!input.handle(ctrl('s')));
}

#[test]
fn text_input_window_follows_cursor() {
    let input = TextInput::new("abcdefghij");
    assert_eq!(input.window(4), ("hij".to_string(), 3));
    assert_eq!(TextInput::new("ab").window(4), ("ab".to_string(), 2));
}

#[test]
fn source_suggests_name_until_name_is_edited() {
    let mut form = JobForm::new();
    type_text(&mut form, "/home/u/Minhas Fotos");
    assert_eq!(form.name.value(), "minhas-fotos");

    focus(&mut form, Field::Name);
    form.on_key(key(KeyCode::Char('x')));
    focus(&mut form, Field::Source);
    type_text(&mut form, "2");
    assert_eq!(form.name.value(), "minhas-fotosx");
}

#[test]
fn tab_wraps_around_fields() {
    let mut form = JobForm::new();
    form.on_key(key(KeyCode::BackTab));
    assert_eq!(form.focused(), Field::Bwlimit);
    form.on_key(key(KeyCode::Tab));
    assert_eq!(form.focused(), Field::Source);
}

#[test]
fn choices_cycle_with_arrows_and_space() {
    let mut form = JobForm::new();
    focus(&mut form, Field::Mode);
    form.on_key(key(KeyCode::Char(' ')));
    assert_eq!(form.mode, Mode::Mirror);
    focus(&mut form, Field::Schedule);
    form.on_key(key(KeyCode::Right));
    assert_eq!(form.schedule, Schedule::Weekly);
    form.on_key(key(KeyCode::Right));
    assert_eq!(form.schedule, Schedule::None);
    form.on_key(key(KeyCode::Left));
    assert_eq!(form.schedule, Schedule::Weekly);
}

#[test]
fn form_builds_job() {
    let mut form = JobForm::new();
    type_text(&mut form, "/data/fotos");
    focus(&mut form, Field::Destination);
    type_text(&mut form, "/mnt/b");
    focus(&mut form, Field::Excludes);
    type_text(&mut form, " .cache, *.tmp ,");
    focus(&mut form, Field::Bwlimit);
    type_text(&mut form, "500");

    let job = form.to_job().unwrap();
    assert_eq!(job.name, "fotos");
    assert_eq!(job.source, PathBuf::from("/data/fotos"));
    assert_eq!(job.excludes, [".cache", "*.tmp"]);
    assert_eq!(job.bwlimit_kbps, Some(500));
    assert_eq!(job.schedule, Schedule::Daily);
}

#[test]
fn form_expands_home() {
    let mut form = JobForm::new();
    type_text(&mut form, "~/Fotos");
    focus(&mut form, Field::Destination);
    type_text(&mut form, "/mnt/b");
    let home = PathBuf::from(std::env::var("HOME").unwrap());
    assert_eq!(form.to_job().unwrap().source, home.join("Fotos"));
}

#[test]
fn form_reports_invalid_input() {
    let mut form = JobForm::new();
    assert!(form.to_job().is_err());
    type_text(&mut form, "/a");
    focus(&mut form, Field::Destination);
    type_text(&mut form, "/b");
    focus(&mut form, Field::Bwlimit);
    type_text(&mut form, "rápido");
    assert!(form.to_job().is_err());
}

#[test]
fn edit_keeps_name_and_enabled() {
    let mut job = Job::new("fotos", "/a".into(), "/b".into(), Mode::Mirror);
    job.enabled = false;
    let mut form = JobForm::edit(&job);
    focus(&mut form, Field::Name);
    type_text(&mut form, "outro");
    let edited = form.to_job().unwrap();
    assert_eq!(edited.name, "fotos");
    assert!(!edited.enabled);
    assert!(!form.turns_into_mirror());
}

#[test]
fn mirror_needs_confirmation_only_when_switching() {
    let mut form = JobForm::new();
    assert!(!form.turns_into_mirror());
    form.mode = Mode::Mirror;
    assert!(form.turns_into_mirror());
}

#[test]
fn form_submit_and_cancel_keys() {
    let mut form = JobForm::new();
    assert_eq!(form.on_key(ctrl('s')), FormEvent::Submit);
    assert_eq!(form.on_key(key(KeyCode::Esc)), FormEvent::Cancel);
    focus(&mut form, Field::Bwlimit);
    assert_eq!(form.on_key(key(KeyCode::Enter)), FormEvent::Submit);
    focus(&mut form, Field::Source);
    assert_eq!(form.on_key(key(KeyCode::Enter)), FormEvent::None);
    assert_eq!(form.focused(), Field::Destination);
}

#[test]
fn view_helpers() {
    assert_eq!(view::short_next_run("Fri 2026-10-09 21:00:00 -03"), "2026-10-09 21:00");
    assert_eq!(view::short_next_run("estranho"), "estranho");
    assert_eq!(view::format_bytes(512), "512 B");
    assert_eq!(view::format_bytes(48_213_004), "46.0 MB");
}

fn app_with_job() -> (tempfile::TempDir, App) {
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::under(root.path());
    let src = root.path().join("src");
    let dst = root.path().join("dst");
    fs::create_dir_all(&src).unwrap();
    fs::create_dir_all(&dst).unwrap();
    Job::new("fotos-teste", src, dst, Mode::Copy).save(&paths.jobs).unwrap();
    (root, App::new(paths, PathBuf::from("/bin/true")))
}

fn screen_text(app: &mut App) -> String {
    let mut terminal = Terminal::new(TestBackend::new(140, 30)).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
    terminal.backend().buffer().content().iter().map(|c| c.symbol()).collect()
}

#[test]
fn dashboard_lists_jobs() {
    let (_root, mut app) = app_with_job();
    assert_eq!(app.table.selected(), Some(0));
    let text = screen_text(&mut app);
    assert!(text.contains("fotos-teste"));
    assert!(text.contains("nunca"));
    assert!(text.contains("cópia"));
}

#[test]
fn empty_dashboard_shows_hint() {
    let root = tempfile::tempdir().unwrap();
    let mut app = App::new(Paths::under(root.path()), PathBuf::from("/bin/true"));
    assert!(screen_text(&mut app).contains("Nenhum diretório configurado"));
}

#[test]
fn new_opens_form_and_esc_closes_it() {
    let (_root, mut app) = app_with_job();
    app.on_key(key(KeyCode::Char('n')));
    assert!(matches!(app.screen, Screen::Form(_)));
    assert!(screen_text(&mut app).contains("Novo job"));
    app.on_key(key(KeyCode::Esc));
    assert!(matches!(app.screen, Screen::Dashboard));
}

#[test]
fn remove_asks_for_confirmation_and_can_cancel() {
    let (_root, mut app) = app_with_job();
    app.on_key(key(KeyCode::Char('x')));
    assert!(matches!(app.screen, Screen::Confirm { .. }));
    app.on_key(key(KeyCode::Char('n')));
    assert!(matches!(app.screen, Screen::Dashboard));
    assert_eq!(app.rows.len(), 1);
}

#[test]
fn invalid_form_stays_open_with_error() {
    let (_root, mut app) = app_with_job();
    app.on_key(key(KeyCode::Char('n')));
    app.on_key(ctrl('s'));
    let Screen::Form(form) = &app.screen else {
        panic!("form deveria continuar aberto");
    };
    assert_eq!(form.error.as_deref(), Some("informe a origem"));
}

#[test]
fn quit_key_stops_app() {
    let (_root, mut app) = app_with_job();
    app.on_key(key(KeyCode::Char('q')));
    assert!(app.quit);
}

#[test]
fn confirm_popup_keeps_keys_visible_with_long_text() {
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    let long = format!("Origem: /{}\nDestino: /{}", "a".repeat(150), "b".repeat(150));
    terminal.draw(|frame| rsync_tui::tui::screens::confirm::render(frame, "Teste", &long, &mut Hits::default())).unwrap();
    let text: String = terminal.backend().buffer().content().iter().map(|c| c.symbol()).collect();
    assert!(text.contains("y confirmar"));
}

fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent { kind, column, row, modifiers: KeyModifiers::NONE }
}

fn click_on(app: &mut App, target: Target) {
    screen_text(app);
    let area = app.hits.area_of(target).expect("alvo deveria estar na tela");
    app.on_mouse(mouse(MouseEventKind::Down(MouseButton::Left), area.x, area.y));
}

fn scroll(app: &mut App, kind: MouseEventKind) {
    screen_text(app);
    app.on_mouse(mouse(kind, 0, 0));
}

#[test]
fn clicking_footer_runs_action() {
    let (_root, mut app) = app_with_job();
    click_on(&mut app, Target::Action(Action::New));
    assert!(matches!(app.screen, Screen::Form(_)));
}

#[test]
fn clicking_row_selects_then_edits() {
    let (root, mut app) = app_with_job();
    let dst = root.path().join("dst");
    Job::new("outro", root.path().join("src"), dst, Mode::Copy).save(&Paths::under(root.path()).jobs).unwrap();
    app.refresh();
    assert_eq!(app.rows.len(), 2);

    click_on(&mut app, Target::Row(1));
    assert_eq!(app.table.selected(), Some(1));
    assert!(matches!(app.screen, Screen::Dashboard));

    click_on(&mut app, Target::Row(1));
    let Screen::Form(form) = &app.screen else {
        panic!("segundo clique deveria abrir o editor");
    };
    assert_eq!(form.name.value(), app.rows[1].job.name);
}

#[test]
fn scroll_moves_selection() {
    let (root, mut app) = app_with_job();
    Job::new("outro", root.path().join("src"), root.path().join("dst"), Mode::Copy).save(&Paths::under(root.path()).jobs).unwrap();
    app.refresh();
    scroll(&mut app, MouseEventKind::ScrollDown);
    assert_eq!(app.table.selected(), Some(1));
    scroll(&mut app, MouseEventKind::ScrollUp);
    assert_eq!(app.table.selected(), Some(0));
}

#[test]
fn clicking_form_fields_focuses_and_cycles_choices() {
    let (_root, mut app) = app_with_job();
    app.on_key(key(KeyCode::Char('n')));
    let mode = FIELDS.iter().position(|f| *f == Field::Mode).unwrap();

    click_on(&mut app, Target::Field(mode));
    let Screen::Form(form) = &app.screen else { panic!() };
    assert_eq!(form.focused(), Field::Mode);
    assert_eq!(form.mode, Mode::Copy);

    click_on(&mut app, Target::Field(mode));
    let Screen::Form(form) = &app.screen else { panic!() };
    assert_eq!(form.mode, Mode::Mirror);

    scroll(&mut app, MouseEventKind::ScrollDown);
    let Screen::Form(form) = &app.screen else { panic!() };
    assert_eq!(form.focused(), Field::Schedule);

    click_on(&mut app, Target::Cancel);
    assert!(matches!(app.screen, Screen::Dashboard));
}

#[test]
fn form_blocks_clicks_on_dashboard() {
    let (_root, mut app) = app_with_job();
    app.on_key(key(KeyCode::Char('n')));
    screen_text(&mut app);
    assert!(app.hits.area_of(Target::Row(0)).is_none());
    assert!(app.hits.area_of(Target::Action(Action::Quit)).is_none());
}

#[test]
fn clicking_submit_validates_form() {
    let (_root, mut app) = app_with_job();
    app.on_key(key(KeyCode::Char('n')));
    click_on(&mut app, Target::Submit);
    let Screen::Form(form) = &app.screen else { panic!() };
    assert_eq!(form.error.as_deref(), Some("informe a origem"));
}

#[test]
fn confirm_buttons_respond_to_clicks() {
    let (_root, mut app) = app_with_job();
    app.on_key(key(KeyCode::Char('x')));
    click_on(&mut app, Target::Reject);
    assert!(matches!(app.screen, Screen::Dashboard));
    assert_eq!(app.rows.len(), 1);

    app.on_key(key(KeyCode::Char('x')));
    click_on(&mut app, Target::Confirm);
    assert!(app.rows.is_empty());
}

#[test]
fn hit_areas_cover_their_labels() {
    let (_root, mut app) = app_with_job();
    let mut terminal = Terminal::new(TestBackend::new(140, 30)).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
    let text_at = |target: Target| {
        let area = app.hits.area_of(target).unwrap();
        let buffer = terminal.backend().buffer();
        (area.x..area.x + area.width).map(|x| buffer[(x, area.y)].symbol().to_string()).collect::<String>()
    };
    assert_eq!(text_at(Target::Action(Action::New)), "n novo");
    assert_eq!(text_at(Target::Action(Action::Toggle)), "espaço ativar/desativar");
    assert_eq!(text_at(Target::Action(Action::Quit)), "q sair");
    assert!(text_at(Target::Row(0)).contains("fotos-teste"));
}

fn wait_until(app: &mut App, done: impl Fn(&App) -> bool) {
    let start = Instant::now();
    while !done(app) {
        assert!(start.elapsed() < Duration::from_secs(10), "tarefa não terminou a tempo");
        thread::sleep(Duration::from_millis(10));
        app.tick();
    }
}

fn preview_done(app: &App) -> bool {
    !matches!(&app.screen, Screen::Preview(p) if !p.is_done())
}

fn run_done(app: &App) -> bool {
    !matches!(&app.screen, Screen::Run(view) if view.is_running())
}

fn fill_new_job(app: &mut App, root: &std::path::Path, destination: &str) {
    app.on_key(key(KeyCode::Char('n')));
    let Screen::Form(form) = &mut app.screen else { panic!() };
    type_text(form, &root.join("src").to_string_lossy());
    focus(form, Field::Name);
    while !form.name.value().is_empty() {
        form.on_key(key(KeyCode::Backspace));
    }
    type_text(form, "novo");
    focus(form, Field::Destination);
    type_text(form, &root.join(destination).to_string_lossy());
    form.schedule = Schedule::None;
}

#[test]
fn preview_lists_pending_changes() {
    let (root, mut app) = app_with_job();
    fs::write(root.path().join("src/a.txt"), "oi").unwrap();
    app.on_key(key(KeyCode::Char('d')));
    wait_until(&mut app, preview_done);

    let Screen::Preview(preview) = &app.screen else { panic!("prévia deveria estar aberta") };
    assert_eq!(preview.count(rsync_tui::rsync::Change::Created), 1);
    assert!(!preview.can_save());
    assert!(!root.path().join("dst/a.txt").exists());
    assert!(screen_text(&mut app).contains("1 novo(s)"));

    app.on_key(key(KeyCode::Esc));
    assert!(matches!(app.screen, Screen::Dashboard));
}

#[test]
fn new_job_requires_preview_before_saving() {
    let (root, mut app) = app_with_job();
    fill_new_job(&mut app, root.path(), "dst");
    app.on_key(ctrl('s'));
    assert!(matches!(app.screen, Screen::Preview(_)));
    wait_until(&mut app, preview_done);

    app.on_key(key(KeyCode::Esc));
    assert!(matches!(app.screen, Screen::Form(_)));
    assert_eq!(app.rows.len(), 1);

    app.on_key(ctrl('s'));
    wait_until(&mut app, preview_done);
    app.on_key(key(KeyCode::Char('y')));
    if let Screen::Form(form) = &app.screen {
        panic!("save falhou: {:?}", form.error);
    }
    assert!(matches!(app.screen, Screen::Dashboard), "{:?}", app.screen);
    assert!(app.rows.iter().any(|r| r.job.name == "novo"));
}

#[test]
fn preview_error_returns_to_form() {
    let (root, mut app) = app_with_job();
    fill_new_job(&mut app, root.path(), "nao-existe");
    app.on_key(ctrl('s'));
    wait_until(&mut app, |app| !matches!(app.screen, Screen::Preview(_)));

    let Screen::Form(form) = &app.screen else { panic!("deveria voltar ao formulário") };
    assert!(form.error.as_deref().unwrap_or_default().contains("destino não existe"));
}

#[test]
fn run_copies_and_records_state() {
    let (root, mut app) = app_with_job();
    fs::write(root.path().join("src/a.txt"), "oi").unwrap();
    app.on_key(key(KeyCode::Char('r')));
    assert!(matches!(app.screen, Screen::Run(_)));
    wait_until(&mut app, run_done);

    assert_eq!(fs::read_to_string(root.path().join("dst/a.txt")).unwrap(), "oi");
    assert!(screen_text(&mut app).contains("concluído"));
    let last = app.rows[0].last_run.as_ref().expect("estado deveria ser gravado");
    assert_eq!(last.files_transferred, 1);

    app.on_key(key(KeyCode::Esc));
    assert!(matches!(app.screen, Screen::Dashboard));
}
