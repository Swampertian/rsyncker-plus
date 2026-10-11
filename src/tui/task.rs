use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;

use anyhow::Result;

use crate::daemon::runner;
use crate::job::Job;
use crate::paths::Paths;
use crate::rsync::{self, Cancel, Options};
use crate::state::{self, RunState};

#[derive(Debug, Clone, PartialEq)]
pub enum Update {
    Line(String),
    Finished(Result<RunState, String>),
}

#[derive(Debug)]
pub struct Task {
    updates: Receiver<Update>,
    cancel: Cancel,
}

impl Task {
    pub fn preview(job: Job) -> Self {
        Self::spawn(move |cancel, sender| {
            let mut job = job;
            runner::preflight(&mut job)?;
            let started_at = state::now();
            let args = rsync::build_args(&job, Options { dry_run: true, progress: false });
            let output = rsync::execute_streaming(&args, &cancel, |line| send_line(&sender, line))?;
            Ok(runner::finish(started_at, &output))
        })
    }

    pub fn run(paths: Paths, name: String) -> Self {
        Self::spawn(move |cancel, sender| {
            runner::run_with(&paths, &name, Options { dry_run: false, progress: true }, |args| {
                rsync::execute_streaming(args, &cancel, |line| send_line(&sender, line))
            })
        })
    }

    fn spawn(work: impl FnOnce(Cancel, Sender<Update>) -> Result<RunState> + Send + 'static) -> Self {
        let (sender, updates) = mpsc::channel();
        let cancel = Cancel::default();
        let worker_cancel = cancel.clone();
        thread::spawn(move || {
            let result = work(worker_cancel, sender.clone()).map_err(|error| format!("{error:#}"));
            let _ = sender.send(Update::Finished(result));
        });
        Self { updates, cancel }
    }

    pub fn updates(&self) -> Vec<Update> {
        let mut received = Vec::new();
        loop {
            match self.updates.try_recv() {
                Ok(update @ Update::Finished(_)) => {
                    received.push(update);
                    break;
                }
                Ok(update) => received.push(update),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    received.push(Update::Finished(Err("tarefa interrompida".to_string())));
                    break;
                }
            }
        }
        received
    }

    pub fn cancel(&self) {
        self.cancel.cancel();
    }
}

fn send_line(sender: &Sender<Update>, line: &str) {
    let _ = sender.send(Update::Line(line.to_string()));
}
