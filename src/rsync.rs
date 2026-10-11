use std::ffi::OsString;
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

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
        args.push("--no-inc-recursive".into());
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

#[derive(Debug, Default)]
struct CancelState {
    cancelled: bool,
    child: Option<Child>,
}

#[derive(Debug, Clone, Default)]
pub struct Cancel {
    state: Arc<Mutex<CancelState>>,
}

impl Cancel {
    pub fn cancel(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.cancelled = true;
        if let Some(child) = state.child.as_mut() {
            let _ = child.kill();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.state.lock().unwrap_or_else(PoisonError::into_inner).cancelled
    }

    fn hold(&self, mut child: Child) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.cancelled {
            let _ = child.kill();
        }
        state.child = Some(child);
    }

    fn release(&self) -> Option<Child> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner).child.take()
    }
}

pub fn execute_streaming(args: &[OsString], cancel: &Cancel, mut on_line: impl FnMut(&str)) -> Result<Output> {
    let mut child = Command::new("rsync")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("não foi possível executar rsync (está instalado?)")?;
    let mut stdout = child.stdout.take().context("sem saída do rsync")?;
    let mut stderr = child.stderr.take().context("sem saída de erro do rsync")?;
    cancel.hold(child);

    let errors = thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    });

    let mut collected = String::new();
    let mut line = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = match stdout.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        for &byte in &buffer[..read] {
            if byte == b'\n' || byte == b'\r' {
                emit_line(&mut line, &mut collected, &mut on_line);
            } else {
                line.push(byte);
            }
        }
    }
    emit_line(&mut line, &mut collected, &mut on_line);

    let mut stderr = errors.join().unwrap_or_default();
    let status = match cancel.release() {
        Some(mut child) => Some(child.wait().context("falha ao aguardar rsync")?),
        None => None,
    };
    if cancel.is_cancelled() {
        stderr.push_str("cancelado pelo usuário\n");
    }
    Ok(Output { exit_code: status.and_then(|s| s.code()).unwrap_or(-1), stdout: collected, stderr })
}

fn emit_line(line: &mut Vec<u8>, collected: &mut String, on_line: &mut impl FnMut(&str)) {
    if line.is_empty() {
        return;
    }
    let text = String::from_utf8_lossy(line).into_owned();
    on_line(&text);
    collected.push_str(&text);
    collected.push('\n');
    line.clear();
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Created,
    Updated,
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub change: Change,
    pub path: String,
}

pub fn parse_item(line: &str) -> Option<Item> {
    if let Some(path) = line.strip_prefix("*deleting") {
        return Some(Item { change: Change::Deleted, path: path.trim_start().to_string() });
    }
    let (code, path) = line.split_at_checked(11)?;
    let path = path.strip_prefix(' ')?;
    let mut chars = code.chars();
    let kind = chars.next()?;
    let file_type = chars.next()?;
    if !matches!(kind, '<' | '>' | 'c' | 'h' | '.') || !matches!(file_type, 'f' | 'd' | 'L' | 'D' | 'S') || path.is_empty() {
        return None;
    }
    let change = if chars.all(|c| c == '+') {
        Change::Created
    } else if kind == '.' {
        return None;
    } else {
        Change::Updated
    };
    Some(Item { change, path: path.to_string() })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progress {
    pub bytes: u64,
    pub percent: u8,
    pub rate: String,
    pub eta: String,
}

pub fn parse_progress(line: &str) -> Option<Progress> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    let [bytes, percent, rate, eta, ..] = parts.as_slice() else {
        return None;
    };
    if !bytes.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    let percent = percent.strip_suffix('%')?.parse().ok()?;
    Some(Progress { bytes: leading_number(bytes), percent, rate: rate.to_string(), eta: eta.to_string() })
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
