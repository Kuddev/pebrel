//! Init-system adapters. Selection is based on the running service manager,
//! not merely on a distribution label or an installed systemctl executable.
use super::{ServiceControl, ServiceStage, process};
use serde::{Deserialize, Serialize};
use std::{io, path::Path};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceManager {
    #[default]
    Systemd,
    OpenRc,
}

pub struct SystemService(ServiceManager);

impl SystemService {
    pub fn detect() -> io::Result<Self> {
        if Path::new("/run/systemd/system").is_dir() {
            return Ok(Self(ServiceManager::Systemd));
        }
        if Path::new("/sbin/openrc-run").is_file()
            && Path::new("/sbin/rc-service").is_file()
            && Path::new("/sbin/rc-update").is_file()
            && Path::new("/run/openrc").is_dir()
        {
            return Ok(Self(ServiceManager::OpenRc));
        }
        Err(io::Error::other("supported_init_required"))
    }
}

impl ServiceControl for SystemService {
    fn manager(&self) -> ServiceManager {
        self.0
    }

    fn run(&self, args: &[&str]) -> io::Result<()> {
        if self.0 == ServiceManager::Systemd {
            if args == ["--version"] {
                let output = process::run("/usr/bin/systemctl", args, true)?;
                let version = std::str::from_utf8(&output)
                    .ok()
                    .and_then(|text| text.split_whitespace().nth(1))
                    .and_then(|value| value.parse::<u32>().ok())
                    .unwrap_or(0);
                return if version >= 247 {
                    Ok(())
                } else {
                    Err(io::Error::other("systemd_247_required"))
                };
            }
            return process::run("/usr/bin/systemctl", args, false).map(|_| ());
        }
        match args {
            ["--version"] => {
                if !Path::new("/sbin/supervise-daemon").is_file() {
                    return Err(io::Error::other("openrc_supervisor_required"));
                }
                Ok(())
            },
            ["daemon-reload"] => Ok(()),
            ["enable", "--now", _] => {
                process::run("/sbin/rc-update", &["add", "pebrel-relay", "default"], false)?;
                process::run("/sbin/rc-service", &["pebrel-relay", "start"], false).map(|_| ())
            },
            ["disable", "--now", _] => {
                // A stopped/failed service is a valid uninstall target.
                if process::run("/sbin/rc-service", &["pebrel-relay", "status"], false).is_ok() {
                    process::run("/sbin/rc-service", &["pebrel-relay", "stop"], false)?;
                }
                process::run("/sbin/rc-update", &["del", "pebrel-relay", "default"], false)
                    .map(|_| ())
            },
            ["is-active", "--quiet", _] => {
                process::run("/sbin/rc-service", &["pebrel-relay", "status"], false).map(|_| ())
            },
            [action @ ("start" | "stop"), _] => {
                process::run("/sbin/rc-service", &["pebrel-relay", action], false).map(|_| ())
            },
            _ => Err(io::Error::other("invalid_service_operation")),
        }
    }

    fn ready(&self) -> io::Result<()> {
        process::run(
            "/opt/pebrel-relay/pebrel-relay",
            &["probe", "--config", "/etc/pebrel-relay/relay.json"],
            false,
        )
        .map(|_| ())
    }

    fn progress(&self, stage: ServiceStage) {
        use io::Write;
        let _ = writeln!(
            io::stdout().lock(),
            "{}",
            serde_json::json!({"event":"progress", "stage":stage})
        );
    }
}

pub(super) fn openrc_script() -> &'static str {
    "#!/sbin/openrc-run\n\
description=\"Pebrel encrypted mobile relay\"\n\
command=/opt/pebrel-relay/pebrel-relay\n\
command_args=\"serve-unprivileged --config /etc/pebrel-relay/relay.json\"\n\
supervisor=supervise-daemon\n\
respawn_delay=3\nrespawn_max=5\nrespawn_period=60\n\
retry=TERM/10/KILL/5\n\
rc_ulimit=\"-n 1024\"\n\
depend() { need net; }\n"
}
