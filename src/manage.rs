use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::daemon::systemd;
use crate::job::Job;
use crate::paths::Paths;
use crate::remote::Side;
use crate::state::RunState;

pub fn prepare(mut job: Job) -> Result<Job> {
    if job.is_local(Side::Source) {
        job.source = fs::canonicalize(&job.source).with_context(|| format!("origem não existe: {}", job.source.display()))?;
        if !job.source.is_dir() {
            bail!("origem não é diretório: {}", job.source.display());
        }
    }
    if job.is_local(Side::Destination) {
        job.destination =
            fs::canonicalize(&job.destination).with_context(|| format!("destino não existe: {} (crie a pasta antes)", job.destination.display()))?;
        if !job.destination.is_dir() {
            bail!("destino não é diretório: {}", job.destination.display());
        }
    }
    job.validate()?;
    Ok(job)
}

pub fn add(paths: &Paths, job: Job, exe: &Path) -> Result<Job> {
    let job = prepare(job)?;
    job.save_new(&paths.jobs)?;
    if let Err(error) = systemd::write_units(paths, &job, exe).and_then(|()| systemd::apply(&job)) {
        let _ = systemd::deactivate(&job.name);
        let _ = systemd::remove_units(paths, &job.name);
        let _ = Job::delete(&paths.jobs, &job.name);
        return Err(error).with_context(|| format!("não foi possível configurar job '{}'", job.name));
    }
    Ok(job)
}

pub fn remove(paths: &Paths, name: &str) -> Result<()> {
    let _lock = crate::daemon::runner::acquire_lock(paths, name)?;
    Job::load(&paths.jobs, name)?;
    systemd::deactivate(name)?;
    systemd::remove_units(paths, name)?;
    Job::delete(&paths.jobs, name)?;
    RunState::delete(&paths.state, name)?;
    let _ = fs::remove_file(paths.state.join(format!("{name}.lock")));
    Ok(())
}

pub fn set_enabled(paths: &Paths, name: &str, enabled: bool, exe: &Path) -> Result<Job> {
    let mut job = Job::load(&paths.jobs, name)?;
    let previous = job.clone();
    job.enabled = enabled;
    job.save(&paths.jobs)?;
    if let Err(error) = systemd::write_units(paths, &job, exe).and_then(|()| systemd::apply(&job)) {
        let _ = previous.save(&paths.jobs);
        let _ = systemd::write_units(paths, &previous, exe);
        let _ = systemd::apply(&previous);
        return Err(error).with_context(|| format!("não foi possível atualizar job '{name}'"));
    }
    Ok(job)
}

pub fn update(paths: &Paths, job: Job, exe: &Path) -> Result<Job> {
    let previous = Job::load(&paths.jobs, &job.name)?;
    let job = prepare(job)?;
    job.save(&paths.jobs)?;
    if let Err(error) = systemd::write_units(paths, &job, exe).and_then(|()| systemd::apply(&job)) {
        let _ = previous.save(&paths.jobs);
        let _ = systemd::write_units(paths, &previous, exe);
        let _ = systemd::apply(&previous);
        return Err(error).with_context(|| format!("não foi possível atualizar job '{}'", job.name));
    }
    Ok(job)
}

pub struct Entry {
    pub job: Job,
    pub last_run: Option<RunState>,
}

pub fn list(paths: &Paths) -> Result<Vec<Entry>> {
    Job::list(&paths.jobs)?
        .into_iter()
        .map(|job| {
            let last_run = RunState::load(&paths.state, &job.name)?;
            Ok(Entry { job, last_run })
        })
        .collect()
}
