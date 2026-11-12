//! Learns the system behind an automatic host icon on an authenticated session.

use std::convert::Infallible;
use std::path::Path;
use std::time::Duration;

use super::{SharedSession, SshEventHost, exec};

const BUDGET: Duration = Duration::from_secs(6);
const SCRIPT: &[u8] = b"uname -s 2>/dev/null || exit 1\nprintf '\\036\\n'\nif [ -r /etc/os-release ]; then cat /etc/os-release; elif [ -r /usr/lib/os-release ]; then cat /usr/lib/os-release; fi\nexit 0\n";

pub(super) async fn learn_while_connected(
    profiles: &Path,
    destination: &str,
    session: &SharedSession,
    automatic: bool,
    host: &impl SshEventHost,
) -> Infallible {
    if automatic && let Some(icon) = probe(session).await {
        let profiles = profiles.to_owned();
        let destination = destination.to_owned();
        let chosen = tokio::task::spawn_blocking(move || {
            if let Err(error) =
                crate::ssh_profiles::detected_os::remember(&profiles, &destination, icon)
            {
                log::debug!("Unable to save the detected SSH host icon: {error}");
            }
            // A choice made while the probe was running takes priority as well.
            crate::ssh_profiles::SshProfiles::load(&profiles).ok().and_then(|profiles| {
                profiles.for_destination(&destination).chosen_icon().map(str::to_owned)
            })
        })
        .await
        .ok()
        .flatten();
        host.ssh_host_icon(Some(chosen.unwrap_or_else(|| icon.to_owned())));
    }
    // Finishing the optional probe must not finish the interactive shell.
    std::future::pending().await
}

async fn probe(session: &SharedSession) -> Option<&'static str> {
    let reply = tokio::time::timeout(BUDGET, async {
        let channel = session.channel_open_session().await?;
        exec::capture_private_input(channel, "exec sh", SCRIPT, 8192).await
    })
    .await;
    let Ok(Ok(raw)) = reply else { return None };
    let text = String::from_utf8_lossy(&raw);
    let (uname, os_release) = text.split_once('\u{1e}')?;
    crate::ssh_profiles::detected_os::icon_for(uname, os_release)
}
