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
    assert!(matches!(cli.command, Some(Command::Run { job }) if job == "fotos"));
}

#[test]
fn add_uses_defaults() {
    let cli = parse(&["rsync-tui", "add", "/a", "--to", "/b"]).unwrap();
    let Some(Command::Add { source, to, mode, schedule, name, excludes, bwlimit, on_change }) = cli.command else {
        panic!("esperava Add");
    };
    assert_eq!(source, Path::new("/a"));
    assert_eq!(to, Path::new("/b"));
    assert_eq!(mode, Mode::Copy);
    assert_eq!(schedule, Schedule::Daily);
    assert!(name.is_none() && excludes.is_empty() && bwlimit.is_none() && !on_change);
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
