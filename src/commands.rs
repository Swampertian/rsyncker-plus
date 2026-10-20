use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::CommandFactory;
use serde::Serialize;
use serde_json::json;

use crate::cli::{Changes, Cli, Command, Confirm};
use crate::daemon::{runner, watch};
use crate::discover;
use crate::hostkey;
use crate::job::{self, Job, Mode, Schedule};
use crate::manage;
use crate::paths::{Paths, expand_home, shorten_home};
use crate::remote::{self, Endpoint, Protocol, Remote, Side};
use crate::report::{self, JobReport};
use crate::rsync::{self, Cancel, Change, Item, Options};
use crate::state::{self, Outcome, RunState};
use crate::tui;
use crate::tui::view::{format_bytes, mode_label, schedule_label};

pub fn execute(cli: Cli, paths: Paths, exe: PathBuf) -> Result<()> {
    let json = cli.json;
    let Some(command) = cli.command else {
        if io::stdin().is_terminal() && io::stdout().is_terminal() {
            return tui::run(paths, exe);
        }
        Cli::command().print_help()?;
        return Ok(());
    };
    match command {
        Command::Run { job, progress } => run(&paths, &job, progress, json),
        Command::Preview { job } => {
            let job = Job::load(&paths.jobs, &job)?;
            job.validate()?;
            preview(&job, &job.name, json)
        }
        Command::Watch { job } => watch::run(&paths, &job),
        Command::Add { source, to, mode, schedule, name, excludes, bwlimit, on_change, key, confirm } => {
            let source = remote::parse(&source).context("origem")?;
            let destination = remote::parse(&to).context("destino")?;
            let name = name.unwrap_or_else(|| job::name_from_path(source.path()));
            let key = key.map(|path| expand_home(&path.to_string_lossy()));
            let mut new = Job::from_endpoints(&name, source, destination, mode, key)?;
            new.schedule = schedule;
            new.excludes = excludes;
            new.bwlimit_kbps = bwlimit.filter(|&kbps| kbps > 0);
            new.on_change = on_change;
            save(&paths, &exe, new, None, &confirm, json)
        }
        Command::Edit { job, changes, confirm } => {
            let original = Job::load(&paths.jobs, &job)?;
            let edited = apply_changes(&original, &changes)?;
            save(&paths, &exe, edited, Some(&original), &confirm, json)
        }
        Command::Show { job } => show(&paths, &job, json),
        Command::List => list(&paths, json),
        Command::Remove { job, yes } => remove(&paths, &job, yes, json),
        Command::Enable { job } => set_enabled(&paths, &job, true, &exe, json),
        Command::Disable { job } => set_enabled(&paths, &job, false, &exe, json),
        Command::TrustHost { target, fingerprint } => trust_host(&paths, &target, fingerprint.as_deref(), json),
        Command::Discover => discover(json),
    }
}

pub fn apply_changes(original: &Job, changes: &Changes) -> Result<Job> {
    let source = remote::parse(&changes.source.clone().unwrap_or_else(|| original.source_label())).context("origem")?;
    let destination = remote::parse(&changes.to.clone().unwrap_or_else(|| original.destination_label())).context("destino")?;
    let both_local = matches!((&source, &destination), (Endpoint::Local(_), Endpoint::Local(_)));
    let key = match &changes.key {
        Some(key) => Some(expand_home(&key.to_string_lossy())),
        None if changes.no_key || both_local => None,
        None => original.remote.as_ref().and_then(|remote| remote.credential()).map(PathBuf::from),
    };
    let mut job = Job::from_endpoints(&original.name, source, destination, changes.mode.unwrap_or(original.mode), key)?;
    job.schedule = changes.schedule.unwrap_or(original.schedule);
    job.on_change = changes.on_change.unwrap_or(original.on_change);
    job.excludes = if changes.clear_excludes {
        Vec::new()
    } else if changes.excludes.is_empty() {
        original.excludes.clone()
    } else {
        changes.excludes.clone()
    };
    job.bwlimit_kbps = match changes.bwlimit {
        Some(0) => None,
        Some(kbps) => Some(kbps),
        None => original.bwlimit_kbps,
    };
    job.enabled = original.enabled;
    job.debounce_secs = original.debounce_secs;
    job.max_wait_secs = original.max_wait_secs;
    job.cooldown_secs = original.cooldown_secs;
    Ok(job)
}

pub fn needs_confirmation(job: &Job, original: Option<&Job>) -> bool {
    job.mode == Mode::Mirror && original.is_none_or(|original| original.mode != Mode::Mirror || !same_place(original, job))
}

pub fn needs_check(job: &Job, original: Option<&Job>) -> bool {
    original.is_none_or(|original| needs_confirmation(job, Some(original)) || !same_place(original, job) || original.remote != job.remote)
}

