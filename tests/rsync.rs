use std::ffi::OsString;
use std::fs;

use rsync_tui::job::{Job, Mode};
use rsync_tui::rsync::{self, Cancel, Change, Item, Options, Progress, Stats};
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

#[test]
fn itemize_lines_become_changes() {
    let item = |change, path: &str| Some(Item { change, path: path.to_string() });
    assert_eq!(rsync::parse_item(">f+++++++++ big"), item(Change::Created, "big"));
    assert_eq!(rsync::parse_item("cd+++++++++ sub/"), item(Change::Created, "sub/"));
    assert_eq!(rsync::parse_item(">f.s....... a"), item(Change::Updated, "a"));
    assert_eq!(rsync::parse_item("*deleting   old/x"), item(Change::Deleted, "old/x"));
    assert_eq!(rsync::parse_item(">f+++++++++ nome com espaço"), item(Change::Created, "nome com espaço"));
    assert_eq!(rsync::parse_item(".d..t...... sub/"), None);
    assert_eq!(rsync::parse_item("Number of files: 5 (reg: 3, dir: 2)"), None);
    assert_eq!(rsync::parse_item("sent 137 bytes  received 49 bytes  372.00 bytes/sec"), None);
    assert_eq!(rsync::parse_item(""), None);
}

#[test]
fn progress_lines_are_parsed() {
    let progress = rsync::parse_progress("        3000002  99%    1.40GB/s    0:00:00 (xfr#2, to-chk=2/5)");
    assert_eq!(progress, Some(Progress { bytes: 3_000_002, percent: 99, rate: "1.40GB/s".to_string(), eta: "0:00:00".to_string() }));
    assert_eq!(rsync::parse_progress(">f+++++++++ big"), None);
    assert_eq!(rsync::parse_progress("Number of files: 5"), None);
}

#[test]
fn progress_flags_are_added() {
    let a = args(&job(Mode::Copy), Options { dry_run: false, progress: true });
    assert!(a.contains(&"--no-inc-recursive".to_string()));
}

#[test]
fn streaming_reports_each_line() {
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "oi").unwrap();
    let j = Job::new("t", src.path().into(), dst.path().into(), Mode::Copy);

    let mut lines = Vec::new();
    let out =
        rsync::execute_streaming(&rsync::build_args(&j, Options { dry_run: false, progress: true }), &Cancel::default(), |line| lines.push(line.to_string()))
            .unwrap();

    assert_eq!(out.exit_code, 0, "{}", out.stderr);
    assert!(lines.iter().any(|l| rsync::parse_item(l) == Some(Item { change: Change::Created, path: "a.txt".to_string() })));
    assert!(lines.iter().any(|l| rsync::parse_progress(l).is_some_and(|p| p.percent == 100)));
    assert_eq!(rsync::parse_stats(&out.stdout).files_transferred, 1);
}

#[test]
fn cancelled_run_fails_and_says_why() {
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();
    fs::write(src.path().join("a.txt"), "oi").unwrap();
    let j = Job::new("t", src.path().into(), dst.path().into(), Mode::Copy);
    let cancel = Cancel::default();
    cancel.cancel();

    let out = rsync::execute_streaming(&rsync::build_args(&j, Options::default()), &cancel, |_| {}).unwrap();

    assert_ne!(out.exit_code, 0);
    assert!(out.stderr.contains("cancelado pelo usuário"));
    assert_eq!(rsync::outcome(out.exit_code), Outcome::Failed);
}

#[test]
fn cancel_stops_a_running_copy() {
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();
    fs::write(src.path().join("big.bin"), vec![7_u8; 5_000_000]).unwrap();
    let mut j = Job::new("t", src.path().into(), dst.path().into(), Mode::Copy);
    j.bwlimit_kbps = Some(100);
    let cancel = Cancel::default();
    let canceller = cancel.clone();
    let started = std::time::Instant::now();
    let stopper = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        canceller.cancel();
    });

    let out = rsync::execute_streaming(&rsync::build_args(&j, Options { dry_run: false, progress: true }), &cancel, |_| {}).unwrap();
    stopper.join().unwrap();

    assert!(started.elapsed() < std::time::Duration::from_secs(5), "cancelamento demorou {:?}", started.elapsed());
    assert_eq!(rsync::outcome(out.exit_code), Outcome::Failed);
    assert!(out.stderr.contains("cancelado pelo usuário"));
}
