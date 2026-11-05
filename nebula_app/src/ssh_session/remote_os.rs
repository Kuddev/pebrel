//! Learns which system a saved host runs, so its automatic icon shows the real one.

use std::time::Duration;

use super::{SharedSession, exec};

const BUDGET: Duration = Duration::from_secs(6);
const SCRIPT: &[u8] = b"uname -s 2>/dev/null\necho '\x1e'\ncat /etc/os-release 2>/dev/null\n";

/// Runs once per new shell on the already authenticated connection. Failures only leave the icon as it was.
pub(super) fn learn(destination: String, session: SharedSession) {
    tokio::spawn(async move {
        let reply = tokio::time::timeout(BUDGET, async {
            let channel = session.channel_open_session().await?;
            exec::capture(channel, "exec sh", SCRIPT, BUDGET, "remote system").await
        })
        .await;
        let Ok(Ok(raw)) = reply else { return };
        let (uname, os_release) = raw.split_once('\u{1e}').unwrap_or((raw.as_str(), ""));
        let Some(icon) = crate::ssh_profiles::detected_os::icon_for(uname, os_release) else {
            return;
        };
        let profiles = crate::display::nebula_data_dir().join("ssh_profiles.json");
        let saved = tokio::task::spawn_blocking(move || {
            crate::ssh_profiles::detected_os::remember(&profiles, &destination, icon)
        })
        .await;
        if let Ok(Err(error)) = saved {
            log::debug!("remote system for the host icon was not saved: {error}");
        }
    });
}
