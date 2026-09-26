//! SSH transport adapter for the shared hook installation policy.
use std::time::Duration;

use serde_json::json;

use super::{SessionError, SharedSession, exec, lifecycle};
use crate::ai_hook::remote::{self, Action, Snapshot};

const BUDGET: Duration = Duration::from_secs(6);
const PYTHON: &str = "python3 -";

pub(super) async fn prepare(session: &SharedSession, token: &str) -> Option<String> {
    if nebula_settings::RawSettings::load().bool_on("ai_hooks") == Some(false) {
        return None;
    }
    let result = tokio::time::timeout(BUDGET, async {
        let channel = session.channel_open_session().await?;
        let raw = exec::capture(
            channel,
            PYTHON,
            &remote::request(&json!({"action":"snapshot"})),
            BUDGET,
            "hook setup",
        )
        .await?;
        let snapshot: Snapshot = serde_json::from_value(remote::response(&raw)?)?;
        let Some(files) = snapshot.plan(Action::Automatic)? else { return Ok(None) };
        if !files.is_empty() {
            let channel = session.channel_open_session().await?;
            let raw = exec::capture(
                channel,
                PYTHON,
                &remote::request(&json!({"action":"apply", "files":files})),
                BUDGET,
                "hook setup",
            )
            .await?;
            applied(&raw)?;
        }
        Ok::<_, SessionError>(Some(snapshot.bootstrap(token)?))
    })
    .await;
    match result {
        Ok(Ok(command)) => command,
        Ok(Err(error)) => {
            log::debug!("SSH hook integration unavailable; opening ordinary shell: {error}");
            None
        },
        Err(_) => {
            log::debug!("SSH hook integration timed out; opening ordinary shell");
            None
        },
    }
}

fn applied(raw: &str) -> Result<(), SessionError> {
    if remote::response(raw)?.get("applied").and_then(serde_json::Value::as_bool) != Some(true) {
        return Err("remote integration did not confirm installation".into());
    }
    Ok(())
}

/// Explicit install/remove uses the same planner and authenticated connection
/// pool as automatic setup. A remote removal persists an opt-out on that host.
pub(crate) fn setup_cli(destination: &str, remove: bool) -> i32 {
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())
        .and_then(|runtime| {
            runtime
                .block_on(async {
                    let raw = super::exec_capture(
                        destination,
                        PYTHON,
                        &remote::request(&json!({"action":"snapshot"})),
                        BUDGET,
                    )
                    .await?;
                    let snapshot: Snapshot = serde_json::from_value(remote::response(&raw)?)?;
                    let action = if remove { Action::Remove } else { Action::Install };
                    let files = snapshot.plan(action)?.ok_or("remote integration disabled")?;
                    if !files.is_empty() {
                        let raw = super::exec_capture(
                            destination,
                            PYTHON,
                            &remote::request(&json!({"action":"apply", "files":files})),
                            BUDGET,
                        )
                        .await?;
                        applied(&raw)?;
                    }
                    Ok::<_, SessionError>(())
                })
                .map_err(|error| error.to_string())
        });
    match result {
        Ok(()) => {
            println!(
                "{}",
                if remove {
                    "SSH hook integration removed."
                } else {
                    "SSH hook integration installed. Reconnect the terminal. Codex may require review in /hooks."
                }
            );
            0
        },
        Err(error) => {
            eprintln!("SSH hook integration: {error}");
            1
        },
    }
}

pub(super) async fn start(
    channel: &mut lifecycle::ShellChannel,
    command: &str,
) -> Result<(), SessionError> {
    channel.exec(true, command).await?;
    lifecycle::wait_request_success(channel, "integrated shell").await
}
