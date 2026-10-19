use std::path::{Path, PathBuf};

use rsync_tui::daemon::systemd;
use rsync_tui::discover::{self, Device};
use rsync_tui::hostkey;
use rsync_tui::job::{Job, Mode};
use rsync_tui::remote::{self, Address, Endpoint, Protocol, Remote, Side};
use rsync_tui::rsync::{self, Options};

fn remote_address(text: &str) -> Address {
    match remote::parse(text).unwrap() {
        Endpoint::Remote(address) => address,
        Endpoint::Local(path) => panic!("esperava remoto, veio local {}", path.display()),
    }
}

fn push_job(destination: &str, credential: Option<&str>) -> Job {
    let source = remote::parse("/home/u/Fotos").unwrap();
    let destination = remote::parse(destination).unwrap();
    Job::from_endpoints("fotos", source, destination, Mode::Copy, credential.map(PathBuf::from)).unwrap()
}

fn args(job: &Job) -> Vec<String> {
    rsync::build_args(job, Options::default()).into_iter().map(|a| a.to_string_lossy().into_owned()).collect()
}

#[test]
fn parses_ssh_scp_style() {
    let address = remote_address("ana@nas.local:/backup/fotos");
    assert_eq!(address.protocol, Protocol::Ssh);
    assert_eq!(address.user.as_deref(), Some("ana"));
    assert_eq!(address.host, "nas.local");
    assert_eq!(address.port, None);
    assert_eq!(address.path, Path::new("/backup/fotos"));
}

#[test]
fn parses_ssh_url_with_port() {
    let address = remote_address("ssh://192.168.1.20:2222/srv/backup");
    assert_eq!((address.host.as_str(), address.port), ("192.168.1.20", Some(2222)));
    assert_eq!(address.path, Path::new("/srv/backup"));
}

#[test]
fn parses_daemon_forms() {
    let url = remote_address("rsync://bkp@nas:8873/modulo/sub");
    assert_eq!(url.protocol, Protocol::Daemon);
    assert_eq!((url.user.as_deref(), url.host.as_str(), url.port), (Some("bkp"), "nas", Some(8873)));
    assert_eq!(url.path, Path::new("modulo/sub"));

    let double_colon = remote_address("nas::modulo");
    assert_eq!(double_colon.protocol, Protocol::Daemon);
    assert_eq!(double_colon.path, Path::new("modulo"));
}

#[test]
fn local_paths_stay_local() {
    for text in ["/mnt/a:b", "./pasta", "~/Fotos"] {
        assert!(matches!(remote::parse(text).unwrap(), Endpoint::Local(_)), "{text}");
    }
}

#[test]
fn rejects_unsafe_or_incomplete_addresses() {
    for text in ["-oProxyCommand=x:/a", "bad host:/a", "nas:relativo", "nas:/", "nas:/a/../b", "rsync://nas/", "ssh://nas:abc/a", "-u@nas:/a"] {
        assert!(remote::parse(text).is_err(), "{text} deveria ser recusado");
    }
}

#[test]
fn labels_round_trip_through_parse() {
    for text in ["ana@nas:/backup", "ssh://nas:2222/backup", "rsync://nas/modulo/sub", "rsync://bkp@nas:8873/modulo"] {
        let job = push_job(text, None);
        assert_eq!(job.destination_label(), text);
    }
}

#[test]
fn ssh_job_passes_options_through_rsync_shell() {
    let job = push_job("ssh://ana@nas:2222/backup", Some("/home/u/.ssh/id backup"));
    let a = args(&job);
    let shell = &a[a.iter().position(|x| x == "-e").unwrap() + 1];
    assert!(shell.starts_with("ssh -o StrictHostKeyChecking=yes -o BatchMode=yes"));
    assert!(shell.contains("-p 2222"));
    assert!(shell.contains("-i '/home/u/.ssh/id backup' -o IdentitiesOnly=yes"));
    assert_eq!(&a[a.len() - 2..], ["/home/u/Fotos/", "ana@nas:/backup"]);
}

#[test]
fn daemon_job_uses_password_file_and_url() {
    let job = push_job("rsync://bkp@nas/modulo", Some("/home/u/.rsync-senha"));
    let a = args(&job);
    assert!(a.contains(&"--password-file=/home/u/.rsync-senha".to_string()));
    assert!(!a.contains(&"-e".to_string()));
    assert_eq!(a.last().unwrap(), "rsync://bkp@nas/modulo");
}

#[test]
fn pull_puts_remote_first() {
    let job = Job::from_endpoints("puxar", remote::parse("nas:/dados").unwrap(), remote::parse("/home/u/copia").unwrap(), Mode::Copy, None).unwrap();
    assert_eq!(job.remote.as_ref().unwrap().side, Side::Source);
    let a = args(&job);
    assert_eq!(&a[a.len() - 2..], ["nas:/dados/", "/home/u/copia"]);
}

