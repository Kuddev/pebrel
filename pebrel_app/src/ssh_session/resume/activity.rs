//! Bounded, in-memory observations of this SSH client's active remote pane.
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::json;

use super::{PtyIdentity, ResumeTarget, SharedSession, exec};

#[derive(Clone, Debug, Deserialize)]
pub struct TmuxActivity {
    pub(crate) context: String,
    pub(crate) command: String,
    pub(crate) title: String,
    argv: Vec<String>,
    pub(crate) screen: String,
    #[serde(skip, default = "Instant::now")]
    pub(crate) observed_at: Instant,
}

impl TmuxActivity {
    pub(crate) fn current(&self) -> bool {
        self.observed_at.elapsed() < Duration::from_secs(8)
    }

    pub(crate) fn program(&self) -> Option<String> {
        use crate::ai_agents::AgentKind;
        // Use the shared executable/launcher rules; observing a process does
        // not claim a prompt submission, a completed turn or a Hook identity.
        let command = self.argv.iter().map(|arg| super::quote(arg)).collect::<Vec<_>>().join(" ");
        let agent = AgentKind::parse(&self.command).or_else(|| AgentKind::parse_command(&command));
        agent.map(|agent| agent.slug().to_owned()).or_else(|| {
            (!self.command.is_empty() && !self.command.chars().any(char::is_control))
                .then(|| self.command.clone())
        })
    }
}

pub(super) async fn observe(
    session: &SharedSession,
    identity: &PtyIdentity,
    target: &ResumeTarget,
    helper: &str,
    token: &str,
) -> Option<TmuxActivity> {
    tokio::time::timeout(super::BUDGET, async {
        let input = serde_json::to_vec(&json!({
            "token": token,
            "tty": identity.tty,
            "pid": identity.pid,
            "executable": target.endpoint.executable,
            "socket": target.endpoint.socket,
            "session": target.session,
            "identity": target.identity,
            "window": target.window,
            "pane": target.pane,
        }))
        .ok()?;
        let channel = session.channel_open_session().await.ok()?;
        let output = exec::capture_private_input(channel, helper, &input, 256 * 1024).await.ok()?;
        let output = std::str::from_utf8(&output).ok()?;
        let body = output.lines().find_map(|line| line.strip_prefix("PEBREL_TMUX="))?;
        let mut activity: TmuxActivity = serde_json::from_str(body).ok()?;
        if activity.context.len() != 64
            || !activity.context.bytes().all(|byte| byte.is_ascii_hexdigit())
            || activity.command.len() > 1024
            || activity.argv.len() > 3
            || activity.argv.iter().any(|arg| arg.len() > 16 * 1024)
            || activity.title.len() > 16 * 1024
            || activity.screen.len() > 128 * 1024
        {
            return None;
        }
        activity.observed_at = Instant::now();
        Some(activity)
    })
    .await
    .ok()
    .flatten()
}
