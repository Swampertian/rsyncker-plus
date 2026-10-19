use std::fs;
use std::io::ErrorKind;
use std::process::Command;

use crate::paths::expand_home;
use crate::remote::{Protocol, validate_host};

const SERVICES: [(&str, Protocol); 3] = [("_ssh._tcp", Protocol::Ssh), ("_sftp-ssh._tcp", Protocol::Ssh), ("_rsync._tcp", Protocol::Daemon)];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub name: String,
    pub host: String,
    pub address: String,
    pub protocol: Protocol,
    pub port: Option<u16>,
    pub from_config: bool,
}

impl Device {
    pub fn endpoint(&self) -> String {
        match (self.protocol, self.port) {
            (Protocol::Ssh, None | Some(22)) => format!("{}:/", self.host),
            (Protocol::Ssh, Some(port)) => format!("ssh://{}:{port}/", self.host),
            (Protocol::Daemon, None | Some(873)) => format!("rsync://{}/", self.host),
            (Protocol::Daemon, Some(port)) => format!("rsync://{}:{port}/", self.host),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Found {
    pub devices: Vec<Device>,
    pub warning: Option<String>,
}

pub fn discover() -> Found {
    let mut found = Found::default();
    for (service, _) in SERVICES {
        match Command::new("avahi-browse").args(["-t", "-p", "-r", "-k", service]).output() {
            Ok(output) if output.status.success() => merge(&mut found.devices, parse_avahi(&String::from_utf8_lossy(&output.stdout))),
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                found.warning = Some(format!("busca na rede falhou (avahi-daemon está rodando?): {stderr}"));
                break;
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {
                found.warning = Some("avahi-browse não encontrado: instale avahi-utils para buscar na rede".to_string());
                break;
            }
            Err(error) => {
                found.warning = Some(format!("busca na rede falhou: {error}"));
                break;
            }
        }
    }
    let config = fs::read_to_string(expand_home("~/.ssh/config")).unwrap_or_default();
    merge(&mut found.devices, parse_ssh_config(&config));
    found
}

fn merge(devices: &mut Vec<Device>, new: Vec<Device>) {
    for device in new {
        let duplicate = devices.iter().any(|d| d.host == device.host && d.protocol == device.protocol && d.port == device.port);
        if !duplicate {
            devices.push(device);
        }
    }
}

pub fn parse_avahi(text: &str) -> Vec<Device> {
    let mut devices: Vec<Device> = Vec::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split(';').collect();
        let ["=", _, family, name, service, _, host, address, port, ..] = fields.as_slice() else {
            continue;
        };
        let Some((_, protocol)) = SERVICES.iter().find(|(s, _)| s == service) else {
            continue;
        };
        let host = host.trim_end_matches('.').to_string();
        if validate_host(&host).is_err() {
            continue;
        }
        let device = Device { name: unescape(name), host, address: address.to_string(), protocol: *protocol, port: port.parse().ok(), from_config: false };
        match devices.iter_mut().find(|d| d.host == device.host && d.protocol == device.protocol && d.port == device.port) {
            Some(existing) if *family == "IPv4" => existing.address = device.address,
            Some(_) => {}
            None => devices.push(device),
        }
    }
    devices
}

fn unescape(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut result = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' {
            let digits: String = chars[i + 1..].iter().take(3).collect();
            if digits.len() == 3 && digits.chars().all(|c| c.is_ascii_digit()) {
                if let Some(c) = digits.parse::<u32>().ok().and_then(char::from_u32) {
                    result.push(c);
                    i += 4;
                    continue;
                }
            }
            if let Some(next) = chars.get(i + 1) {
                result.push(*next);
                i += 2;
                continue;
            }
        }
        result.push(chars[i]);
        i += 1;
    }
    result
}

pub fn parse_ssh_config(text: &str) -> Vec<Device> {
    let mut devices = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        let Some((key, value)) = line.split_once(|c: char| c.is_whitespace() || c == '=') else {
            continue;
        };
        if !key.eq_ignore_ascii_case("host") {
            continue;
        }
        for alias in value.trim_start_matches(|c: char| c.is_whitespace() || c == '=').split_whitespace() {
            if alias.contains(['*', '?', '!']) || validate_host(alias).is_err() {
                continue;
            }
            devices.push(Device {
                name: alias.to_string(),
                host: alias.to_string(),
                address: String::new(),
                protocol: Protocol::Ssh,
                port: None,
                from_config: true,
            });
        }
    }
    devices
}