fn same_place(a: &Job, b: &Job) -> bool {
    let place = |job: &Job| job.remote.as_ref().map(|r| (r.side, r.protocol, r.host.clone(), r.user.clone(), r.port));
    a.source == b.source && a.destination == b.destination && place(a) == place(b)
}

fn save(paths: &Paths, exe: &Path, job: Job, original: Option<&Job>, confirm: &Confirm, json: bool) -> Result<()> {
    let job = manage::prepare(job)?;
    let target = remote_target(&job);
    if confirm.dry_run {
        return preview(&job, &target, json);
    }
    if needs_confirmation(&job, original) && !confirm.yes {
        bail!(
            "modo mirror apaga no destino o que não existe na origem ({} -> {}). Veja o efeito com --dry-run e repita com --yes para confirmar",
            job.source_label(),
            job.destination_label()
        );
    }
    if needs_check(&job, original) {
        verify(&job, &target)?;
    }
    let saved = match original {
        None => manage::add(paths, job, exe)?,
        Some(_) => manage::update(paths, job, exe)?,
    };
    if json {
        return print_json(&report::one(paths, &saved.name)?);
    }
    let verb = if original.is_none() { "criado" } else { "atualizado" };
    println!("job '{}' {verb}: {} -> {}", saved.name, saved.source_label(), saved.destination_label());
    Ok(())
}

fn remote_target(job: &Job) -> String {
    match &job.remote {
        Some(remote) if remote.side == Side::Source => job.source_label(),
        Some(_) => job.destination_label(),
        None => job.name.clone(),
    }
}

fn ensure_known_host(job: &Job, target: &str) -> Result<()> {
    if hostkey::check_job(job)?.is_some() {
        bail!(
            "chave do host SSH ainda não confirmada. Veja a impressão digital com `rsync-tui trust-host {target}`, confira com o dono do servidor e confirme com --fingerprint"
        );
    }
    Ok(())
}

fn verify(job: &Job, target: &str) -> Result<()> {
    ensure_known_host(job, target)?;
    let state = runner::dry_run(job.clone(), &Cancel::default(), |_| {})?;
    if state.outcome == Outcome::Failed {
        bail!("verificação (dry-run) falhou: {}", state.message.unwrap_or_default());
    }
    Ok(())
}

#[derive(Serialize)]
struct PreviewReport {
    job: String,
    source: String,
    destination: String,
    mode: Mode,
    created: Vec<String>,
    updated: Vec<String>,
    deleted: Vec<String>,
    outcome: Outcome,
    message: Option<String>,
}

fn preview(job: &Job, target: &str, json: bool) -> Result<()> {
    ensure_known_host(job, target)?;
    let mut items: Vec<Item> = Vec::new();
    let state = runner::dry_run(job.clone(), &Cancel::default(), |line| items.extend(rsync::parse_item(line)))?;
    let paths_of = |change: Change| items.iter().filter(|item| item.change == change).map(|item| item.path.clone()).collect::<Vec<_>>();
    let report = PreviewReport {
        job: job.name.clone(),
        source: job.source_label(),
        destination: job.destination_label(),
        mode: job.mode,
        created: paths_of(Change::Created),
        updated: paths_of(Change::Updated),
        deleted: paths_of(Change::Deleted),
        outcome: state.outcome,
        message: state.message.clone(),
    };
    if json {
        print_json(&report)?;
    } else {
        println!("prévia de '{}': {} -> {}", report.job, report.source, report.destination);
        for item in &items {
            println!("{} {}", mark(item.change), item.path);
        }
        if items.is_empty() && state.outcome != Outcome::Failed {
            println!("nada a fazer: o destino já está em dia");
        }
        println!("{} novo(s), {} alterado(s), {} apagado(s)", report.created.len(), report.updated.len(), report.deleted.len());
        if !report.deleted.is_empty() {
            println!("atenção: modo mirror; os itens marcados com - serão APAGADOS no destino");
        }
    }
    if state.outcome == Outcome::Failed {
        bail!("prévia falhou: {}", state.message.unwrap_or_default());
    }
    Ok(())
}

fn mark(change: Change) -> &'static str {
    match change {
        Change::Created => "+",
        Change::Updated => "~",
        Change::Deleted => "-",
    }
}

#[derive(Serialize)]
struct RunReport<'a> {
    job: &'a str,
    #[serde(flatten)]
    state: &'a RunState,
}

