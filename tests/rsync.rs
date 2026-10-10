use std::ffi::OsString;
use std::fs;

use rsync_tui::job::{Job, Mode};
use rsync_tui::rsync::{self, Options, Stats};
use rsync_tui::state::Outcome;

const STATS: &str = "\
>f+++++++++ a.bin

Number of files: 4 (reg: 2, dir: 2)
Number of created files: 3 (reg: 2, dir: 1)
Number of deleted files: 1 (reg: 1)
Number of regular files transferred: 2
Total file size: 123459 bytes
Total transferred file size: 1,234,567 bytes
Literal data: 123459 bytes
";

fn args(job: &Job, opts: Options) -> Vec<String> {
    rsync::build_args(job, opts).into_iter().map(|a| a.to_string_lossy().into_owned()).collect()
}

fn job(mode: Mode) -> Job {
    Job::new("fotos", "/src".into(), "/dst".into(), mode)
}

#[test]
fn copy_mode_has_base_flags_and_trailing_slash() {
    let a = args(&job(Mode::Copy), Options::default());
    assert_eq!(&a[..2], ["-a", "--partial"]);
    assert!(a.contains(&"--stats".into()));
    assert!(!a.contains(&"--delete".into()));
    assert_eq!(&a[a.len() - 2..], ["/src/", "/dst"]);
}

#[test]
fn mirror_mode_adds_delete() {
    assert!(args(&job(Mode::Mirror), Options::default()).contains(&"--delete".into()));
}

#[test]
fn excludes_bwlimit_and_options_become_flags() {
    let mut j = job(Mode::Copy);
    j.excludes = vec!["*.tmp".into(), ".cache".into()];
    j.bwlimit_kbps = Some(500);
    let a = args(&j, Options { dry_run: true, progress: true });
    for flag in ["--exclude=*.tmp", "--exclude=.cache", "--bwlimit=500", "--dry-run", "--info=progress2"] {
        assert!(a.contains(&flag.into()), "{flag}");
    }
}

#[test]
fn existing_trailing_slash_is_not_doubled() {
    let j = Job::new("x", "/src/".into(), "/dst".into(), Mode::Copy);
    assert!(args(&j, Options::default()).contains(&"/src/".into()));
}

#[test]
fn parses_stats() {
    assert_eq!(rsync::parse_stats(STATS), Stats { files_transferred: 2, files_created: 3, files_deleted: 1, bytes_transferred: 1_234_567 });
}

#[test]
fn missing_stats_are_zero() {
    assert_eq!(rsync::parse_stats("nada aqui"), Stats::default());
}

#[test]
fn exit_codes_map_to_outcome() {
    assert_eq!(rsync::outcome(0), Outcome::Success);
    assert_eq!(rsync::outcome(24), Outcome::Warning);
    assert_eq!(rsync::outcome(23), Outcome::Failed);
    assert_eq!(rsync::outcome(-1), Outcome::Failed);
}

#[test]
fn real_rsync_copies_files() {
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();
    fs::create_dir(src.path().join("sub")).unwrap();
    fs::write(src.path().join("sub/a.txt"), "oi").unwrap();
    let j = Job::new("t", src.path().into(), dst.path().into(), Mode::Copy);

    let out = rsync::execute(&rsync::build_args(&j, Options::default())).unwrap();

    assert_eq!(out.exit_code, 0, "{}", out.stderr);
    assert_eq!(fs::read_to_string(dst.path().join("sub/a.txt")).unwrap(), "oi");
    assert_eq!(rsync::parse_stats(&out.stdout).files_transferred, 1);
}

#[test]
fn dry_run_changes_nothing() {
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "oi").unwrap();
    let j = Job::new("t", src.path().into(), dst.path().into(), Mode::Copy);

    let args: Vec<OsString> = rsync::build_args(&j, Options { dry_run: true, progress: false });
    rsync::execute(&args).unwrap();

    assert!(!dst.path().join("a.txt").exists());
}
