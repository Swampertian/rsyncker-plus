use std::path::Path;

use clap::{CommandFactory, Parser};
use rsync_tui::cli::{Cli, Command};
use rsync_tui::job::{Mode, Schedule};

fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
    Cli::try_parse_from(args)
}

#[test]
fn cli_definition_is_valid() {
    Cli::command().debug_assert();
}

#[test]
fn no_subcommand_opens_tui() {
    assert!(parse(&["rsync-tui"]).unwrap().command.is_none());
}

#[test]
fn run_reads_job_name() {
    let cli = parse(&["rsync-tui", "run", "fotos"]).unwrap();
    assert!(matches!(cli.command, Some(Command::Run { job, progress: false }) if job == "fotos"));
}

#[test]
fn add_uses_defaults() {
    let cli = parse(&["rsync-tui", "add", "/a", "--to", "/b"]).unwrap();
    let Some(Command::Add { source, to, mode, schedule, name, excludes, bwlimit, on_change, key, confirm }) = cli.command else {
        panic!("esperava Add");
    };
    assert!(!confirm.dry_run && !confirm.yes);
    assert_eq!(source, "/a");
    assert_eq!(to, "/b");
    assert_eq!(mode, Mode::Copy);
    assert_eq!(schedule, Schedule::Daily);
    assert!(name.is_none() && excludes.is_empty() && bwlimit.is_none() && !on_change && key.is_none());
}

#[test]
fn add_accepts_remote_destination_and_key() {
    let cli = parse(&["rsync-tui", "add", "/a", "--to", "nas:/backup", "--key", "/home/u/.ssh/id_backup"]).unwrap();
    let Some(Command::Add { to, key, .. }) = cli.command else {
        panic!("esperava Add");
    };
    assert_eq!(to, "nas:/backup");
    assert_eq!(key.as_deref(), Some(Path::new("/home/u/.ssh/id_backup")));
}

#[test]
fn add_reads_all_options() {
    let cli = parse(&[
        "rsync-tui",
        "add",
        "/a",
        "--to",
        "/b",
        "--mode",
        "mirror",
        "--schedule",
        "hourly",
        "--name",
        "x",
        "--exclude",
        "*.tmp",
        "--exclude",
        ".cache",
        "--bwlimit",
        "500",
        "--on-change",
    ])
    .unwrap();
    let Some(Command::Add { mode, schedule, name, excludes, bwlimit, on_change, .. }) = cli.command else {
        panic!("esperava Add");
    };
    assert_eq!(mode, Mode::Mirror);
    assert_eq!(schedule, Schedule::Hourly);
    assert_eq!(name.as_deref(), Some("x"));
    assert_eq!(excludes, ["*.tmp", ".cache"]);
    assert_eq!(bwlimit, Some(500));
    assert!(on_change);
}

#[test]
fn add_without_destination_is_rejected() {
    assert!(parse(&["rsync-tui", "add", "/a"]).is_err());
}

#[test]
fn invalid_mode_is_rejected() {
    assert!(parse(&["rsync-tui", "add", "/a", "--to", "/b", "--mode", "sync"]).is_err());
}

#[test]
fn unknown_subcommand_is_rejected() {
    assert!(parse(&["rsync-tui", "sync"]).is_err());
}

#[test]
fn json_flag_works_before_or_after_subcommand() {
    assert!(parse(&["rsync-tui", "list", "--json"]).unwrap().json);
    assert!(parse(&["rsync-tui", "--json", "list"]).unwrap().json);
    assert!(!parse(&["rsync-tui", "list"]).unwrap().json);
}

#[test]
fn edit_reads_only_given_changes() {
    let cli = parse(&["rsync-tui", "edit", "fotos", "--mode", "mirror", "--on-change", "false", "--bwlimit", "0", "--yes"]).unwrap();
    let Some(Command::Edit { job, changes, confirm }) = cli.command else {
        panic!("esperava Edit");
    };
    assert_eq!(job, "fotos");
    assert_eq!(changes.mode, Some(Mode::Mirror));
    assert_eq!(changes.on_change, Some(false));
    assert_eq!(changes.bwlimit, Some(0));
    assert!(changes.source.is_none() && changes.to.is_none() && changes.schedule.is_none() && changes.key.is_none());
    assert!(confirm.yes && !confirm.dry_run);
}

#[test]
fn edit_rejects_conflicting_options() {
    assert!(parse(&["rsync-tui", "edit", "fotos", "--key", "/k", "--no-key"]).is_err());
    assert!(parse(&["rsync-tui", "edit", "fotos", "--exclude", "*.tmp", "--clear-excludes"]).is_err());
    assert!(parse(&["rsync-tui", "edit", "fotos", "--on-change", "talvez"]).is_err());
}

#[test]
fn remove_needs_explicit_yes_flag() {
    let cli = parse(&["rsync-tui", "remove", "fotos"]).unwrap();
    assert!(matches!(cli.command, Some(Command::Remove { yes: false, .. })));
    let cli = parse(&["rsync-tui", "remove", "fotos", "--yes"]).unwrap();
    assert!(matches!(cli.command, Some(Command::Remove { yes: true, .. })));
}

#[test]
fn add_accepts_dry_run() {
    let cli = parse(&["rsync-tui", "add", "/a", "--to", "/b", "--dry-run"]).unwrap();
    assert!(matches!(cli.command, Some(Command::Add { confirm, .. }) if confirm.dry_run));
}

#[test]
fn new_commands_are_parsed() {
    assert!(matches!(parse(&["rsync-tui", "preview", "fotos"]).unwrap().command, Some(Command::Preview { .. })));
    assert!(matches!(parse(&["rsync-tui", "show", "fotos"]).unwrap().command, Some(Command::Show { .. })));
    assert!(matches!(parse(&["rsync-tui", "discover"]).unwrap().command, Some(Command::Discover)));
    assert!(matches!(parse(&["rsync-tui", "run", "fotos", "--progress"]).unwrap().command, Some(Command::Run { progress: true, .. })));
    let cli = parse(&["rsync-tui", "trust-host", "nas:/backup", "--fingerprint", "SHA256:abc"]).unwrap();
    assert!(matches!(cli.command, Some(Command::TrustHost { target, fingerprint }) if target == "nas:/backup" && fingerprint.as_deref() == Some("SHA256:abc")));
}
