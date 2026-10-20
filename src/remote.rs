use std::env;
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::paths::expand_home;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Source,
    Destination,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Ssh,
    Daemon,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Remote {
    pub side: Side,
    pub protocol: Protocol,
    pub host: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password_file: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Address {
    pub protocol: Protocol,
    pub user: Option<String>,
    pub host: String,
    pub port: Option<u16>,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    Local(PathBuf),
    Remote(Address),
}

impl Endpoint {
    pub fn path(&self) -> &Path {
        match self {
            Endpoint::Local(path) => path,
            Endpoint::Remote(address) => &address.path,
        }
    }
}

pub fn parse(text: &str) -> Result<Endpoint> {
    let text = text.trim();
    if let Some(rest) = text.strip_prefix("rsync://") {
        let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
        let (user, host_port) = split_user(authority);
        let (host, port) = split_port(host_port)?;
        return address(Protocol::Daemon, user, host, port, path);
    }
    if let Some(rest) = text.strip_prefix("ssh://") {
        let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
        let (user, host_port) = split_user(authority);
        let (host, port) = split_port(host_port)?;
        return address(Protocol::Ssh, user, host, port, &format!("/{path}"));
    }
    if !text.starts_with(['/', '~', '.']) {
        if let Some((authority, path)) = text.split_once("::") {
            if !authority.contains('/') {
                let (user, host) = split_user(authority);
                return address(Protocol::Daemon, user, host, None, path);
            }
        }
        if let Some((authority, path)) = text.split_once(':') {
            if !authority.contains('/') {
                let (user, host) = split_user(authority);
                return address(Protocol::Ssh, user, host, None, path);
            }
        }
    }
    Ok(Endpoint::Local(expand_home(text)))
}

fn split_user(authority: &str) -> (Option<&str>, &str) {
    match authority.rsplit_once('@') {
        Some((user, host)) => (Some(user), host),
        None => (None, authority),
    }
}

fn split_port(host_port: &str) -> Result<(&str, Option<u16>)> {
    match host_port.rsplit_once(':') {
        Some((host, port)) => Ok((host, Some(port.parse().with_context(|| format!("porta inválida: '{port}'"))?))),
        None => Ok((host_port, None)),
    }
}

fn address(protocol: Protocol, user: Option<&str>, host: &str, port: Option<u16>, path: &str) -> Result<Endpoint> {
    let address = Address { protocol, user: user.map(String::from), host: host.to_string(), port, path: PathBuf::from(path) };
    validate_host(&address.host)?;
    if let Some(user) = &address.user {
        validate_user(user)?;
    }
    validate_path(protocol, &address.path)?;
    Ok(Endpoint::Remote(address))
}

pub fn validate_host(host: &str) -> Result<()> {
    let valid = !host.is_empty() && !host.starts_with('-') && host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
    if !valid {
        bail!("host '{host}' inválido: use nome, IP ou alias do ~/.ssh/config");
    }
    Ok(())
}

fn validate_user(user: &str) -> Result<()> {
    let valid = !user.is_empty() && !user.starts_with('-') && user.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
    if !valid {
        bail!("usuário '{user}' inválido");
    }
    Ok(())
}

pub fn validate_path(protocol: Protocol, path: &Path) -> Result<()> {
    if path.components().any(|c| c == Component::ParentDir) {
        bail!("caminho remoto não pode conter '..': {}", path.display());
    }
    match protocol {
        Protocol::Ssh if !path.is_absolute() => bail!("caminho remoto precisa ser absoluto: usuário@host:/caminho"),
        Protocol::Ssh if path == Path::new("/") => bail!("escolha uma pasta no host remoto, não a raiz '/'"),
        Protocol::Daemon if path.as_os_str().is_empty() || path.is_absolute() => bail!("informe o módulo: rsync://host/módulo/caminho"),
        _ => Ok(()),
    }
}

impl Address {
    pub fn into_remote(self, side: Side, credential: Option<PathBuf>) -> Remote {
        let (identity, password_file) = match self.protocol {
            Protocol::Ssh => (credential, None),
            Protocol::Daemon => (None, credential),
        };
        Remote { side, protocol: self.protocol, host: self.host, user: self.user, port: self.port, identity, password_file }
    }
}

impl Remote {
    pub fn validate(&self, path: &Path) -> Result<()> {
        validate_host(&self.host)?;
        if let Some(user) = &self.user {
            validate_user(user)?;
        }
        validate_path(self.protocol, path)?;
        for file in [&self.identity, &self.password_file].into_iter().flatten() {
            if !file.is_absolute() {
                bail!("arquivo de chave/senha precisa ser caminho absoluto: {}", file.display());
            }
        }
        Ok(())
    }

    pub fn credential(&self) -> Option<&Path> {
        self.identity.as_deref().or(self.password_file.as_deref())
    }

    pub fn destination(&self) -> String {
        match &self.user {
            Some(user) => format!("{user}@{}", self.host),
            None => self.host.clone(),
        }
    }

    pub fn label(&self, path: &Path) -> String {
        let path = path.display();
        match (self.protocol, self.port) {
            (Protocol::Ssh, None) => format!("{}:{path}", self.destination()),
            (Protocol::Ssh, Some(port)) => format!("ssh://{}:{port}{path}", self.destination()),
            (Protocol::Daemon, None) => format!("rsync://{}/{path}", self.destination()),
            (Protocol::Daemon, Some(port)) => format!("rsync://{}:{port}/{path}", self.destination()),
        }
    }

    pub fn spec(&self, path: &Path) -> OsString {
        match self.protocol {
            Protocol::Ssh => {
                let mut spec = OsString::from(format!("{}:", self.destination()));
                spec.push(path);
                spec
            }
            Protocol::Daemon => {
                let mut spec = OsString::from(self.label(Path::new("")));
                spec.push(path);
                spec
            }
        }
    }

    pub fn ssh_options(&self) -> Vec<OsString> {
        let mut options: Vec<OsString> = ["-o", "BatchMode=yes", "-o", "ConnectTimeout=10"].into_iter().map(OsString::from).collect();
        if let Some(port) = self.port {
            options.push("-p".into());
            options.push(port.to_string().into());
        }
        if let Some(identity) = &self.identity {
            options.push("-i".into());
            options.push(identity.clone().into_os_string());
            options.push("-o".into());
            options.push("IdentitiesOnly=yes".into());
        }
        options
    }

    pub fn rsync_options(&self) -> Vec<OsString> {
        match self.protocol {
            Protocol::Ssh => {
                let mut command = OsString::from("ssh -o StrictHostKeyChecking=yes");
                for option in self.ssh_options() {
                    command.push(" ");
                    command.push(quote(&option));
                }
                vec!["-e".into(), command]
            }
            Protocol::Daemon => {
                let mut options = vec![OsString::from("--contimeout=10")];
                if let Some(file) = &self.password_file {
                    let mut option = OsString::from("--password-file=");
                    option.push(file);
                    options.push(option);
                }
                options
            }
        }
    }
}

fn quote(text: &OsString) -> OsString {
    let raw = text.to_string_lossy();
    if raw.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | '=' | ':' | '+' | ',')) {
        return text.clone();
    }
    OsString::from(format!("'{}'", raw.replace('\'', "''")))
}

