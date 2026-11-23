//! Establish this test-owned process before running either one-shot identity probe.

use super::super::*;
use std::os::unix::ffi::OsStrExt as _;
use std::path::PathBuf;

pub(super) struct Expected<'a> {
    pub pane: &'a str,
    pub instance: &'a str,
    pub home: &'a Path,
    pub codex_home: Option<&'a Path>,
    pub cwd: &'a Path,
    pub rollout: &'a Path,
    pub first_line: &'a str,
}

#[derive(Clone, Debug)]
struct Snapshot {
    comm: String,
    environment: Vec<u8>,
    cwd: PathBuf,
    rollout_fd: PathBuf,
    first_line: String,
}

fn check(snapshot: &Snapshot, expected: &Expected<'_>) -> Result<(), &'static str> {
    if !is_codex_process_name(snapshot.comm.trim()) {
        return Err("comm is not a Codex fixture process name");
    }
    for (key, value) in [
        (b"PEBREL_PANE_ID".as_slice(), expected.pane.as_bytes()),
        (b"NEBULA_PANE_ID".as_slice(), expected.pane.as_bytes()),
        (crate::agent_env::PROCESS_ENV.as_bytes(), expected.instance.as_bytes()),
        (b"HOME".as_slice(), expected.home.as_os_str().as_bytes()),
    ] {
        if !environment_has_value(&snapshot.environment, key, value) {
            return Err("environ does not contain the fixture's pane, instance and HOME");
        }
    }
    match expected.codex_home {
        Some(home)
            if !environment_has_value(
                &snapshot.environment,
                b"CODEX_HOME",
                home.as_os_str().as_bytes(),
            ) =>
        {
            return Err("environ CODEX_HOME does not match this fixture case");
        },
        None if snapshot
            .environment
            .split(|byte| *byte == 0)
            .any(|item| item.starts_with(b"CODEX_HOME=")) =>
        {
            return Err("environ unexpectedly contains CODEX_HOME in the fallback case");
        },
        _ => {},
    }
    if snapshot.cwd != expected.cwd {
        return Err("cwd is not the owned fixture root");
    }
    if snapshot.rollout_fd != expected.rollout {
        return Err("fd/2 does not point to the owned rollout");
    }
    if snapshot.first_line != expected.first_line {
        return Err("fd/2 metadata does not match the owned rollout");
    }
    Ok(())
}

fn read_snapshot(proc_dir: &Path, owned_rollout: &Path) -> io::Result<Snapshot> {
    let rollout_fd = proc_dir.join("fd/2");
    let link = std::fs::read_link(&rollout_fd)?;
    if link != owned_rollout {
        return Err(io::Error::other("fixture fd/2 is not the owned rollout"));
    }
    // Never open a pipe/device when a failed fixture did not inherit our file.
    if !std::fs::metadata(&rollout_fd)?.is_file() {
        return Err(io::Error::other("fixture fd/2 is not a regular file"));
    }
    Ok(Snapshot {
        comm: String::from_utf8(read_bounded(std::fs::File::open(proc_dir.join("comm"))?, 512)?)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?,
        environment: read_bounded(
            std::fs::File::open(proc_dir.join("environ"))?,
            MAX_PROBE_OUTPUT,
        )?,
        cwd: std::fs::read_link(proc_dir.join("cwd"))?,
        rollout_fd: link,
        first_line: read_first_line(&rollout_fd)?,
    })
}

#[cfg(target_os = "linux")]
pub(super) fn wait_for_child(
    child: &mut std::process::Child,
    expected: &Expected<'_>,
) -> Result<(), String> {
    const READY_TIMEOUT: Duration = Duration::from_secs(1);
    const POLL_INTERVAL: Duration = Duration::from_millis(5);
    // Only this owned child is inspected. Do not scan other processes, call
    // either identity probe, or wait for its expected positive/negative result.
    let proc_dir = Path::new("/proc").join(child.id().to_string());
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            return Err(format!("owned child {} exited early: {status}", child.id()));
        }
        let missing = match read_snapshot(&proc_dir, expected.rollout) {
            Ok(snapshot) => match check(&snapshot, expected) {
                Ok(()) => return Ok(()),
                Err(error) => error.to_owned(),
            },
            Err(error) => format!("owned proc snapshot: {error}"),
        };
        if started.elapsed() >= READY_TIMEOUT {
            return Err(format!(
                "child={} elapsed={:?} missing={missing}",
                child.id(),
                started.elapsed()
            ));
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

fn environment(pane: &str, instance: &str, home: &Path, codex_home: Option<&Path>) -> Vec<u8> {
    let mut bytes = format!(
        "PEBREL_PANE_ID={pane}\0NEBULA_PANE_ID={pane}\0{}={instance}\0HOME=",
        crate::agent_env::PROCESS_ENV
    )
    .into_bytes();
    bytes.extend_from_slice(home.as_os_str().as_bytes());
    bytes.push(0);
    if let Some(codex_home) = codex_home {
        bytes.extend_from_slice(b"CODEX_HOME=");
        bytes.extend_from_slice(codex_home.as_os_str().as_bytes());
        bytes.push(0);
    }
    bytes
}

#[test]
fn readiness_rejects_missing_or_wrong_owned_process_fields() {
    let expected = Expected {
        pane: "owned-pane",
        instance: "owned-instance",
        home: Path::new("/owned/home"),
        codex_home: Some(Path::new("relative codex")),
        cwd: Path::new("/owned/root"),
        rollout: Path::new("/owned/rollout.jsonl"),
        first_line: "synthetic metadata",
    };
    let ready = Snapshot {
        comm: "codex\n".into(),
        environment: environment(
            expected.pane,
            expected.instance,
            expected.home,
            expected.codex_home,
        ),
        cwd: expected.cwd.into(),
        rollout_fd: expected.rollout.into(),
        first_line: expected.first_line.into(),
    };
    assert_eq!(check(&ready, &expected), Ok(()));
    let mut wrong = ready.clone();
    wrong.comm = "sleep".into();
    assert!(check(&wrong, &expected).unwrap_err().starts_with("comm"));
    for (pane, instance, home, codex_home) in [
        ("other-pane", expected.instance, expected.home, expected.codex_home),
        (expected.pane, "other-instance", expected.home, expected.codex_home),
        (expected.pane, expected.instance, Path::new("/other/home"), expected.codex_home),
        (expected.pane, expected.instance, expected.home, None),
        (expected.pane, expected.instance, expected.home, Some(Path::new("/other/codex"))),
    ] {
        let mut wrong = ready.clone();
        wrong.environment = environment(pane, instance, home, codex_home);
        assert!(check(&wrong, &expected).unwrap_err().starts_with("environ"));
    }
    let mut wrong = ready.clone();
    wrong.cwd = "/other/root".into();
    assert!(check(&wrong, &expected).unwrap_err().starts_with("cwd"));
    let mut wrong = ready.clone();
    wrong.rollout_fd = "/other/rollout.jsonl".into();
    assert!(check(&wrong, &expected).unwrap_err().starts_with("fd/2"));
    let mut wrong = ready.clone();
    wrong.first_line.clear();
    assert!(check(&wrong, &expected).unwrap_err().contains("metadata"));
    let fallback = Expected { codex_home: None, ..expected };
    assert!(check(&ready, &fallback).unwrap_err().contains("unexpectedly"));
    let mut ready = ready;
    ready.environment = environment(fallback.pane, fallback.instance, fallback.home, None);
    assert_eq!(check(&ready, &fallback), Ok(()));
    ready.environment.extend_from_slice(b"CODEX_HOME=\0");
    assert!(check(&ready, &fallback).unwrap_err().contains("unexpectedly"));
}

#[test]
fn owned_snapshot_checks_real_cwd_and_regular_rollout_fd_links() {
    use std::os::unix::fs::symlink;
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let proc = root.join("owned-proc");
    std::fs::create_dir_all(proc.join("fd")).unwrap();
    let rollout = root.join("rollout.jsonl");
    std::fs::write(&rollout, "synthetic metadata\nprivate body is not needed\n").unwrap();
    let expected = Expected {
        pane: "owned-pane",
        instance: "owned-instance",
        home: &root,
        codex_home: None,
        cwd: &root,
        rollout: &rollout,
        first_line: "synthetic metadata",
    };
    std::fs::write(proc.join("comm"), "codex\n").unwrap();
    std::fs::write(
        proc.join("environ"),
        environment(expected.pane, expected.instance, &root, None),
    )
    .unwrap();
    symlink(&root, proc.join("cwd")).unwrap();
    symlink(&rollout, proc.join("fd/2")).unwrap();
    let snapshot = read_snapshot(&proc, &rollout).unwrap();
    assert_eq!(check(&snapshot, &expected), Ok(()));
    std::fs::remove_file(proc.join("fd/2")).unwrap();
    symlink(&root, proc.join("fd/2")).unwrap();
    assert!(
        read_snapshot(&proc, &rollout).unwrap_err().to_string().contains("not the owned rollout")
    );
}
