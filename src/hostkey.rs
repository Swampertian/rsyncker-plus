use std::env;
use std::fs::{self, DirBuilder, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};

use crate::paths::expand_home;
use crate::remote::{self, Remote};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unknown {
    pub host: String,
    pub fingerprints: Vec<String>,
    pub entries: String,
    pub file: PathBuf,
}

pub fn check(remote: &Remote) -> Result<Option<Unknown>> {
    let strict = probe(remote, &["StrictHostKeyChecking=yes".to_string()])?;
    let stderr = String::from_utf8_lossy(&strict.stderr);
    if stderr.contains("IDENTIFICATION HAS CHANGED") {
        bail!("{}", remote::explain(&stderr).unwrap_or_default());
    }
    if !stderr.contains("Host key verification failed") {
        if strict.status.success() || stderr.contains("Permission denied") {
            return Ok(None);
        }
        bail!("{}", remote::explain(&stderr).unwrap_or_else(|| remote::last_line(&stderr)));
    }

    let scratch = env::temp_dir().join(format!("rsync-tui-hostkey-{}-{}", std::process::id(), nanos()));
    OpenOptions::new().write(true).create_new(true).mode(0o600).open(&scratch).context("não foi possível criar arquivo temporário")?;
    let options =
        ["StrictHostKeyChecking=accept-new".to_string(), format!("UserKnownHostsFile={}", scratch.display()), "GlobalKnownHostsFile=/dev/null".to_string()];
    let scan = probe(remote, &options);
    let entries = fs::read_to_string(&scratch).unwrap_or_default();
    let fingerprints = fingerprints(&scratch);
    let _ = fs::remove_file(&scratch);
    let scan = scan?;
    if entries.trim().is_empty() {
        let stderr = String::from_utf8_lossy(&scan.stderr);
        bail!("não foi possível obter a chave do host: {}", remote::explain(&stderr).unwrap_or_else(|| remote::last_line(&stderr)));
    }
    let host = match remote.port {
        Some(port) => format!("{}:{port}", remote.host),
        None => remote.host.clone(),
    };
    Ok(Some(Unknown { host, fingerprints: fingerprints?, entries, file: known_hosts_file(remote) }))
}

pub fn trust(unknown: &Unknown) -> Result<()> {
    if let Some(dir) = unknown.file.parent() {
        if !dir.exists() {
            DirBuilder::new().recursive(true).mode(0o700).create(dir).with_context(|| format!("não foi possível criar {}", dir.display()))?;
        }
    }
    let existing = fs::read_to_string(&unknown.file).unwrap_or_default();
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(&unknown.file)
        .with_context(|| format!("não foi possível abrir {}", unknown.file.display()))?;
    if !existing.is_empty() && !existing.ends_with('\n') {
        file.write_all(b"\n")?;
    }
    file.write_all(unknown.entries.as_bytes())?;
    if !unknown.entries.ends_with('\n') {
        file.write_all(b"\n")?;
    }
    Ok(())
}

fn probe(remote: &Remote, extra: &[String]) -> Result<Output> {
    let mut command = Command::new("ssh");
    command.args(remote.ssh_options());
    for option in
        ["PubkeyAuthentication=no", "PasswordAuthentication=no", "KbdInteractiveAuthentication=no", "GSSAPIAuthentication=no", "HostbasedAuthentication=no"]
            .iter()
            .map(|o| o.to_string())
            .chain(extra.iter().cloned())
    {
        command.arg("-o").arg(option);
    }
    command.arg(remote.destination()).arg("true");
    command.output().context("não foi possível executar ssh (está instalado?)")
}

fn fingerprints(file: &Path) -> Result<Vec<String>> {
    let output = Command::new("ssh-keygen").arg("-l").arg("-f").arg(file).output().context("não foi possível executar ssh-keygen")?;
    Ok(parse_fingerprints(&String::from_utf8_lossy(&output.stdout)))
}

pub fn parse_fingerprints(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            match parts.as_slice() {
                [_, hash, .., kind] if hash.contains(':') => Some(format!("{hash} {kind}")),
                _ => None,
            }
        })
        .collect()
}

fn known_hosts_file(remote: &Remote) -> PathBuf {
    let default = expand_home("~/.ssh/known_hosts");
    let Ok(output) = Command::new("ssh").args(remote.ssh_options()).arg("-G").arg(remote.destination()).output() else {
        return default;
    };
    parse_known_hosts_file(&String::from_utf8_lossy(&output.stdout)).unwrap_or(default)
}

pub fn parse_known_hosts_file(ssh_config: &str) -> Option<PathBuf> {
    ssh_config.lines().find_map(|line| line.strip_prefix("userknownhostsfile ")).and_then(|files| files.split_whitespace().next()).map(expand_home)
}

fn nanos() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or_default()
}
