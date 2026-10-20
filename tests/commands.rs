use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use rsync_tui::cli::Changes;
use rsync_tui::commands::{apply_changes, needs_check, needs_confirmation};
use rsync_tui::daemon::systemd::UnitStatus;
use rsync_tui::hostkey::{self, Unknown};
use rsync_tui::job::{Job, Mode, Schedule};
use rsync_tui::paths::Paths;
use rsync_tui::remote::Side;
use rsync_tui::report::{JobReport, Status};
use rsync_tui::state::{Outcome, RunState};
use serde_json::Value;
use tempfile::TempDir;

fn local_job(root: &Path) -> Job {
    let mut job = Job::new("fotos", root.join("src"), root.join("dst"), Mode::Copy);
    job.schedule = Schedule::Daily;
    job.excludes = vec!["*.tmp".to_string()];
    job.bwlimit_kbps = Some(500);
    job
}

#[test]
fn edit_keeps_what_was_not_given() {
    let original = local_job(Path::new("/data"));
    let edited = apply_changes(&original, &Changes { schedule: Some(Schedule::Weekly), ..Changes::default() }).unwrap();
    assert_eq!(edited.schedule, Schedule::Weekly);
    assert_eq!(edited.source, original.source);
    assert_eq!(edited.destination, original.destination);
    assert_eq!(edited.mode, original.mode);
    assert_eq!(edited.excludes, original.excludes);
    assert_eq!(edited.bwlimit_kbps, original.bwlimit_kbps);
    assert_eq!(edited.enabled, original.enabled);
}

#[test]
fn edit_replaces_and_clears_lists_and_limits() {
    let original = local_job(Path::new("/data"));
    let edited =
        apply_changes(&original, &Changes { excludes: vec![".cache".to_string()], bwlimit: Some(0), on_change: Some(true), ..Changes::default() }).unwrap();
    assert_eq!(edited.excludes, [".cache"]);
    assert_eq!(edited.bwlimit_kbps, None);
    assert!(edited.on_change);

    let cleared = apply_changes(&original, &Changes { clear_excludes: true, ..Changes::default() }).unwrap();
    assert!(cleared.excludes.is_empty());
}

#[test]
fn edit_moves_destination_to_remote_with_key() {
    let original = local_job(Path::new("/data"));
    let changes = Changes { to: Some("nas:/backup".to_string()), key: Some(PathBuf::from("/k/id")), ..Changes::default() };
    let edited = apply_changes(&original, &changes).unwrap();
    let remote = edited.remote.as_ref().unwrap();
    assert_eq!(remote.side, Side::Destination);
    assert_eq!(remote.host, "nas");
    assert_eq!(remote.identity.as_deref(), Some(Path::new("/k/id")));
    assert_eq!(edited.destination, Path::new("/backup"));
}

#[test]
fn edit_back_to_local_drops_old_key() {
    let original = local_job(Path::new("/data"));
    let remote = apply_changes(&original, &Changes { to: Some("nas:/backup".to_string()), key: Some(PathBuf::from("/k/id")), ..Changes::default() }).unwrap();
    let local = apply_changes(&remote, &Changes { to: Some("/data/dst".to_string()), ..Changes::default() }).unwrap();
    assert!(local.remote.is_none());
}

#[test]
fn mirror_needs_confirmation_only_when_risk_changes() {
    let root = Path::new("/data");
    let copy = local_job(root);
    let mut mirror = local_job(root);
    mirror.mode = Mode::Mirror;
    let mut moved = mirror.clone();
    moved.destination = root.join("outro");

    assert!(!needs_confirmation(&copy, None));
    assert!(needs_confirmation(&mirror, None));
    assert!(needs_confirmation(&mirror, Some(&copy)));
    assert!(!needs_confirmation(&mirror, Some(&mirror)));
    assert!(needs_confirmation(&moved, Some(&mirror)));
    assert!(!needs_confirmation(&copy, Some(&mirror)));
}

#[test]
fn check_runs_for_new_jobs_and_changed_places() {
    let root = Path::new("/data");
    let original = local_job(root);
    let mut schedule_only = original.clone();
    schedule_only.schedule = Schedule::Hourly;
    let mut moved = original.clone();
    moved.source = root.join("outra");

    assert!(needs_check(&original, None));
    assert!(!needs_check(&schedule_only, Some(&original)));
    assert!(needs_check(&moved, Some(&original)));
}

#[test]
fn fingerprint_must_match_exactly() {
    let unknown = Unknown {
        host: "nas".to_string(),
        fingerprints: vec!["SHA256:abc123 (ED25519)".to_string()],
        entries: String::new(),
        file: PathBuf::from("/tmp/known_hosts"),
    };
    assert!(hostkey::matches(&unknown, "SHA256:abc123"));
    assert!(hostkey::matches(&unknown, " SHA256:abc123 (ED25519) "));
    assert!(!hostkey::matches(&unknown, "SHA256:abc12"));
    assert!(!hostkey::matches(&unknown, ""));
}

