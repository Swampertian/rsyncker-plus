use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Copy,
    Mirror,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Schedule {
    #[default]
    None,
    Hourly,
    Daily,
    Weekly,
}

impl Schedule {
    pub fn on_calendar(self) -> Option<&'static str> {
        match self {
            Schedule::None => None,
            Schedule::Hourly => Some("hourly"),
            Schedule::Daily => Some("daily"),
            Schedule::Weekly => Some("weekly"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Job {
    #[serde(skip)]
    pub name: String,
    pub source: PathBuf,
    pub destination: PathBuf,
    pub mode: Mode,
    #[serde(default)]
    pub excludes: Vec<String>,
    #[serde(default)]
    pub schedule: Schedule,
    #[serde(default)]
    pub bwlimit_kbps: Option<u32>,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
}

fn enabled_by_default() -> bool {
    true
}

impl Job {
    pub fn new(name: &str, source: PathBuf, destination: PathBuf, mode: Mode) -> Self {
        Self { name: name.to_string(), source, destination, mode, excludes: Vec::new(), schedule: Schedule::None, bwlimit_kbps: None, enabled: true }
    }

    pub fn from_toml(name: &str, text: &str) -> Result<Self> {
        let mut job: Job = toml::from_str(text).with_context(|| format!("job '{name}' inválido"))?;
        job.name = name.to_string();
        Ok(job)
    }

    pub fn to_toml(&self) -> Result<String> {
        Ok(toml::to_string(self)?)
    }

    pub fn file(dir: &Path, name: &str) -> PathBuf {
        dir.join(format!("{name}.toml"))
    }

    pub fn load(dir: &Path, name: &str) -> Result<Self> {
        validate_name(name)?;
        let path = Self::file(dir, name);
        let text = fs::read_to_string(&path).with_context(|| format!("job '{name}' não encontrado em {}", path.display()))?;
        Self::from_toml(name, &text)
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        validate_name(&self.name)?;
        fs::create_dir_all(dir)?;
        let content = self.to_toml()?;
        let path = Self::file(dir, &self.name);
        let tmp = path.with_extension("toml.tmp");
        fs::write(&tmp, content)?;
        fs::rename(tmp, path)?;
        Ok(())
    }

    pub fn save_new(&self, dir: &Path) -> Result<()> {
        validate_name(&self.name)?;
        fs::create_dir_all(dir)?;
        let content = self.to_toml()?;
        let path = Self::file(dir, &self.name);
        let mut file =
            OpenOptions::new().write(true).create_new(true).open(&path).with_context(|| format!("job '{}' já existe ou não pode ser criado", self.name))?;
        if let Err(error) = file.write_all(content.as_bytes()) {
            let _ = fs::remove_file(path);
            return Err(error.into());
        }
        Ok(())
    }

    pub fn delete(dir: &Path, name: &str) -> Result<()> {
        validate_name(name)?;
        fs::remove_file(Self::file(dir, name)).with_context(|| format!("job '{name}' não encontrado"))
    }

    pub fn list(dir: &Path) -> Result<Vec<Self>> {
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut jobs = Vec::new();
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "toml") {
                if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                    jobs.push(Self::load(dir, name)?);
                }
            }
        }
        jobs.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(jobs)
    }

    pub fn validate(&self) -> Result<()> {
        validate_name(&self.name)?;
        for (label, path) in [("origem", &self.source), ("destino", &self.destination)] {
            if !path.is_absolute() {
                bail!("{label} precisa ser caminho absoluto: {}", path.display());
            }
            if path.components().any(|c| c == Component::ParentDir) {
                bail!("{label} não pode conter '..': {}", path.display());
            }
        }
        if self.source.starts_with(&self.destination) || self.destination.starts_with(&self.source) {
            bail!("origem e destino não podem ser iguais nem um dentro do outro");
        }
        Ok(())
    }
}

pub fn validate_name(name: &str) -> Result<()> {
    let valid = !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    if !valid {
        bail!("nome '{name}' inválido: use só a-z, 0-9, '-' e '_'");
    }
    Ok(())
}

pub fn name_from_path(path: &Path) -> String {
    let raw = path.file_name().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
    raw.chars().map(|c| if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' { c } else { '-' }).collect::<String>().trim_matches('-').to_string()
}
