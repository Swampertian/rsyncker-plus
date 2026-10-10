use rsync_tui::state::{self, Outcome, RunState};

fn sample() -> RunState {
    RunState {
        started_at: 100,
        finished_at: 142,
        exit_code: 0,
        outcome: Outcome::Success,
        files_transferred: 12,
        files_deleted: 0,
        bytes_transferred: 48_213_004,
        message: None,
    }
}

#[test]
fn save_and_load_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    sample().save(dir.path(), "fotos").unwrap();
    assert_eq!(RunState::load(dir.path(), "fotos").unwrap(), Some(sample()));
}

#[test]
fn missing_state_is_none() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(RunState::load(dir.path(), "fotos").unwrap(), None);
}

#[test]
fn corrupted_state_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(RunState::file(dir.path(), "fotos"), "{").unwrap();
    assert!(RunState::load(dir.path(), "fotos").is_err());
}

#[test]
fn format_ago_buckets() {
    assert_eq!(state::format_ago(1_000, 1_030), "agora");
    assert_eq!(state::format_ago(1_000, 1_000 + 180), "há 3 min");
    assert_eq!(state::format_ago(0, 7_200), "há 2 h");
    assert_eq!(state::format_ago(0, 3 * 86_400), "há 3 d");
    assert_eq!(state::format_ago(500, 100), "agora");
}
