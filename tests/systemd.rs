use std::fs;
use std::path::Path;

use rsync_tui::daemon::systemd;
use rsync_tui::job::{Job, Mode, Schedule};
use rsync_tui::paths::Paths;

fn job(schedule: Schedule) -> Job {
    let mut j = Job::new("fotos", "/home/u/Fotos".into(), "/mnt/b k/fotos".into(), Mode::Copy);
    j.schedule = schedule;
    j
}

#[test]
fn service_template_runs_job_with_low_priority() {
    let unit = systemd::service_template(Path::new("/usr/bin/rsync-tui"));
    assert!(unit.contains("ExecStart=\"/usr/bin/rsync-tui\" run %i"));
    assert!(unit.contains("Type=oneshot"));
    assert!(unit.contains("Nice=19"));
    assert!(unit.contains("IOSchedulingClass=idle"));
}

#[test]
fn percent_in_exe_path_is_escaped() {
    let unit = systemd::service_template(Path::new("/opt/100%/rsync-tui"));
    assert!(unit.contains("/opt/100%%/rsync-tui"));
}

#[test]
fn timer_uses_preset_and_is_persistent() {
    let timer = systemd::timer_unit(&job(Schedule::Daily)).unwrap();
    assert!(timer.contains("OnCalendar=daily"));
    assert!(timer.contains("Persistent=true"));
    assert!(timer.contains("Unit=rsync-tui@fotos.service"));
    assert!(timer.contains("WantedBy=timers.target"));
}

#[test]
fn no_schedule_means_no_timer() {
    assert!(systemd::timer_unit(&job(Schedule::None)).is_none());
}

#[test]
fn mounts_dropin_quotes_paths() {
    let dropin = systemd::mounts_dropin(&job(Schedule::Daily));
    assert!(dropin.contains("RequiresMountsFor=\"/home/u/Fotos\" \"/mnt/b k/fotos\""));
}

#[test]
fn write_and_remove_units() {
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::under(root.path());
    let exe = Path::new("/usr/bin/rsync-tui");

    systemd::write_units(&paths, &job(Schedule::Hourly), exe).unwrap();
    let timer = paths.systemd.join("rsync-tui@fotos.timer");
    let dropin = paths.systemd.join("rsync-tui@fotos.service.d/mounts.conf");
    assert!(paths.systemd.join("rsync-tui@.service").exists());
    assert!(timer.exists());
    assert!(dropin.exists());

    systemd::write_units(&paths, &job(Schedule::None), exe).unwrap();
    assert!(!timer.exists());

    systemd::remove_units(&paths, "fotos").unwrap();
    assert!(!dropin.exists());
    assert!(paths.systemd.join("rsync-tui@.service").exists());
}

#[test]
fn watch_template_restarts_on_failure() {
    let unit = systemd::watch_template(Path::new("/usr/bin/rsync-tui"));
    assert!(unit.contains("ExecStart=\"/usr/bin/rsync-tui\" watch %i"));
    assert!(unit.contains("Restart=on-failure"));
    assert!(unit.contains("WantedBy=default.target"));
}

#[test]
fn watch_dropin_only_for_on_change_jobs() {
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::under(root.path());
    let exe = Path::new("/usr/bin/rsync-tui");
    let dropin = paths.systemd.join("rsync-tui-watch@fotos.service.d/mounts.conf");

    let mut watched = job(Schedule::None);
    watched.on_change = true;
    systemd::write_units(&paths, &watched, exe).unwrap();
    assert!(paths.systemd.join("rsync-tui-watch@.service").exists());
    assert!(fs::read_to_string(&dropin).unwrap().contains("RequiresMountsFor=\"/home/u/Fotos\""));

    systemd::write_units(&paths, &job(Schedule::None), exe).unwrap();
    assert!(!dropin.exists());

    systemd::write_units(&paths, &watched, exe).unwrap();
    systemd::remove_units(&paths, "fotos").unwrap();
    assert!(!dropin.exists());
}

#[test]
fn parses_watch_unit_state() {
    let text = "Id=rsync-tui-watch@fotos.service\nActiveState=active\n\nId=rsync-tui@fotos.service\nActiveState=inactive\n";
    let status = systemd::parse_status(text);
    assert!(status["fotos"].watching);
    assert!(!status["fotos"].running);
}

#[test]
fn parses_systemctl_show_output() {
    let text = "Id=rsync-tui@fotos.service\nActiveState=activating\n\n\
                NextElapseUSecRealtime=Fri 2026-10-09 21:00:00 -03\nId=rsync-tui@fotos.timer\nActiveState=active\n\n\
                Id=rsync-tui@docs.service\nActiveState=inactive\n\n\
                NextElapseUSecRealtime=\nId=rsync-tui@docs.timer\nActiveState=inactive\n";
    let status = systemd::parse_status(text);
    assert!(status["fotos"].running);
    assert_eq!(status["fotos"].next_run.as_deref(), Some("Fri 2026-10-09 21:00:00 -03"));
    assert!(!status["docs"].running);
    assert_eq!(status["docs"].next_run, None);
}