#[test]
fn job_rules_for_remote() {
    let both = Job::from_endpoints("x", remote::parse("a:/x").unwrap(), remote::parse("b:/y").unwrap(), Mode::Copy, None);
    assert!(both.is_err());

    let local_key = Job::from_endpoints("x", remote::parse("/a").unwrap(), remote::parse("/b").unwrap(), Mode::Copy, Some("/k".into()));
    assert!(local_key.is_err());

    let mut pull = Job::from_endpoints("x", remote::parse("nas:/dados").unwrap(), remote::parse("/home/u/c").unwrap(), Mode::Copy, None).unwrap();
    assert!(pull.validate().is_ok());
    pull.on_change = true;
    assert!(pull.validate().unwrap_err().to_string().contains("origem local"));

    let push = push_job("nas:/home/u/Fotos/copia", None);
    assert!(push.validate().is_ok(), "mesmo caminho em outro host não é sobreposição");
}

#[test]
fn remote_survives_toml() {
    let job = push_job("ssh://ana@nas:2222/backup", Some("/home/u/.ssh/id_backup"));
    let text = job.to_toml().unwrap();
    assert!(text.contains("[remote]"));
    let loaded = Job::from_toml("fotos", &text).unwrap();
    assert_eq!(loaded, job);

    let local = Job::new("l", "/a".into(), "/b".into(), Mode::Copy);
    assert!(!local.to_toml().unwrap().contains("remote"));
}

#[test]
fn mounts_only_cover_local_side() {
    let job = push_job("nas:/backup", None);
    assert_eq!(systemd::mounts_dropin(&job), "[Unit]\nRequiresMountsFor=\"/home/u/Fotos\"\n");
}

#[test]
fn explains_common_network_errors() {
    let cases = [
        ("Host key verification failed.\r\nrsync: connection unexpectedly closed", "chave do host desconhecida"),
        ("ana@nas: Permission denied (publickey).", "acesso negado"),
        ("ssh: Could not resolve hostname nas: Name or service not known", "host não encontrado"),
        ("@ERROR: auth failed on module backup", "senha do rsync"),
        ("@ERROR: Unknown module 'x'", "módulo rsync não existe"),
        ("ERROR: password file must not be other-accessible", "chmod 600"),
    ];
    for (stderr, expected) in cases {
        assert!(remote::explain(stderr).unwrap().contains(expected), "{stderr}");
    }
    assert_eq!(remote::explain("rsync: algo local"), None);
}

#[test]
fn reads_fingerprints_and_known_hosts_file() {
    let fingerprints = hostkey::parse_fingerprints("256 SHA256:abc123 |1|xyz= (ED25519)\n3072 SHA256:def456 nas (RSA)\n");
    assert_eq!(fingerprints, ["SHA256:abc123 (ED25519)", "SHA256:def456 (RSA)"]);

    let config = "hostname nas\nuserknownhostsfile /home/u/.ssh/known_hosts /home/u/.ssh/known_hosts2\nport 22\n";
    assert_eq!(hostkey::parse_known_hosts_file(config), Some(PathBuf::from("/home/u/.ssh/known_hosts")));
}

#[test]
fn trust_appends_entries_with_private_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("ssh/known_hosts");
    let unknown = hostkey::Unknown { host: "nas".into(), fingerprints: vec![], entries: "nas ssh-ed25519 AAAA".into(), file: file.clone() };
    hostkey::trust(&unknown).unwrap();
    hostkey::trust(&unknown).unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "nas ssh-ed25519 AAAA\nnas ssh-ed25519 AAAA\n");
    assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
}

#[test]
fn parses_avahi_services() {
    let text = "\
+;eth0;IPv4;NAS;_ssh._tcp;local
=;eth0;IPv6;Meu\\032NAS;_ssh._tcp;local;nas.local;fe80::1;22;
=;eth0;IPv4;Meu\\032NAS;_ssh._tcp;local;nas.local;192.168.1.20;22;
=;eth0;IPv4;Pi;_rsync._tcp;local;pi.local;192.168.1.30;8873;
=;eth0;IPv4;Impressora;_ipp._tcp;local;epson.local;192.168.1.2;631;
";
    let devices = discover::parse_avahi(text);
    assert_eq!(devices.len(), 2);
    assert_eq!(devices[0].name, "Meu NAS");
    assert_eq!(devices[0].address, "192.168.1.20");
    assert_eq!(devices[0].endpoint(), "nas.local:/");
    assert_eq!(devices[1].endpoint(), "rsync://pi.local:8873/");
}

#[test]
fn reads_ssh_config_hosts() {
    let config = "Host nas backup-pi\n  HostName 192.168.1.20\nHost *.lan !x\nhost=servidor\n";
    let names: Vec<String> = discover::parse_ssh_config(config).into_iter().map(|d| d.host).collect();
    assert_eq!(names, ["nas", "backup-pi", "servidor"]);
}

#[test]
fn device_endpoint_matches_protocol_and_port() {
    let device = |protocol, port| Device { name: "x".into(), host: "nas".into(), address: String::new(), protocol, port, from_config: false };
    assert_eq!(device(Protocol::Ssh, Some(2222)).endpoint(), "ssh://nas:2222/");
    assert_eq!(device(Protocol::Daemon, Some(873)).endpoint(), "rsync://nas/");
}

#[test]
fn remote_struct_reports_credential() {
    let remote = Remote {
        side: Side::Destination,
        protocol: Protocol::Daemon,
        host: "nas".into(),
        user: None,
        port: None,
        identity: None,
        password_file: Some("/s".into()),
    };
    assert_eq!(remote.credential(), Some(Path::new("/s")));
}
