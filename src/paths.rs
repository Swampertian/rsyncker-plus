use std::env;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub struct Paths {
    pub jobs: PathBuf,
    pub state: PathBuf,
    pub systemd: PathBuf,
}

impl Paths {
    pub fn from_env() -> Result<Self> {
        let home = env::var_os("HOME").map(PathBuf::from).context("HOME não definido")?;
        let config = xdg_dir("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config"));
        let state = xdg_dir("XDG_STATE_HOME").unwrap_or_else(|| home.join(".local/state"));
        Ok(Self { jobs: config.join("rsync-tui/jobs"), state: state.join("rsync-tui"), systemd: config.join("systemd/user") })
    }

    pub fn under(root: &Path) -> Self {
        Self { jobs: root.join("jobs"), state: root.join("state"), systemd: root.join("systemd") }
    }
}

fn xdg_dir(var: &str) -> Option<PathBuf> {
    env::var_os(var).map(PathBuf::from).filter(|p| p.is_absolute())
}

pub fn expand_home(input: &str) -> PathBuf {
    let home = env::var_os("HOME").map(PathBuf::from);
    match (input, home) {
        ("~", Some(home)) => home,
        (text, Some(home)) if text.starts_with("~/") => home.join(&text[2..]),
        (text, _) => PathBuf::from(text),
    }
}

pub fn shorten_home(path: &Path) -> String {
    if let Some(home) = env::var_os("HOME").map(PathBuf::from) {
        if let Ok(rest) = path.strip_prefix(&home) {
            return if rest.as_os_str().is_empty() { "~".to_string() } else { format!("~/{}", rest.display()) };
        }
    }
    path.display().to_string()
}
