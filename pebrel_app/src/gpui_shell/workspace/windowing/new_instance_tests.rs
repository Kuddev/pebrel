use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::process::Command;
use std::time::Duration;

use clap::Parser;

use super::*;
use crate::gpui_shell::workspace::session_persistence::{SaveReason, SessionPersistence};
use crate::platform::startup::instance;
use crate::runtime_api::{ENDPOINT_ENV, RuntimeHub, RuntimeServer, apply_child_endpoint};

#[test]
fn new_instance_is_opt_in_and_conflicts_with_noninteractive_launch_modes() {
    assert!(!crate::cli::Options::try_parse_from(["pebrel"]).unwrap().new_instance);
    for conflicting in [
        "--daemon",
        "--ref-test",
        "--launched-at-login",
        #[cfg(feature = "legacy-shell")]
        "--legacy-shell",
    ] {
        assert!(
            crate::cli::Options::try_parse_from(["pebrel", "--new-instance", conflicting]).is_err()
        );
    }
    let options =
        crate::cli::Options::try_parse_from(["pebrel", "--new-instance", "--shell", "bash"])
            .unwrap();
    assert!(options.new_instance);
    assert_eq!(options.window_options.terminal_options.shell_id().as_deref(), Some("bash"));
}

#[test]
fn new_instance_process_keeps_shared_endpoint_and_session_untouched() {
    const CHILD: &str = "PEBREL_TEST_NEW_INSTANCE";
    if std::env::var_os(CHILD).is_none() {
        // A subprocess owns the real process-scoped launch intent and endpoint;
        // parallel tests must never change those globals in the parent harness.
        let directory = tempfile::tempdir().unwrap();
        let test_name = format!(
            "{}::new_instance_process_keeps_shared_endpoint_and_session_untouched",
            module_path!().split_once("::").unwrap().1
        );
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &test_name, "--nocapture"])
            .env(CHILD, "1")
            .env("PEBREL_CONFIG_DIR", directory.path())
            .env("NEBULA_CONFIG_DIR", directory.path())
            .env(ENDPOINT_ENV, "12345 11111111111111111111111111111111")
            .output()
            .unwrap();
        assert!(
            output.status.success() && String::from_utf8_lossy(&output.stdout).contains("1 passed"),
            "child failed: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }

    let options = crate::cli::Options::try_parse_from(["pebrel", "--new-instance"]).unwrap();
    instance::initialize(options.new_instance);
    let directory = crate::display::nebula_data_dir();
    let discovery = directory.join("runtime.port");
    let saved = directory.join("session.json");
    let original_endpoint = b"12345 11111111111111111111111111111111 1\n";
    let original_session = b"existing resident workspace\n";
    fs::write(&discovery, original_endpoint).unwrap();
    fs::write(&saved, original_session).unwrap();
    // Holding the resident's actual lock catches accidental reuse of its scope.
    let _resident = crate::atomic_file::try_lifetime_lock(&discovery).unwrap().unwrap();

    assert!(instance::is_private());
    assert!(matches!(
        initial_startup(None, None, instance::is_private()),
        WorkspaceStartup::NewTerminal { cwd: None }
    ));
    let server = RuntimeServer::spawn_callback(|_| {}, RuntimeHub::new())
        .expect("an independent endpoint starts while the resident owns its lock");
    let mut env = HashMap::new();
    apply_child_endpoint(&mut env);
    let mut endpoint = env[ENDPOINT_ENV].split_whitespace();
    let port = endpoint.next().unwrap().parse::<u16>().unwrap();
    let token = endpoint.next().unwrap();
    assert_ne!(token, "11111111111111111111111111111111");
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    writeln!(stream, "PING {token}").unwrap();
    let mut reply = String::new();
    BufReader::new(stream).read_line(&mut reply).unwrap();
    assert_eq!(reply.trim(), "OK");

    let session = crate::session::Session::new(
        0,
        vec![crate::session::TabSession::single("project".into(), None, None)],
    );
    let mut persistence = SessionPersistence::default();
    for reason in
        [SaveReason::Checkpoint, SaveReason::TabsClosed, SaveReason::WindowClose, SaveReason::Quit]
    {
        persistence.save(Some(session.clone()), reason).unwrap();
        assert_eq!(fs::read(&saved).unwrap(), original_session);
    }
    drop(server);
    assert_eq!(fs::read(&discovery).unwrap(), original_endpoint);
    assert_eq!(fs::read(&saved).unwrap(), original_session);
}
