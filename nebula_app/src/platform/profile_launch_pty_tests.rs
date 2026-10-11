//! Actual Unix PTY validation of the shared local-profile launch adapter.

use super::*;

#[cfg(unix)]
#[test]
fn local_profile_legacy_default_options_reach_a_real_safe_pty_child() {
    if crate::platform::profile_test_isolation::run_exact_child(
        "profile_launch::legacy_tests::pty::local_profile_legacy_default_options_reach_a_real_safe_pty_child",
        None,
    ) {
        return;
    }
    use std::time::{Duration, Instant};
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("child.txt");
    let process_value = std::env::var_os("QA_DEFAULT_OVERLAY");
    let mut profile = profile("/bin/sh");
    profile.args = vec![
        "-c".into(),
        "printf '%s|%s\\n' \"$QA_DEFAULT_OVERLAY\" \"${QA_EMPTY+x}:$QA_EMPTY\" > \"$QA_OUTPUT\""
            .into(),
    ];
    profile.env.0.insert("QA_OUTPUT".into(), output.to_string_lossy().into_owned());
    let selected =
        selected_local_profile(&[profile], Some("profile:sh|same-stable-id"), false).unwrap();
    let mut options = nebula_terminal::tty::Options::default();
    options.shell = Some(selected.shell());
    options.working_directory = Some(directory.path().into());
    options.env.insert("QA_DEFAULT_OVERLAY".into(), "global".into());
    apply_local_profile_options(&mut options, &selected.env);
    let _pty = nebula_terminal::tty::new(
        &options,
        nebula_terminal::event::WindowSize {
            num_cols: 80,
            num_lines: 24,
            cell_width: 8,
            cell_height: 16,
        },
        0,
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if std::fs::read_to_string(&output).ok().as_deref() == Some("profile|x:\n") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "synthetic legacy PTY child did not receive its profile overlay"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(std::env::var_os("QA_DEFAULT_OVERLAY"), process_value);
}
