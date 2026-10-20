use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::ErrorKind;

use anyhow::{Context, Result, bail};
use fs2::FileExt;

use crate::job::{Job, validate_name};
use crate::paths::Paths;
use crate::remote::{self, Side};
use crate::rsync::{self, Cancel, Options, Output};
use crate::state::{self, Outcome, RunState};

pub fn run(paths: &Paths, name: &str) -> Result<RunState> {
    run_with(paths, name, Options::default(), |args| {
        let output = rsync::execute(args)?;
        print!("{}", output.stdout);
        eprint!("{}", output.stderr);
        Ok(output)
    })
}

pub fn run_with(paths: &Paths, name: &str, opts: Options, execute: impl FnOnce(&[OsString]) -> Result<Output>) -> Result<RunState> {
    let _lock = acquire_lock(paths, name)?;
    let mut job = Job::load(&paths.jobs, name)?;
    job.validate()?;

    let started_at = state::now();
    let state = match preflight(&mut job) {
        Err(err) => RunState {
            started_at,
            finished_at: state::now(),
            exit_code: -1,
            outcome: Outcome::Failed,
            files_transferred: 0,
            files_deleted: 0,
            bytes_transferred: 0,
            message: Some(err.to_string()),
        },
        Ok(()) => finish(started_at, &execute(&rsync::build_args(&job, opts))?),
    };

    state.save(&paths.state, name)?;
    Ok(state)
}

pub fn dry_run(mut job: Job, cancel: &Cancel, on_line: impl FnMut(&str)) -> Result<RunState> {
    preflight(&mut job)?;
    let started_at = state::now();
    let args = rsync::build_args(&job, Options { dry_run: true, progress: false });
    let output = rsync::execute_streaming(&args, cancel, on_line)?;
    Ok(finish(started_at, &output))
}

pub fn finish(started_at: u64, output: &Output) -> RunState {
    let stats = rsync::parse_stats(&output.stdout);
    let outcome = rsync::outcome(output.exit_code);
    RunState {
        started_at,
        finished_at: state::now(),
        exit_code: output.exit_code,
        outcome,
        files_transferred: stats.files_transferred,
        files_deleted: stats.files_deleted,
        bytes_transferred: stats.bytes_transferred,
        message: (outcome != Outcome::Success).then(|| remote::explain(&output.stderr).unwrap_or_else(|| remote::last_line(&output.stderr))),
    }
}

pub fn acquire_lock(paths: &Paths, name: &str) -> Result<File> {
    validate_name(name)?;
    fs::create_dir_all(&paths.state)?;
    let lock = OpenOptions::new().create(true).truncate(false).read(true).write(true).open(paths.state.join(format!("{name}.lock")))?;
    if let Err(error) = lock.try_lock_exclusive() {
        if error.kind() == ErrorKind::WouldBlock {
            bail!("job '{name}' já está em execução");
        }
        return Err(error).context("não foi possível obter lock do job");
    }
    Ok(lock)
}

pub fn preflight(job: &mut Job) -> Result<()> {
    if job.is_local(Side::Source) {
        if !job.source.is_dir() {
            bail!("origem não existe: {}", job.source.display());
        }
        job.source = fs::canonicalize(&job.source).with_context(|| format!("não foi possível resolver origem: {}", job.source.display()))?;
        fs::read_dir(&job.source).context("sem permissão de leitura na origem")?;
    }
    if job.is_local(Side::Destination) {
        if !job.destination.is_dir() {
            bail!("destino não existe (disco desmontado?): {}", job.destination.display());
        }
        job.destination = fs::canonicalize(&job.destination).with_context(|| format!("não foi possível resolver destino: {}", job.destination.display()))?;
    }
    match &job.remote {
        None if job.source.starts_with(&job.destination) || job.destination.starts_with(&job.source) => {
            bail!("origem e destino não podem ser iguais nem um dentro do outro após resolver links simbólicos")
        }
        None => {}
        Some(remote) => {
            let path = if remote.side == Side::Source { &job.source } else { &job.destination };
            remote::check_dir(remote, path)?;
        }
    }
    Ok(())
}