pub fn rsync_command() -> Command {
    let mut command = Command::new("rsync");
    if env::var_os("RSYNC_PASSWORD").is_none() {
        command.env("RSYNC_PASSWORD", "");
    }
    command
}

pub fn check_dir(remote: &Remote, path: &Path) -> Result<()> {
    let output = rsync_command()
        .args(remote.rsync_options())
        .args(["--list-only", "--dirs"])
        .arg(remote.spec(path))
        .output()
        .context("não foi possível executar rsync (está instalado?)")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.status.success() {
        if stdout.lines().next().is_some_and(|line| line.starts_with('d')) {
            return Ok(());
        }
        bail!("caminho remoto não é pasta: {}", remote.label(path));
    }
    if stderr.contains("No such file or directory") {
        bail!("pasta remota não existe: {} (crie a pasta antes)", remote.label(path));
    }
    bail!("{}", explain(&stderr).unwrap_or_else(|| last_line(&stderr)))
}

pub fn explain(stderr: &str) -> Option<String> {
    let rules = [
        ("IDENTIFICATION HAS CHANGED", "ATENÇÃO: a chave do host mudou (servidor reinstalado ou possível ataque); verifique antes de confiar de novo"),
        ("Host key verification failed", "chave do host desconhecida: confira e confirme com `rsync-tui trust-host <job>` ou pela prévia (d) na TUI"),
        ("Permission denied", "acesso negado pelo SSH: confira usuário e chave (sem senha ou carregada no ssh-agent)"),
        ("Could not resolve hostname", "host não encontrado na rede"),
        ("Connection refused", "conexão recusada: o serviço SSH/rsync está rodando no host?"),
        ("No route to host", "sem rota até o host: ele está ligado e na mesma rede?"),
        ("timed out", "tempo esgotado ao conectar: o host está ligado e na mesma rede?"),
        ("@ERROR: auth failed", "usuário ou senha do rsync recusados: confira o arquivo de senha"),
        ("@ERROR: Unknown module", "módulo rsync não existe no host"),
        ("@ERROR: access denied", "acesso negado ao módulo rsync"),
        ("must not be other-accessible", "arquivo de senha precisa ter permissão 600 (chmod 600)"),
        ("could not open password file", "não foi possível ler o arquivo de senha"),
    ];
    rules.iter().find(|(pattern, _)| stderr.contains(pattern)).map(|(_, message)| message.to_string())
}

pub fn last_line(text: &str) -> String {
    text.lines().filter(|line| !line.trim().is_empty()).last().unwrap_or_default().to_string()
}
