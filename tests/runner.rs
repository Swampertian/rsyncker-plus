use std::fs::{self, OpenOptions};

use fs2::FileExt;
use rsync_tui::daemon::runner;
use rsync_tui::job::{Job, Mode};
use rsync_tui::paths::Paths;
use rsync_tui::state::{Outcome, RunState};
use tempfile::TempDir;

struct Fixture {
    root: TempDir,
    paths: Paths,
}

fn fixture(mode: Mode) -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::under(root.path());
    let src = root.path().join("src");
    let dst = root.path().join("dst");
    fs::create_dir_all(&src).unwrap();
    fs::create_dir_all(&dst).unwrap();
    fs::write(src.join("a.txt"), "oi").unwrap();
    Job::new("fotos", src, dst, mode).save(&paths.jobs).unwrap();
    Fixture { root, paths }
}

#[test]
fn successful_run_copies_and_saves_state() {
    let f = fixture(Mode::Copy);
    let result = runner::run(&f.paths, "fotos").unwrap();

    assert_eq!(result.outcome, Outcome::Success);
    assert_eq!(result.files_transferred, 1);
    assert!(f.root.path().join("dst/a.txt").exists());
    assert_eq!(RunState::load(&f.paths.state, "fotos").unwrap(), Some(result));
}

#[test]
fn missing_destination_fails_without_creating_it() {
    let f = fixture(Mode::Copy);
    let dst = f.root.path().join("dst");
    fs::remove_dir_all(&dst).unwrap();

    let result = runner::run(&f.paths, "fotos").unwrap();

    assert_eq!(result.outcome, Outcome::Failed);
    assert!(result.message.unwrap().contains("destino não existe"));
    assert!(!dst.exists());
}

#[test]
fn mirror_deletes_extra_files_in_destination() {
    let f = fixture(Mode::Mirror);
    let extra = f.root.path().join("dst/extra.txt");
    fs::write(&extra, "x").unwrap();

    let result = runner::run(&f.paths, "fotos").unwrap();

    assert_eq!(result.files_deleted, 1);
    assert!(!extra.exists());
}

#[test]
fn copy_keeps_extra_files_in_destination() {
    let f = fixture(Mode::Copy);
    let extra = f.root.path().join("dst/extra.txt");
    fs::write(&extra, "x").unwrap();

    runner::run(&f.paths, "fotos").unwrap();

    assert!(extra.exists());
}

#[test]
fn unknown_job_is_an_error() {
    let f = fixture(Mode::Copy);
    assert!(runner::run(&f.paths, "nao-existe").is_err());
}

#[test]
fn active_lock_prevents_a_second_run() {
    let f = fixture(Mode::Copy);
    fs::create_dir_all(&f.paths.state).unwrap();
    let lock = OpenOptions::new().create(true).truncate(false).read(true).write(true).open(f.paths.state.join("fotos.lock")).unwrap();
    lock.try_lock_exclusive().unwrap();

    let error = runner::run(&f.paths, "fotos").unwrap_err();

    assert!(error.to_string().contains("já está em execução"));
    let error = rsync_tui::manage::remove(&f.paths, "fotos").unwrap_err();
    assert!(error.to_string().contains("já está em execução"));
    assert!(Job::file(&f.paths.jobs, "fotos").exists());
}

#[cfg(unix)]
#[test]
fn preflight_rejects_destination_symlink_nested_in_source() {
    use std::os::unix::fs::symlink;

    let f = fixture(Mode::Mirror);
    let source = f.root.path().join("src");
    let nested_destination = source.join("nested-destination");
    fs::create_dir(&nested_destination).unwrap();
    let destination_link = f.root.path().join("destination-link");
    symlink(&nested_destination, &destination_link).unwrap();
    Job::new("fotos", source, destination_link, Mode::Mirror).save(&f.paths.jobs).unwrap();

    let result = runner::run(&f.paths, "fotos").unwrap();

    assert_eq!(result.outcome, Outcome::Failed);
    assert!(result.message.unwrap().contains("após resolver links simbólicos"));
    assert!(nested_destination.is_dir());
}