fn run(paths: &Paths, name: &str, progress: bool, json: bool) -> Result<()> {
    let progress = progress && io::stderr().is_terminal();
    let cancel = Cancel::default();
    let state = runner::run_with(paths, name, Options { dry_run: false, progress }, |args| {
        rsync::execute_streaming(args, &cancel, |line| show_line(line, progress, json))
    })?;
    if progress {
        eprintln!();
    }
    if json {
        print_json(&RunReport { job: name, state: &state })?;
    } else {
        let totals = format!("{} arquivo(s), {} copiados, {} apagado(s)", state.files_transferred, format_bytes(state.bytes_transferred), state.files_deleted);
        match state.outcome {
            Outcome::Success => println!("concluído: {totals}"),
            Outcome::Warning => println!("concluído com avisos ({}): {totals}", state.message.clone().unwrap_or_default()),
            Outcome::Failed => {}
        }
    }
    if state.outcome == Outcome::Failed {
        bail!("job '{name}' falhou: {}", state.message.unwrap_or_default());
    }
    Ok(())
}

fn show_line(line: &str, progress: bool, json: bool) {
    if let Some(update) = rsync::parse_progress(line) {
        if progress {
            eprint!("\r{:>3}%  {}  {}  {} restante   ", update.percent, format_bytes(update.bytes), update.rate, update.eta);
        }
        return;
    }
    if json {
        return;
    }
    if let Some(item) = rsync::parse_item(line) {
        println!("{} {}", mark(item.change), item.path);
    }
}

fn show(paths: &Paths, name: &str, json: bool) -> Result<()> {
    let report = report::one(paths, name)?;
    if json {
        return print_json(&report);
    }
    let yes_no = |value: bool| if value { "sim" } else { "não" };
    println!("nome:        {}", report.name);
    println!("origem:      {}", report.source);
    println!("destino:     {}", report.destination);
    println!("modo:        {} ({})", mode_name(report.mode), mode_label(report.mode));
    println!("agenda:      {} ({})", schedule_name(report.schedule), schedule_label(report.schedule));
    println!("ao alterar:  {}", yes_no(report.on_change));
    println!("ativo:       {}", yes_no(report.enabled));
    println!("ignorar:     {}", if report.excludes.is_empty() { "-".to_string() } else { report.excludes.join(", ") });
    println!("banda:       {}", report.bwlimit_kbps.map_or("sem limite".to_string(), |kbps| format!("{kbps} KB/s")));
    println!("chave/senha: {}", report.key.as_deref().map_or("-".to_string(), shorten_home));
    println!("estado:      {}", report.status.label());
    println!("último run:  {}", last_run_text(report.last_run.as_ref()));
    println!("próximo run: {}", next_run_text(&report));
    Ok(())
}

fn list(paths: &Paths, json: bool) -> Result<()> {
    let reports = report::all(paths)?;
    if json {
        return print_json(&reports);
    }
    if reports.is_empty() {
        println!("nenhum job. crie com: rsync-tui add <origem> --to <destino>");
        return Ok(());
    }
    for report in &reports {
        println!(
            "{}  [{}]  {}/{}{}  {} -> {}\n    último: {}  próximo: {}",
            report.name,
            report.status.label(),
            mode_name(report.mode),
            schedule_name(report.schedule),
            if report.on_change { "+alteração" } else { "" },
            report.source,
            report.destination,
            last_run_text(report.last_run.as_ref()),
            next_run_text(report)
        );
    }
    Ok(())
}

fn last_run_text(last_run: Option<&RunState>) -> String {
    let Some(run) = last_run else {
        return "nunca rodou".to_string();
    };
    let outcome = match run.outcome {
        Outcome::Success => "ok",
        Outcome::Warning => "avisos",
        Outcome::Failed => "falhou",
    };
    let ago = state::format_ago(run.finished_at, state::now());
    match &run.message {
        Some(message) if run.outcome != Outcome::Success => format!("{outcome} {ago}: {message}"),
        _ => format!("{outcome} {ago}"),
    }
}

fn next_run_text(report: &JobReport) -> String {
    match (&report.next_run, report.enabled) {
        (_, false) => "desativado".to_string(),
        (Some(next), true) => next.clone(),
        (None, true) => "-".to_string(),
    }
}

fn mode_name(mode: Mode) -> &'static str {
    match mode {
        Mode::Copy => "copy",
        Mode::Mirror => "mirror",
    }
}

fn schedule_name(schedule: Schedule) -> &'static str {
    match schedule {
        Schedule::None => "none",
        Schedule::Hourly => "hourly",
        Schedule::Daily => "daily",
        Schedule::Weekly => "weekly",
    }
}

fn remove(paths: &Paths, name: &str, yes: bool, json: bool) -> Result<()> {
    let job = Job::load(&paths.jobs, name)?;
    if !yes {
        bail!(
            "remover '{name}' apaga a configuração e os timers ({} -> {}); os arquivos no destino ficam. Repita com --yes para confirmar",
            job.source_label(),
            job.destination_label()
        );
    }
    manage::remove(paths, name)?;
    if json {
        return print_json(&json!({ "job": name, "removed": true }));
    }
    println!("job '{name}' removido");
    Ok(())
}