#[test]
fn status_follows_dashboard_rules() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("dst")).unwrap();
    let job = local_job(root.path());
    let failed = RunState {
        started_at: 0,
        finished_at: 0,
        exit_code: 23,
        outcome: Outcome::Failed,
        files_transferred: 0,
        files_deleted: 0,
        bytes_transferred: 0,
        message: None,
    };
    assert_eq!(Status::of(&job, None, false), Status::Never);
    assert_eq!(Status::of(&job, Some(&failed), false), Status::Failed);
    assert_eq!(Status::of(&job, Some(&failed), true), Status::Running);

    let missing = local_job(Path::new("/nao/existe"));
    assert_eq!(Status::of(&missing, None, false), Status::DestinationMissing);

    let report = JobReport::new(&job, Some(failed), &UnitStatus::default());
    let value = serde_json::to_value(&report).unwrap();
    assert_eq!(value["status"], "failed");
    assert_eq!(value["mode"], "copy");
    assert_eq!(value["schedule"], "daily");
    assert_eq!(value["last_run"]["outcome"], "failed");
}

struct Cli {
    root: TempDir,
}

impl Cli {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let paths = Paths::under(&root.path().join("config/rsync-tui"));
        for dir in ["src", "dst"] {
            fs::create_dir_all(root.path().join(dir)).unwrap();
        }
        fs::write(root.path().join("src/a.txt"), "oi").unwrap();
        fs::write(root.path().join("dst/velho.txt"), "x").unwrap();
        let mut job = Job::new("fotos", root.path().join("src"), root.path().join("dst"), Mode::Mirror);
        job.schedule = Schedule::Daily;
        job.save(&paths.jobs).unwrap();
        Self { root }
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_rsync-tui"))
            .args(args)
            .env("HOME", self.root.path())
            .env("XDG_CONFIG_HOME", self.root.path().join("config"))
            .env("XDG_STATE_HOME", self.root.path().join("state"))
            .output()
            .unwrap()
    }

    fn json(&self, args: &[&str]) -> Value {
        let output = self.run(args);
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        serde_json::from_slice(&output.stdout).unwrap()
    }
}

#[test]
fn preview_reports_changes_without_touching_files() {
    let cli = Cli::new();
    let value = cli.json(&["preview", "fotos", "--json"]);
    assert_eq!(value["created"], serde_json::json!(["a.txt"]));
    assert_eq!(value["deleted"], serde_json::json!(["velho.txt"]));
    assert_eq!(value["outcome"], "success");
    assert!(!cli.root.path().join("dst/a.txt").exists());
    assert!(cli.root.path().join("dst/velho.txt").exists());
}

#[test]
fn run_reports_result_as_json() {
    let cli = Cli::new();
    let value = cli.json(&["run", "fotos", "--json"]);
    assert_eq!(value["job"], "fotos");
    assert_eq!(value["outcome"], "success");
    assert_eq!(value["files_transferred"], 1);
    assert!(cli.root.path().join("dst/a.txt").exists());
}

#[test]
fn run_streams_changed_files_as_text() {
    let cli = Cli::new();
    let output = cli.run(&["run", "fotos"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("+ a.txt"), "{stdout}");
    assert!(stdout.contains("- velho.txt"), "{stdout}");
    assert!(stdout.contains("concluído"), "{stdout}");
}

#[test]
fn show_and_list_describe_the_job() {
    let cli = Cli::new();
    let shown = cli.json(&["show", "fotos", "--json"]);
    assert_eq!(shown["name"], "fotos");
    assert_eq!(shown["mode"], "mirror");
    assert_eq!(shown["status"], "never");
    let listed = cli.json(&["--json", "list"]);
    assert_eq!(listed[0]["name"], "fotos");
}

#[test]
fn remove_without_yes_changes_nothing() {
    let cli = Cli::new();
    let output = cli.run(&["remove", "fotos"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--yes"));
    assert!(cli.root.path().join("config/rsync-tui/jobs/fotos.toml").exists());
}

#[test]
fn add_mirror_without_yes_is_refused() {
    let cli = Cli::new();
    let output = cli.run(&[
        "add",
        cli.root.path().join("src").to_str().unwrap(),
        "--to",
        cli.root.path().join("dst").to_str().unwrap(),
        "--name",
        "outro",
        "--mode",
        "mirror",
    ]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--yes"));
    assert!(!cli.root.path().join("config/rsync-tui/jobs/outro.toml").exists());
}

#[test]
fn add_dry_run_previews_without_saving() {
    let cli = Cli::new();
    let src = cli.root.path().join("src");
    let dst = cli.root.path().join("dst");
    let value = cli.json(&["add", src.to_str().unwrap(), "--to", dst.to_str().unwrap(), "--name", "outro", "--dry-run", "--json"]);
    assert_eq!(value["job"], "outro");
    assert_eq!(value["created"], serde_json::json!(["a.txt"]));
    assert_eq!(value["deleted"], serde_json::json!([]));
    assert!(!cli.root.path().join("config/rsync-tui/jobs/outro.toml").exists());
}

#[test]
fn local_job_has_no_host_to_trust() {
    let cli = Cli::new();
    let output = cli.run(&["trust-host", "fotos"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("é local"));
}
