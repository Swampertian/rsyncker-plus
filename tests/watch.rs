use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use notify::event::{AccessKind, CreateKind};
use notify::{Event, EventKind};
use rsync_tui::daemon::watch::{self, Debounce};
use rsync_tui::job::{Job, Mode};

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

#[test]
fn nothing_to_do_without_events() {
    let mut debounce = Debounce::new(secs(60), secs(600), secs(0));
    assert_eq!(debounce.deadline(), None);
    assert!(!debounce.fire_if_due(Instant::now()));
}

#[test]
fn fires_after_quiet_period() {
    let start = Instant::now();
    let mut debounce = Debounce::new(secs(60), secs(600), secs(0));
    debounce.record(start);
    debounce.record(start + secs(30));
    assert!(!debounce.fire_if_due(start + secs(89)));
    assert!(debounce.fire_if_due(start + secs(90)));
    assert!(!debounce.fire_if_due(start + secs(200)));
}

#[test]
fn continuous_changes_fire_at_max_wait() {
    let start = Instant::now();
    let mut debounce = Debounce::new(secs(60), secs(600), secs(0));
    for t in (0..=600).step_by(10) {
        debounce.record(start + secs(t));
    }
    assert!(debounce.fire_if_due(start + secs(600)));
}

#[test]
fn cooldown_delays_next_run() {
    let start = Instant::now();
    let mut debounce = Debounce::new(secs(10), secs(600), secs(120));
    debounce.record(start);
    assert!(debounce.fire_if_due(start + secs(10)));
    debounce.record(start + secs(20));
    assert!(!debounce.fire_if_due(start + secs(100)));
    assert!(debounce.fire_if_due(start + secs(130)));
}

#[test]
fn excludes_match_like_rsync() {
    let source = Path::new("/src");
    let excludes = vec![".cache".to_string(), "*.tmp".to_string(), "/build/".to_string(), "docs/old".to_string()];
    let ignored = |path: &str| watch::ignored(source, Path::new(path), &excludes);

    assert!(ignored("/src/.cache"));
    assert!(ignored("/src/a/.cache/file"));
    assert!(ignored("/src/a/notes.tmp"));
    assert!(ignored("/src/build/out.o"));
    assert!(!ignored("/src/a/build/out.o"));
    assert!(ignored("/src/x/docs/old/f"));
    assert!(!ignored("/src/docs/new/f"));
    assert!(!ignored("/src/photo.jpg"));
    assert!(!ignored("/src/tmp"));
}

fn job_at(source: PathBuf) -> Job {
    let mut job = Job::new("t", source, "/dst".into(), Mode::Copy);
    job.on_change = true;
    job.excludes = vec!["*.tmp".to_string()];
    job
}

#[test]
fn access_and_excluded_events_are_not_relevant() {
    let job = job_at("/src".into());
    let create = |path: &str| Event::new(EventKind::Create(CreateKind::File)).add_path(path.into());

    assert!(watch::relevant(&job, &create("/src/a.jpg")));
    assert!(!watch::relevant(&job, &create("/src/a.tmp")));
    assert!(!watch::relevant(&job, &Event::new(EventKind::Access(AccessKind::Any)).add_path("/src/a.jpg".into())));
}

#[test]
fn watcher_triggers_after_change_in_subfolder() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("src");
    fs::create_dir_all(source.join("sub")).unwrap();
    let mut job = job_at(source.clone());
    job.debounce_secs = 0;
    job.cooldown_secs = 0;

    let (tx, rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        watch::watch(&job, || {
            tx.send(()).unwrap();
            false
        })
    });

    let deadline = Instant::now() + secs(10);
    let mut triggered = false;
    let mut i = 0;
    while Instant::now() < deadline {
        fs::write(source.join("sub").join("ignored.tmp"), "x").unwrap();
        fs::write(source.join("sub").join(format!("f{i}.txt")), "x").unwrap();
        i += 1;
        if rx.recv_timeout(Duration::from_millis(200)).is_ok() {
            triggered = true;
            break;
        }
    }
    assert!(triggered, "watcher não disparou");
    handle.join().unwrap().unwrap();
}

#[test]
fn watcher_ignores_only_excluded_changes() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("src");
    fs::create_dir_all(&source).unwrap();
    let mut job = job_at(source.clone());
    job.debounce_secs = 0;

    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = watch::watch(&job, || {
            let _ = tx.send(());
            false
        });
    });

    for _ in 0..10 {
        fs::write(source.join("a.tmp"), "x").unwrap();
        thread::sleep(Duration::from_millis(50));
    }
    assert!(rx.recv_timeout(Duration::from_millis(500)).is_err());
}

#[test]
fn moving_source_away_stops_watcher_with_error() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("src");
    fs::create_dir_all(&source).unwrap();
    let job = job_at(source.clone());

    let handle = thread::spawn(move || watch::watch(&job, || true));
    thread::sleep(Duration::from_millis(300));
    fs::rename(&source, root.path().join("gone")).unwrap();
    fs::create_dir_all(&source).unwrap();

    let error = handle.join().unwrap().unwrap_err();
    assert!(error.to_string().contains("desmontada"));
}

#[test]
fn missing_source_is_an_error() {
    let job = job_at("/nao/existe/rsync-tui".into());
    assert!(watch::watch(&job, || false).is_err());
}

#[test]
fn old_job_files_get_watch_defaults() {
    let job = Job::from_toml("x", "source = \"/a\"\ndestination = \"/b\"\nmode = \"copy\"\n").unwrap();
    assert!(!job.on_change);
    assert_eq!((job.debounce_secs, job.max_wait_secs, job.cooldown_secs), (60, 600, 120));
}
