use std::fs::{self, File, OpenOptions};
use std::io::ErrorKind;

use anyhow::{Context, Result, bail};
use fs2::FileExt;

use crate::job::{Job, validate_name};
use crate::paths::Paths;
use crate::rsync::{self, Options};
use crate::state::{self, Outcome, RunState};

pub fn run(paths: &Paths, name: &str) -> Result<RunState> {
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
        Ok(()) => {
            let output = rsync::execute(&rsync::build_args(&job, Options::default()))?;
            print!("{}", output.stdout);
            eprint!("{}", output.stderr);
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
                message: (outcome != Outcome::Success).then(|| output.stderr.lines().last().unwrap_or_default().to_string()),
            }
        }
    };

    state.save(&paths.state, name)?;
    Ok(state)
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

fn preflight(job: &mut Job) -> Result<()> {
    if !job.source.is_dir() {
        bail!("origem não existe: {}", job.source.display());
    }
    if !job.destination.is_dir() {
        bail!("destino não existe (disco desmontado?): {}", job.destination.display());
    }
    let source = fs::canonicalize(&job.source).with_context(|| format!("não foi possível resolver origem: {}", job.source.display()))?;
    let destination = fs::canonicalize(&job.destination).with_context(|| format!("não foi possível resolver destino: {}", job.destination.display()))?;
    if source.starts_with(&destination) || destination.starts_with(&source) {
        bail!("origem e destino não podem ser iguais nem um dentro do outro após resolver links simbólicos");
    }
    job.source = source;
    job.destination = destination;
    fs::read_dir(&job.source).context("sem permissão de leitura na origem")?;
    Ok(())
}
