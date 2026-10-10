use std::ffi::OsString;
use std::process::Command;

use anyhow::{Context, Result};

use crate::job::{Job, Mode};
use crate::state::Outcome;

#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    pub dry_run: bool,
    pub progress: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    pub files_transferred: u64,
    pub files_created: u64,
    pub files_deleted: u64,
    pub bytes_transferred: u64,
}

#[derive(Debug, Clone)]
pub struct Output {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

pub fn build_args(job: &Job, opts: Options) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["-a", "--partial", "--stats", "--no-human-readable", "--itemize-changes"].into_iter().map(OsString::from).collect();
    if job.mode == Mode::Mirror {
        args.push("--delete".into());
    }
    for pattern in &job.excludes {
        args.push(format!("--exclude={pattern}").into());
    }
    if let Some(kbps) = job.bwlimit_kbps {
        args.push(format!("--bwlimit={kbps}").into());
    }
    if opts.dry_run {
        args.push("--dry-run".into());
    }
    if opts.progress {
        args.push("--info=progress2".into());
    }
    let mut source = job.source.clone().into_os_string();
    if !source.to_string_lossy().ends_with('/') {
        source.push("/");
    }
    args.push(source);
    args.push(job.destination.clone().into_os_string());
    args
}

pub fn execute(args: &[OsString]) -> Result<Output> {
    let out = Command::new("rsync").args(args).output().context("não foi possível executar rsync (está instalado?)")?;
    Ok(Output {
        exit_code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
}

pub fn parse_stats(stdout: &str) -> Stats {
    let mut stats = Stats::default();
    for line in stdout.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let number = leading_number(value);
        match key.trim() {
            "Number of regular files transferred" => stats.files_transferred = number,
            "Number of created files" => stats.files_created = number,
            "Number of deleted files" => stats.files_deleted = number,
            "Total transferred file size" => stats.bytes_transferred = number,
            _ => {}
        }
    }
    stats
}

fn leading_number(text: &str) -> u64 {
    let mut number = 0_u64;
    let mut found_digit = false;
    for byte in text.trim().bytes() {
        match byte {
            b'0'..=b'9' => {
                found_digit = true;
                let Some(next) = number.checked_mul(10).and_then(|value| value.checked_add(u64::from(byte - b'0'))) else {
                    return 0;
                };
                number = next;
            }
            b',' => {}
            _ => break,
        }
    }
    if found_digit { number } else { 0 }
}

pub fn outcome(exit_code: i32) -> Outcome {
    match exit_code {
        0 => Outcome::Success,
        24 => Outcome::Warning,
        _ => Outcome::Failed,
    }
}
