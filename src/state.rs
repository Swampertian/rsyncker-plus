use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Success,
    Warning,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunState {
    pub started_at: u64,
    pub finished_at: u64,
    pub exit_code: i32,
    pub outcome: Outcome,
    pub files_transferred: u64,
    pub files_deleted: u64,
    pub bytes_transferred: u64,
    #[serde(default)]
    pub message: Option<String>,
}

impl RunState {
    pub fn file(dir: &Path, name: &str) -> PathBuf {
        dir.join(format!("{name}.json"))
    }

    pub fn load(dir: &Path, name: &str) -> Result<Option<Self>> {
        let path = Self::file(dir, name);
        if !path.exists() {
            return Ok(None);
        }
        let text = fs::read_to_string(&path)?;
        let state = serde_json::from_str(&text).with_context(|| format!("estado corrompido: {}", path.display()))?;
        Ok(Some(state))
    }

    pub fn save(&self, dir: &Path, name: &str) -> Result<()> {
        fs::create_dir_all(dir)?;
        let path = Self::file(dir, name);
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        fs::rename(tmp, path)?;
        Ok(())
    }

    pub fn delete(dir: &Path, name: &str) -> Result<()> {
        let path = Self::file(dir, name);
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn format_ago(timestamp: u64, now: u64) -> String {
    let secs = now.saturating_sub(timestamp);
    match secs {
        0..60 => "agora".to_string(),
        60..3_600 => format!("há {} min", secs / 60),
        3_600..86_400 => format!("há {} h", secs / 3_600),
        _ => format!("há {} d", secs / 86_400),
    }
}
