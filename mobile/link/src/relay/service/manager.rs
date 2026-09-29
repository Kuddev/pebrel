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
                return systemd_version().map(|_| ());
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

    fn definition(&self) -> io::Result<&'static str> {
        if self.0 == ServiceManager::OpenRc {
            Ok(openrc_script())
        } else {
            systemd_definition(systemd_version()?)
        }
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

fn systemd_version() -> io::Result<u32> {
    let output = process::run("/usr/bin/systemctl", &["--version"], true)?;
    let version = std::str::from_utf8(&output)
        .ok()
        .and_then(|text| text.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or_else(|| io::Error::other("systemd_239_required"))?;
    systemd_definition(version)?;
    Ok(version)
}

fn systemd_definition(version: u32) -> io::Result<&'static str> {
    match version {
        247.. => Ok(super::unit()),
        239..=246 => Ok(legacy_systemd_unit()),
        _ => Err(io::Error::other("systemd_239_required")),
    }
}

/// Read protected credentials and bind before permanently dropping to nobody.
/// This is the same single-threaded privilege boundary already used by OpenRC;
/// systemd 239 has no LoadCredential/%d support. No accounts are created.
pub(super) fn legacy_systemd_unit() -> &'static str {
    "[Unit]\nDescription=Pebrel encrypted mobile relay\nAfter=network-online.target\nWants=network-online.target\n\n\
[Service]\nType=simple\nUMask=0077\n\
ExecStart=/opt/pebrel-relay/pebrel-relay serve-unprivileged --config /etc/pebrel-relay/relay.json\n\
Restart=on-failure\nRestartSec=3\nTimeoutStopSec=10\n\
NoNewPrivileges=yes\nProtectSystem=strict\nProtectHome=yes\nPrivateTmp=yes\nPrivateDevices=yes\n\
ProtectKernelTunables=yes\nProtectKernelModules=yes\nProtectControlGroups=yes\nRestrictSUIDSGID=yes\n\
CapabilityBoundingSet=CAP_NET_BIND_SERVICE CAP_SETUID CAP_SETGID\n\
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX\nLimitNOFILE=1024\nTasksMax=64\n\n\
[Install]\nWantedBy=multi-user.target\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_matches_available_systemd_features() {
        assert!(systemd_definition(238).is_err());
        for version in [239, 245, 246] {
            let unit = systemd_definition(version).unwrap();
            assert!(unit.contains("serve-unprivileged"));
            assert!(unit.contains("CAP_SETUID CAP_SETGID"));
            assert!(!unit.contains("LoadCredential"));
            assert!(!unit.contains("%d/"));
        }
        assert!(systemd_definition(247).unwrap().contains("LoadCredential"));
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
