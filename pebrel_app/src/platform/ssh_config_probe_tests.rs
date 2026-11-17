//! Native macOS OpenSSH config-only identity and capability with owned inputs.
//! No personal SSH configuration, identity files, agent or connection is used.

use crate::ssh_session::{SshDestination, parse_resolved_config, ssh_config_probe_args};
use std::io::Write as _;

#[test]
fn jumpserver_openssh_probes_preserve_identity_and_report_native_unicode_policy() {
    let version = std::process::Command::new("/usr/bin/ssh").arg("-V").output().unwrap();
    assert!(version.status.success());
    let version = String::from_utf8_lossy(&version.stderr);
    let directory = tempfile::tempdir().unwrap();
    let include = directory.path().join("included-config");
    let config = directory.path().join("config");
    std::fs::write(
        &include,
        format!(
            "Match user jumpserverUser@root,运维@root\n HostName 127.0.0.1\n ControlPath \"{}/control-%r\"\n RemoteCommand printf_%r\nHost gateway\n HostName 127.0.0.2\n",
            directory.path().display(),
        ),
    )
    .unwrap();
    std::fs::write(&config, format!("Include \"{}\"\n", include.display())).unwrap();
    for (spec, user, host, config) in [
        (
            "jumpserverUser@root@127.0.0.1:2222",
            "jumpserverUser@root",
            "127.0.0.1",
            std::path::Path::new("/dev/null"),
        ),
        (
            "ssh://jumpserverUser@root@[::1]:2222",
            "jumpserverUser@root",
            "::1",
            std::path::Path::new("/dev/null"),
        ),
        (
            "literal%40@root@127.0.0.1:2222",
            "literal%40@root",
            "127.0.0.1",
            std::path::Path::new("/dev/null"),
        ),
        ("jumpserverUser@root@gateway:2222", "jumpserverUser@root", "127.0.0.1", config.as_path()),
        ("ssh://运维@root@[::1]:2222", "运维@root", "::1", std::path::Path::new("/dev/null")),
        ("运维@root@gateway:2222", "运维@root", "127.0.0.1", config.as_path()),
    ] {
        let args = ssh_config_probe_args(spec);
        let raw = SshDestination::parse(spec).unwrap();
        assert_eq!(raw.user, user);
        assert_eq!(raw.original, spec);
        assert_eq!(raw.port, 2222);
        assert!(!raw.has_identity_files());
        if !user.is_ascii() {
            let literal_user = format!("User={user}");
            assert!(args.windows(2).any(|pair| pair[0] == "-o" && pair[1] == literal_user));
        }
        // -G never connects. -F selects only owned input, with all identity
        // and known-host defaults disabled even if this test starts normally.
        let output = std::process::Command::new("/usr/bin/ssh")
            .args(["-F"])
            .arg(config)
            .args([
                "-o",
                "IdentityFile=none",
                "-o",
                "GlobalKnownHostsFile=/dev/null",
                "-o",
                "UserKnownHostsFile=/dev/null",
                "-G",
            ])
            .args(&args)
            .env_remove("SSH_AUTH_SOCK")
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        if !output.status.success() {
            // Some native clients classify a UTF-8 continuation byte as a
            // control character. This is a checked policy outcome, not native
            // Unicode acceptance: no rewritten user or fallback may stand in.
            assert!(!user.is_ascii(), "ASCII probe failed: {spec}: {output:?}");
            assert_eq!(output.status.code(), Some(255));
            assert!(output.stdout.is_empty());
            let stderr = String::from_utf8_lossy(&output.stderr);
            let errors = stderr
                .lines()
                .filter(|line| {
                    *line
                        != "Pseudo-terminal will not be allocated because stdin is not a terminal."
                })
                .collect::<Vec<_>>();
            assert_eq!(errors, ["remote username contains invalid characters"], "{spec}");
            assert!(
                parse_resolved_config(spec, &String::from_utf8_lossy(&output.stdout)).is_none()
            );
            assert_eq!(raw.user, user);
            assert_eq!(raw.original, spec);
            assert_eq!(
                crate::ssh_credentials::credential_target(&raw.original),
                format!("Pebrel/SSH/{spec}")
            );
            report_native_outcome(&version, spec, "native_unicode_policy_rejection");
            continue;
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let parsed = parse_resolved_config(spec, &text).unwrap();
        assert_eq!((parsed.user.as_str(), parsed.host.as_str(), parsed.port), (user, host, 2222));
        assert_eq!(parsed.original, spec);
        assert!(!parsed.has_identity_files());
        if host == "127.0.0.1" && config != std::path::Path::new("/dev/null") {
            let control = directory.path().join(format!("control-{user}"));
            assert!(text.lines().any(|line| line == format!("controlpath {}", control.display())));
            assert!(text.lines().any(|line| line == format!("remotecommand printf_{user}")));
        }
        report_native_outcome(&version, spec, "native_identity_preserved");
    }
    for (spec, selected_config, host) in [
        ("jumpserverUser@root@127.0.0.1:2222", std::path::Path::new("/dev/null"), "127.0.0.1"),
        ("ssh://jumpserverUser@root@[::1]:2222", std::path::Path::new("/dev/null"), "::1"),
        ("jumpserverUser@root@gateway:2222", config.as_path(), "127.0.0.2"),
        ("运维@root@gateway:2222", config.as_path(), "127.0.0.2"),
    ] {
        for user_option in [["-l", "cli@account"], ["-o", "User=cli@account"]] {
            let output = std::process::Command::new("/usr/bin/ssh")
                .arg("-F")
                .arg(selected_config)
                .args([
                    "-o",
                    "IdentityFile=none",
                    "-o",
                    "GlobalKnownHostsFile=/dev/null",
                    "-o",
                    "UserKnownHostsFile=/dev/null",
                ])
                .args(user_option)
                .args(["-p", "2022", "-G"])
                .args(ssh_config_probe_args(spec))
                .env_remove("SSH_AUTH_SOCK")
                .stdin(std::process::Stdio::null())
                .output()
                .unwrap();
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            let parsed =
                parse_resolved_config(spec, &String::from_utf8_lossy(&output.stdout)).unwrap();
            assert_eq!(
                (parsed.user.as_str(), parsed.host.as_str(), parsed.port),
                ("cli@account", host, 2022)
            );
            assert_eq!(parsed.original, spec);
            assert!(!parsed.has_identity_files());
            report_native_outcome(&version, spec, "native_cli_override_preserved");
        }
    }
}

fn report_native_outcome(version: &str, spec: &str, outcome: &str) {
    let _ =
        std::io::stderr().lock().write_fmt(format_args!("{}: {spec}: {outcome}\n", version.trim()));
}
