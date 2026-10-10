use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::job::Job;
use crate::paths::Paths;

pub const SERVICE_TEMPLATE: &str = "rsync-tui@.service";

pub fn service_name(name: &str) -> String {
    format!("rsync-tui@{name}.service")
}

pub fn timer_name(name: &str) -> String {
    format!("rsync-tui@{name}.timer")
}

pub fn service_template(exe: &Path) -> String {
    format!(
        "[Unit]\n\
         Description=rsync-tui: %i\n\
         \n\
         [Service]\n\
         Type=oneshot\n\
         ExecStart={} run %i\n\
         Nice=19\n\
         IOSchedulingClass=idle\n",
        quote(exe)
    )
}

pub fn mounts_dropin(job: &Job) -> String {
    format!("[Unit]\nRequiresMountsFor={} {}\n", quote(&job.source), quote(&job.destination))
}

pub fn timer_unit(job: &Job) -> Option<String> {
    let calendar = job.schedule.on_calendar()?;
    Some(format!(
        "[Unit]\n\
         Description=rsync-tui: {name}\n\
         \n\
         [Timer]\n\
         OnCalendar={calendar}\n\
         Persistent=true\n\
         Unit={service}\n\
         \n\
         [Install]\n\
         WantedBy=timers.target\n",
        name = job.name,
        service = service_name(&job.name),
    ))
}

fn dropin_dir(paths: &Paths, name: &str) -> PathBuf {
    paths.systemd.join(format!("{}.d", service_name(name)))
}

pub fn write_units(paths: &Paths, job: &Job, exe: &Path) -> Result<()> {
    fs::create_dir_all(&paths.systemd)?;
    fs::write(paths.systemd.join(SERVICE_TEMPLATE), service_template(exe))?;

    let dropin = dropin_dir(paths, &job.name);
    fs::create_dir_all(&dropin)?;
    fs::write(dropin.join("mounts.conf"), mounts_dropin(job))?;

    let timer = paths.systemd.join(timer_name(&job.name));
    match timer_unit(job) {
        Some(content) => fs::write(timer, content)?,
        None if timer.exists() => fs::remove_file(timer)?,
        None => {}
    }
    Ok(())
}

pub fn remove_units(paths: &Paths, name: &str) -> Result<()> {
    let timer = paths.systemd.join(timer_name(name));
    if timer.exists() {
        fs::remove_file(timer)?;
    }
    let dropin = dropin_dir(paths, name);
    if dropin.exists() {
        fs::remove_dir_all(dropin)?;
    }
    Ok(())
}

pub fn systemctl(args: &[&str]) -> Result<String> {
    let out = Command::new("systemctl").arg("--user").args(args).output().context("não foi possível executar systemctl")?;
    if !out.status.success() {
        bail!("systemctl --user {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn apply(job: &Job) -> Result<()> {
    systemctl(&["daemon-reload"])?;
    let timer = timer_name(&job.name);
    if job.enabled && job.schedule.on_calendar().is_some() {
        systemctl(&["enable", "--now", &timer])?;
    } else {
        let _ = systemctl(&["disable", "--now", &timer]);
    }
    Ok(())
}

pub fn deactivate(name: &str) -> Result<()> {
    let _ = systemctl(&["disable", "--now", &timer_name(name)]);
    systemctl(&["daemon-reload"])?;
    Ok(())
}

pub fn next_run(name: &str) -> Option<String> {
    let value = systemctl(&["show", &timer_name(name), "--property=NextElapseUSecRealtime", "--value"]).ok()?;
    (!value.is_empty() && value != "n/a").then_some(value)
}

pub fn start(name: &str) -> Result<()> {
    systemctl(&["start", "--no-block", &service_name(name)])?;
    Ok(())
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnitStatus {
    pub running: bool,
    pub next_run: Option<String>,
}

pub fn status(names: &[String]) -> HashMap<String, UnitStatus> {
    if names.is_empty() {
        return HashMap::new();
    }
    let units: Vec<String> = names.iter().flat_map(|name| [service_name(name), timer_name(name)]).collect();
    let mut args = vec!["show", "--property=Id,ActiveState,NextElapseUSecRealtime"];
    args.extend(units.iter().map(String::as_str));
    systemctl(&args).map(|out| parse_status(&out)).unwrap_or_default()
}

pub fn parse_status(text: &str) -> HashMap<String, UnitStatus> {
    let mut result: HashMap<String, UnitStatus> = HashMap::new();
    for block in text.split("\n\n") {
        let props: HashMap<&str, &str> = block.lines().filter_map(|line| line.split_once('=')).collect();
        let Some(id) = props.get("Id").and_then(|id| id.strip_prefix("rsync-tui@")) else {
            continue;
        };
        if let Some(name) = id.strip_suffix(".service") {
            let state = props.get("ActiveState").copied().unwrap_or_default();
            result.entry(name.to_string()).or_default().running = matches!(state, "active" | "activating" | "reloading");
        } else if let Some(name) = id.strip_suffix(".timer") {
            let next = props.get("NextElapseUSecRealtime").copied().unwrap_or_default();
            result.entry(name.to_string()).or_default().next_run = (!next.is_empty() && next != "n/a").then(|| next.to_string());
        }
    }
    result
}

fn quote(path: &Path) -> String {
    let text = path.to_string_lossy().replace('%', "%%");
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}
