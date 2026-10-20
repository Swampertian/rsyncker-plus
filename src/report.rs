use std::path::PathBuf;

use anyhow::Result;
use serde::Serialize;

use crate::daemon::systemd::{self, UnitStatus};
use crate::job::{Job, Mode, Schedule};
use crate::manage;
use crate::paths::Paths;
use crate::remote::Side;
use crate::state::{Outcome, RunState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Running,
    DestinationMissing,
    Never,
    Success,
    Warning,
    Failed,
}

impl Status {
    pub fn of(job: &Job, last_run: Option<&RunState>, running: bool) -> Self {
        if running {
            return Status::Running;
        }
        if job.is_local(Side::Destination) && !job.destination.is_dir() {
            return Status::DestinationMissing;
        }
        match last_run.map(|run| run.outcome) {
            None => Status::Never,
            Some(Outcome::Success) => Status::Success,
            Some(Outcome::Warning) => Status::Warning,
            Some(Outcome::Failed) => Status::Failed,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Status::Running => "rodando",
            Status::DestinationMissing => "destino ausente",
            Status::Never => "nunca rodou",
            Status::Success => "ok",
            Status::Warning => "avisos",
            Status::Failed => "falhou",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct JobReport {
    pub name: String,
    pub source: String,
    pub destination: String,
    pub mode: Mode,
    pub schedule: Schedule,
    pub on_change: bool,
    pub enabled: bool,
    pub excludes: Vec<String>,
    pub bwlimit_kbps: Option<u32>,
    pub key: Option<PathBuf>,
    pub status: Status,
    pub next_run: Option<String>,
    pub last_run: Option<RunState>,
}

impl JobReport {
    pub fn new(job: &Job, last_run: Option<RunState>, unit: &UnitStatus) -> Self {
        Self {
            name: job.name.clone(),
            source: job.source_label(),
            destination: job.destination_label(),
            mode: job.mode,
            schedule: job.schedule,
            on_change: job.on_change,
            enabled: job.enabled,
            excludes: job.excludes.clone(),
            bwlimit_kbps: job.bwlimit_kbps,
            key: job.remote.as_ref().and_then(|remote| remote.credential()).map(PathBuf::from),
            status: Status::of(job, last_run.as_ref(), unit.running),
            next_run: if job.enabled { unit.next_run.clone() } else { None },
            last_run,
        }
    }
}

pub fn all(paths: &Paths) -> Result<Vec<JobReport>> {
    let entries = manage::list(paths)?;
    let names: Vec<String> = entries.iter().map(|entry| entry.job.name.clone()).collect();
    let units = systemd::status(&names);
    Ok(entries.into_iter().map(|entry| JobReport::new(&entry.job, entry.last_run, &units.get(&entry.job.name).cloned().unwrap_or_default())).collect())
}

pub fn one(paths: &Paths, name: &str) -> Result<JobReport> {
    let job = Job::load(&paths.jobs, name)?;
    let last_run = RunState::load(&paths.state, name)?;
    let units = systemd::status(&[name.to_string()]);
    Ok(JobReport::new(&job, last_run, &units.get(name).cloned().unwrap_or_default()))
}
