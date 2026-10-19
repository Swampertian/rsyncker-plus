use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use notify::{Event, EventKind, RecursiveMode, Watcher};

use crate::daemon::systemd;
use crate::job::Job;
use crate::paths::Paths;
use crate::state::{self, Outcome, RunState};

#[derive(Debug, Clone)]
pub struct Debounce {
    quiet: Duration,
    max_wait: Duration,
    cooldown: Duration,
    first: Option<Instant>,
    last: Option<Instant>,
    fired: Option<Instant>,
}

impl Debounce {
    pub fn new(quiet: Duration, max_wait: Duration, cooldown: Duration) -> Self {
        Self { quiet, max_wait, cooldown, first: None, last: None, fired: None }
    }

    pub fn for_job(job: &Job) -> Self {
        Self::new(Duration::from_secs(job.debounce_secs), Duration::from_secs(job.max_wait_secs), Duration::from_secs(job.cooldown_secs))
    }

    pub fn record(&mut self, now: Instant) {
        self.first.get_or_insert(now);
        self.last = Some(now);
    }

    pub fn deadline(&self) -> Option<Instant> {
        let ready = (self.last? + self.quiet).min(self.first? + self.max_wait);
        Some(match self.fired {
            Some(fired) => ready.max(fired + self.cooldown),
            None => ready,
        })
    }

    pub fn fire_if_due(&mut self, now: Instant) -> bool {
        match self.deadline() {
            Some(deadline) if now >= deadline => {
                self.first = None;
                self.last = None;
                self.fired = Some(now);
                true
            }
            _ => false,
        }
    }
}

pub fn ignored(source: &Path, path: &Path, excludes: &[String]) -> bool {
    let Ok(relative) = path.strip_prefix(source) else {
        return false;
    };
    let parts: Vec<Vec<char>> = relative.components().map(|c| c.as_os_str().to_string_lossy().chars().collect()).collect();
    excludes.iter().any(|pattern| {
        let pattern = pattern.trim_end_matches('/');
        let (anchored, pattern) = match pattern.strip_prefix('/') {
            Some(rest) => (true, rest),
            None => (false, pattern),
        };
        let pieces: Vec<Vec<char>> = pattern.split('/').map(|p| p.chars().collect()).collect();
        if pieces.is_empty() || pieces.len() > parts.len() {
            return false;
        }
        let starts = if anchored { 0..1 } else { 0..parts.len() - pieces.len() + 1 };
        starts.into_iter().any(|start| pieces.iter().zip(&parts[start..]).all(|(p, s)| glob(p, s)))
    })
}

fn glob(pattern: &[char], text: &[char]) -> bool {
    match (pattern.first(), text.first()) {
        (None, None) => true,
        (Some('*'), _) => glob(&pattern[1..], text) || (!text.is_empty() && glob(pattern, &text[1..])),
        (Some('?'), Some(_)) => glob(&pattern[1..], &text[1..]),
        (Some(p), Some(t)) if p == t => glob(&pattern[1..], &text[1..]),
        _ => false,
    }
}

pub fn relevant(job: &Job, event: &Event) -> bool {
    if matches!(event.kind, EventKind::Access(_)) {
        return false;
    }
    event.paths.is_empty() || event.paths.iter().any(|path| !ignored(&job.source, path, &job.excludes))
}

pub fn run(paths: &Paths, name: &str) -> Result<()> {
    let job = Job::load(&paths.jobs, name)?;
    job.validate()?;
    if !job.on_change {
        bail!("job '{name}' não tem gatilho por alteração (on_change = false)");
    }
    let result = watch(&job, || {
        println!("alteração detectada, iniciando job '{name}'");
        if let Err(error) = systemd::start(name) {
            eprintln!("{error:#}");
        }
        true
    });
    if let Err(error) = &result {
        record_failure(paths, name, error);
    }
    result
}

pub fn watch(job: &Job, mut trigger: impl FnMut() -> bool) -> Result<()> {
    let identity = source_identity(&job.source).with_context(|| format!("origem não existe: {}", job.source.display()))?;
    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(tx).context("não foi possível iniciar o watcher")?;
    watcher.watch(&job.source, RecursiveMode::Recursive).map_err(explain)?;
    println!("observando {}", job.source.display());

    let mut debounce = Debounce::for_job(job);
    loop {
        let check = Instant::now() + Duration::from_secs(60);
        let wake = debounce.deadline().map_or(check, |deadline| deadline.min(check));
        let event = rx.recv_timeout(wake.saturating_duration_since(Instant::now()));
        if source_identity(&job.source).ok() != Some(identity) {
            bail!("origem removida, movida ou desmontada: {}", job.source.display());
        }
        match event {
            Ok(Ok(event)) => {
                if relevant(job, &event) {
                    debounce.record(Instant::now());
                }
            }
            Ok(Err(error)) => return Err(explain(error)),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => bail!("watcher encerrado inesperadamente"),
        }
        if debounce.fire_if_due(Instant::now()) && !trigger() {
            return Ok(());
        }
    }
}

fn source_identity(source: &Path) -> Result<(u64, u64)> {
    let metadata = fs::metadata(source)?;
    if !metadata.is_dir() {
        bail!("origem não é diretório: {}", source.display());
    }
    Ok((metadata.dev(), metadata.ino()))
}

fn explain(error: notify::Error) -> anyhow::Error {
    let too_many = match &error.kind {
        notify::ErrorKind::MaxFilesWatch => true,
        notify::ErrorKind::Io(io) => io.raw_os_error() == Some(28),
        _ => false,
    };
    if too_many {
        anyhow!("limite de inotify atingido: aumente fs.inotify.max_user_watches (ex.: sysctl fs.inotify.max_user_watches=524288)")
    } else {
        anyhow!("erro no watcher: {error}")
    }
}

fn record_failure(paths: &Paths, name: &str, error: &anyhow::Error) {
    let now = state::now();
    let state = RunState {
        started_at: now,
        finished_at: now,
        exit_code: -1,
        outcome: Outcome::Failed,
        files_transferred: 0,
        files_deleted: 0,
        bytes_transferred: 0,
        message: Some(format!("watcher: {error}")),
    };
    let _ = state.save(&paths.state, name);
}
