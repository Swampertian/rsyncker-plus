use std::env;

use anyhow::{Result, bail};
use clap::Parser;
use rsync_tui::cli::{Cli, Command};
use rsync_tui::daemon::{runner, systemd};
use rsync_tui::job::{self, Job};
use rsync_tui::manage;
use rsync_tui::paths::Paths;
use rsync_tui::state::{self, Outcome};

fn main() -> Result<()> {
    let cli = Cli::parse();
    let paths = Paths::from_env()?;
    let exe = env::current_exe()?;

    match cli.command {
        None => rsync_tui::tui::run(paths, exe)?,
        Some(Command::Run { job }) => {
            let result = runner::run(&paths, &job)?;
            if result.outcome == Outcome::Failed {
                bail!("job '{job}' falhou: {}", result.message.unwrap_or_default());
            }
        }
        Some(Command::Watch { job }) => bail!("watch '{job}' ainda não implementado"),
        Some(Command::Add { source, to, mode, schedule, name, excludes, bwlimit }) => {
            let name = name.unwrap_or_else(|| job::name_from_path(&source));
            let mut new = Job::new(&name, source, to, mode);
            new.schedule = schedule;
            new.excludes = excludes;
            new.bwlimit_kbps = bwlimit;
            let created = manage::add(&paths, new, &exe)?;
            println!("job '{}' criado: {} -> {}", created.name, created.source.display(), created.destination.display());
        }
        Some(Command::List) => print_list(&paths)?,
        Some(Command::Remove { job }) => {
            manage::remove(&paths, &job)?;
            println!("job '{job}' removido");
        }
        Some(Command::Enable { job }) => {
            manage::set_enabled(&paths, &job, true, &exe)?;
            println!("job '{job}' ativado");
        }
        Some(Command::Disable { job }) => {
            manage::set_enabled(&paths, &job, false, &exe)?;
            println!("job '{job}' desativado");
        }
    }
    Ok(())
}

fn print_list(paths: &Paths) -> Result<()> {
    let entries = manage::list(paths)?;
    if entries.is_empty() {
        println!("nenhum job. crie com: rsync-tui add <origem> --to <destino>");
        return Ok(());
    }
    let now = state::now();
    for entry in entries {
        let job = entry.job;
        let last = match entry.last_run {
            Some(run) => format!("{:?} {}", run.outcome, state::format_ago(run.finished_at, now)),
            None => "nunca rodou".to_string(),
        };
        let next = if job.enabled { systemd::next_run(&job.name).unwrap_or_else(|| "-".to_string()) } else { "desativado".to_string() };
        println!(
            "{}  [{:?}/{:?}]  {} -> {}\n    último: {}  próximo: {}",
            job.name,
            job.mode,
            job.schedule,
            job.source.display(),
            job.destination.display(),
            last,
            next
        );
    }
    Ok(())
}
