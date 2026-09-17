use std::{io, path::Path};

use pebrel_mobile_link::relay::{self, setup};
use tokio_util::sync::CancellationToken;

fn main() {
    if let Err(error) = run() {
        // Never print paths, config contents, tokens or network peer messages.
        eprintln!("{}", serde_json::json!({"ok": false, "error": error_code(&error)}));
        std::process::exit(1);
    }
}

/// Keep recoverable failures distinguishable without exporting arbitrary OS,
/// TLS, SSH or peer messages. The client localizes these fixed identifiers.
fn error_code(error: &io::Error) -> &'static str {
    const CODES: &[&str] = &[
        "systemd_247_required",
        "linux_systemd_required",
        "root_required",
        "installation_conflict",
        "configuration_directory_not_empty",
        "symlink_installation_path",
        "invalid_ownership_manifest",
        "managed_file_changed",
        "binary_integrity_failed",
        "explicit_update_required",
        "service_not_ready",
        "service_command_failed",
        "service_command_timeout",
        "service_command_output_limit",
        "invalid_initialization_input",
        "invalid_port",
        "invalid_listener",
        "invalid_arguments",
        "health_timeout",
        "relay_not_ready",
    ];
    let message = error.to_string();
    if let Some(code) = CODES.iter().copied().find(|code| *code == message) {
        return code;
    }
    match error.kind() {
        io::ErrorKind::PermissionDenied => "permission_denied",
        io::ErrorKind::AlreadyExists => "installation_conflict",
        io::ErrorKind::NotFound => "file_or_service_not_found",
        io::ErrorKind::TimedOut => "operation_timeout",
        io::ErrorKind::AddrInUse => "port_in_use",
        _ => "relay_operation_failed",
    }
}

fn run() -> io::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["service-status"] => {
            println!(
                "{}",
                serde_json::to_string(&relay::service::status(&relay::service::Systemd))
                    .map_err(io::Error::other)?
            );
        },
        [command @ ("service-start" | "service-stop")] => {
            require_linux_root()?;
            relay::service::change_running(*command == "service-start", &relay::service::Systemd)?;
            println!("{}", serde_json::json!({"ok": true, "operation": command}));
        },
        [
            "service-install",
            "--source",
            source,
            "--sha256",
            hash,
            "--address",
            address,
            "--port",
            port,
        ] => {
            require_linux_root()?;
            let listen =
                format!("0.0.0.0:{port}").parse().map_err(|_| io::Error::other("invalid_port"))?;
            relay::service::install(
                Path::new(source),
                hash,
                address,
                listen,
                &relay::service::Systemd,
            )?;
            println!("{}", serde_json::json!({"ok": true, "stage": "ready"}));
        },
        ["service-uninstall"] | ["service-uninstall", "--purge"] => {
            require_linux_root()?;
            let purge = args.len() == 2;
            relay::service::uninstall(purge, &relay::service::Systemd)?;
            println!(
                "{}",
                serde_json::json!({"ok": true, "stage": "uninstalled", "configuration_retained": !purge})
            );
        },
        ["init", "--directory", directory, "--address", address, "--listen", listen] => {
            setup::initialize(
                Path::new(directory),
                address,
                listen.parse().map_err(|_| io::Error::other("invalid_listener"))?,
            )?;
            println!("{}", serde_json::json!({"ok": true, "stage": "initialized"}));
        },
        ["export-access", "--directory", directory] => {
            let bytes = setup::read_access(Path::new(directory))?;
            use io::Write;
            io::stdout().lock().write_all(&bytes)?;
        },
        [command @ ("serve" | "probe" | "check-config"), "--config", path] => {
            let config = setup::read_config(Path::new(path))?;
            if *command == "check-config" {
                println!("{}", serde_json::json!({"ok": true, "stage": "validated"}));
                return Ok(());
            }
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()?;
            if *command == "probe" {
                runtime.block_on(setup::probe(&config))?;
                println!("{}", serde_json::json!({"ok": true, "ready": true}));
            } else {
                runtime.block_on(async {
                    let shutdown = CancellationToken::new();
                    let signal = shutdown.clone();
                    tokio::spawn(async move {
                        #[cfg(unix)] {
                            if let Ok(mut terminate) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                                tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
                            } else { let _ = tokio::signal::ctrl_c().await; }
                        }
                        #[cfg(not(unix))] { let _ = tokio::signal::ctrl_c().await; }
                        signal.cancel();
                    });
                    relay::serve(config, shutdown).await
                })?;
            }
        },
        ["--version"] => println!("pebrel-relay {} protocol=2", env!("CARGO_PKG_VERSION")),
        _ => {
            eprintln!(
                "pebrel-relay init --directory PATH --address HOST --listen IP:PORT\npebrel-relay serve|probe|check-config --config PATH\npebrel-relay export-access --directory PATH"
            );
            return Err(io::Error::other("invalid_arguments"));
        },
    }
    Ok(())
}

fn require_linux_root() -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;
        if std::fs::metadata("/proc/self")?.uid() == 0 {
            return Ok(());
        }
        Err(io::Error::new(io::ErrorKind::PermissionDenied, "root_required"))
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(io::Error::other("linux_systemd_required"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_failures_are_actionable_but_untrusted_details_are_not_exported() {
        assert_eq!(error_code(&io::Error::other("service_not_ready")), "service_not_ready");
        assert_eq!(error_code(&io::Error::other("managed_file_changed")), "managed_file_changed");
        assert_eq!(
            error_code(&io::Error::other("service_not_ready: private host or token")),
            "relay_operation_failed",
        );
        assert_eq!(
            error_code(&io::Error::new(io::ErrorKind::PermissionDenied, "private path")),
            "permission_denied",
        );
    }
}