fn set_enabled(paths: &Paths, name: &str, enabled: bool, exe: &Path, json: bool) -> Result<()> {
    manage::set_enabled(paths, name, enabled, exe)?;
    if json {
        return print_json(&report::one(paths, name)?);
    }
    println!("job '{name}' {}", if enabled { "ativado" } else { "desativado" });
    Ok(())
}

#[derive(Serialize)]
struct HostReport {
    host: String,
    trusted: bool,
    fingerprints: Vec<String>,
    known_hosts: Option<PathBuf>,
}

fn trust_host(paths: &Paths, target: &str, fingerprint: Option<&str>, json: bool) -> Result<()> {
    let remote = host_of(paths, target)?;
    let Some(unknown) = hostkey::check(&remote)? else {
        let host = match remote.port {
            Some(port) => format!("{}:{port}", remote.host),
            None => remote.host.clone(),
        };
        if json {
            return print_json(&HostReport { host, trusted: true, fingerprints: Vec::new(), known_hosts: None });
        }
        println!("host {host} já é confiável");
        return Ok(());
    };
    match fingerprint {
        Some(fingerprint) if hostkey::matches(&unknown, fingerprint) => {
            hostkey::trust(&unknown)?;
            if json {
                return print_json(&HostReport {
                    host: unknown.host.clone(),
                    trusted: true,
                    fingerprints: unknown.fingerprints.clone(),
                    known_hosts: Some(unknown.file.clone()),
                });
            }
            println!("chave de {} gravada em {}", unknown.host, shorten_home(&unknown.file));
            Ok(())
        }
        Some(fingerprint) => {
            bail!("impressão digital não confere: informada {fingerprint}, servidor apresentou {}. Nada foi gravado", unknown.fingerprints.join(", "))
        }
        None => {
            if json {
                print_json(&HostReport {
                    host: unknown.host.clone(),
                    trusted: false,
                    fingerprints: unknown.fingerprints.clone(),
                    known_hosts: Some(unknown.file.clone()),
                })?;
            } else {
                println!("primeira conexão com {}", unknown.host);
                println!("impressão digital da chave do servidor:");
                for fingerprint in &unknown.fingerprints {
                    println!("  {fingerprint}");
                }
                println!("confira no próprio servidor: ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub");
            }
            bail!("chave ainda não confirmada: confira a impressão digital e repita com --fingerprint <SHA256:...>")
        }
    }
}

fn host_of(paths: &Paths, target: &str) -> Result<Remote> {
    let remote = if job::validate_name(target).is_ok() && Job::file(&paths.jobs, target).exists() {
        Job::load(&paths.jobs, target)?.remote.with_context(|| format!("job '{target}' é local: não há host para confirmar"))?
    } else {
        match remote::parse(target)? {
            Endpoint::Remote(address) => address.into_remote(Side::Destination, None),
            Endpoint::Local(_) => bail!("'{target}' não é job existente nem endereço remoto (usuário@host:/caminho)"),
        }
    };
    if remote.protocol != Protocol::Ssh {
        bail!("rsync:// não usa chave de host: nada a confirmar");
    }
    Ok(remote)
}

#[derive(Serialize)]
struct DeviceReport {
    name: String,
    host: String,
    address: Option<String>,
    protocol: Protocol,
    port: Option<u16>,
    endpoint: String,
    from_ssh_config: bool,
}

#[derive(Serialize)]
struct DiscoverReport {
    devices: Vec<DeviceReport>,
    warning: Option<String>,
}

fn discover(json: bool) -> Result<()> {
    let found = discover::discover();
    let report = DiscoverReport {
        devices: found
            .devices
            .iter()
            .map(|device| DeviceReport {
                name: device.name.clone(),
                host: device.host.clone(),
                address: (!device.address.is_empty()).then(|| device.address.clone()),
                protocol: device.protocol,
                port: device.port,
                endpoint: device.endpoint(),
                from_ssh_config: device.from_config,
            })
            .collect(),
        warning: found.warning,
    };
    if json {
        return print_json(&report);
    }
    if let Some(warning) = &report.warning {
        eprintln!("aviso: {warning}");
    }
    if report.devices.is_empty() {
        println!("nenhum dispositivo encontrado");
        return Ok(());
    }
    for device in &report.devices {
        let origin = if device.from_ssh_config { "~/.ssh/config".to_string() } else { device.address.clone().unwrap_or_default() };
        println!("{:<32} {}  ({origin})", device.endpoint, device.name);
    }
    println!("use como origem/destino completando o caminho, ex.: host:/pasta ou rsync://host/módulo");
    Ok(())
}

fn print_json(value: &impl Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
