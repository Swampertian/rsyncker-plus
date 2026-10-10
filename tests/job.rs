use std::path::{Path, PathBuf};

use rsync_tui::job::{self, Job, Mode, Schedule};

fn job(source: &str, destination: &str) -> Job {
    Job::new("fotos", source.into(), destination.into(), Mode::Copy)
}

#[test]
fn parses_minimal_toml_with_defaults() {
    let job = Job::from_toml(
        "fotos",
        r#"
        source = "/home/u/Fotos"
        destination = "/mnt/backup/fotos"
        mode = "mirror"
        "#,
    )
    .unwrap();
    assert_eq!(job.name, "fotos");
    assert_eq!(job.mode, Mode::Mirror);
    assert_eq!(job.schedule, Schedule::None);
    assert!(job.enabled);
    assert!(job.excludes.is_empty());
}

#[test]
fn rejects_unknown_mode() {
    let text = "source = \"/a\"\ndestination = \"/b\"\nmode = \"sync\"\n";
    assert!(Job::from_toml("x", text).is_err());
}

#[test]
fn save_and_load_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let mut original = job("/a", "/b");
    original.schedule = Schedule::Weekly;
    original.excludes = vec!["*.tmp".into()];
    original.save(dir.path()).unwrap();
    assert_eq!(Job::load(dir.path(), "fotos").unwrap(), original);
}

#[test]
fn list_returns_jobs_sorted() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["zeta", "alfa"] {
        Job::new(name, "/a".into(), "/b".into(), Mode::Copy).save(dir.path()).unwrap();
    }
    let names: Vec<_> = Job::list(dir.path()).unwrap().into_iter().map(|j| j.name).collect();
    assert_eq!(names, ["alfa", "zeta"]);
}

#[test]
fn list_of_missing_dir_is_empty() {
    assert!(Job::list(Path::new("/nao/existe")).unwrap().is_empty());
}

#[test]
fn valid_job_passes() {
    assert!(job("/home/u/Fotos", "/mnt/backup/fotos").validate().is_ok());
}

#[test]
fn relative_paths_are_rejected() {
    assert!(job("Fotos", "/mnt/b").validate().is_err());
    assert!(job("/home/u/Fotos", "backup").validate().is_err());
}

#[test]
fn parent_dir_components_are_rejected() {
    assert!(job("/home/u/../x", "/mnt/b").validate().is_err());
}

#[test]
fn same_or_nested_paths_are_rejected() {
    assert!(job("/data", "/data").validate().is_err());
    assert!(job("/data", "/data/backup").validate().is_err());
    assert!(job("/data/fotos", "/data").validate().is_err());
}

#[test]
fn sibling_with_common_prefix_is_allowed() {
    assert!(job("/data/fotos", "/data/fotos-backup").validate().is_ok());
}

#[test]
fn invalid_names_are_rejected() {
    for name in ["", "Fotos", "a b", "a/b", "a.b"] {
        assert!(job::validate_name(name).is_err(), "{name:?}");
    }
    assert!(job::validate_name("fotos-2024_x").is_ok());
}

#[test]
fn load_and_delete_reject_path_traversal_names() {
    let dir = tempfile::tempdir().unwrap();

    assert!(Job::load(dir.path(), "../outside").is_err());
    assert!(Job::delete(dir.path(), "../outside").is_err());
}

#[test]
fn name_from_path_is_sanitized() {
    assert_eq!(job::name_from_path(&PathBuf::from("/home/u/Minhas Fotos")), "minhas-fotos");
    assert_eq!(job::name_from_path(&PathBuf::from("/home/u/docs/")), "docs");
}
