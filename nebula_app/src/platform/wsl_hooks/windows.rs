//! WSL transport for the shared POSIX hook installer. No provider policy here.
//!
//! A guest has its own home and executable search path. Windows hook files and
//! Windows PIDs cannot stand in for either. Delivery uses the pane's existing
//! authenticated OSC protocol, with a fresh token for every PTY.

use std::collections::HashMap;
use std::io::{Read as _, Seek as _, Write as _};
use std::process::{Command, Stdio};
use std::sync::{OnceLock, mpsc};
use std::time::{Duration, Instant};

use nebula_terminal::tty;
use serde_json::json;

use crate::ai_hook::remote::{self, Action, Snapshot};

const BUDGET: Duration = Duration::from_secs(8);
const MAX_OUTPUT: u64 = 20 * 1024 * 1024;
const RETRY_AFTER: Duration = Duration::from_secs(300);

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct Target {
    distro: Option<String>,
    user: Option<String>,
}

impl Target {
    fn from_shell(shell: &tty::Shell) -> Option<Self> {
        (crate::display::extract_program(shell.program()).as_deref() == Some("wsl")).then(|| Self {
            distro: crate::shell_detect::wsl_launch_distro(shell.program(), shell.args())
                .map(str::to_owned),
            user: crate::shell_detect::wsl_launch_user(shell.program(), shell.args())
                .map(str::to_owned),
        })
    }

    fn command(&self) -> Command {
        let mut command = Command::new("wsl.exe");
        if let Some(distro) = &self.distro {
            command.args(["--distribution", distro]);
        }
        if let Some(user) = &self.user {
            command.args(["--user", user]);
        }
        // This is a separate setup process. The user's interactive shell and
        // its launch arguments are left to WSL's normal /etc/passwd selection.
        command.args(["--exec", "sh", "-lc", "exec python3 -"]);
        crate::platform::process::hidden_command(&mut command);
        command
    }

    fn exchange(&self, request: &[u8], deadline: Instant) -> Result<String, String> {
        let run = || -> std::io::Result<String> {
            let mut input = tempfile::tempfile()?;
            input.write_all(request)?;
            input.rewind()?;
            let mut output = tempfile::tempfile()?;
            let mut child = self
                .command()
                .stdin(input)
                .stdout(output.try_clone()?)
                .stderr(Stdio::null())
                .spawn()?;
            let status = loop {
                let poll = child.try_wait().and_then(|status| {
                    if status.is_none()
                        && (Instant::now() >= deadline || output.metadata()?.len() > MAX_OUTPUT)
                    {
                        return Err(std::io::Error::other("WSL hook setup exceeded its budget"));
                    }
                    Ok(status)
                });
                match poll {
                    Ok(Some(status)) => break status,
                    Ok(None) => {
                        std::thread::sleep(Duration::from_millis(10));
                    },
                    Err(error) => {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(error);
                    },
                }
            };
            if !status.success() || output.metadata()?.len() > MAX_OUTPUT {
                return Err(std::io::Error::other("WSL hook setup failed"));
            }
            output.rewind()?;
            let mut text = String::new();
            output.take(MAX_OUTPUT).read_to_string(&mut text)?;
            Ok(text)
        };
        run().map_err(|error| error.to_string())
    }

    fn install(&self, action: Action) -> Result<(), String> {
        let deadline = Instant::now() + BUDGET;
        let raw = self.exchange(&remote::request(&json!({"action":"snapshot"})), deadline)?;
        let snapshot: Snapshot = serde_json::from_value(remote::response(&raw)?)
            .map_err(|_| "invalid WSL integration snapshot")?;
        let Some(files) = snapshot.for_wsl().plan(action)? else { return Ok(()) };
        if files.is_empty() {
            return Ok(());
        }
        let raw =
            self.exchange(&remote::request(&json!({"action":"apply", "files":files})), deadline)?;
        if remote::response(&raw)?.get("applied").and_then(serde_json::Value::as_bool) != Some(true)
        {
            return Err("WSL integration did not confirm installation".into());
        }
        log::info!("ai_hook: WSL hook files updated; Codex may require review in /hooks");
        Ok(())
    }
}

/// Called during local PTY preparation. Guest I/O belongs to one setup worker;
/// the caller checks the local preference and prepares the pane environment.
pub(crate) fn prepare(options: &mut tty::Options) {
    let Some(target) = options.shell.as_ref().and_then(Target::from_shell) else { return };
    if nebula_settings::RawSettings::load().bool_on("ai_hooks") == Some(false) {
        return;
    }
    let token = match remote::new_token() {
        Ok(token) => token,
        Err(error) => {
            log::warn!("ai_hook: WSL channel unavailable: {error}");
            return;
        },
    };
    options.env.insert(remote::TOKEN_ENV.into(), token);
    let wslenv = options.env.entry("WSLENV".into()).or_default();
    if !wslenv.split(':').any(|entry| entry.split('/').next() == Some(remote::TOKEN_ENV)) {
        if !wslenv.is_empty() {
            wslenv.push(':');
        }
        wslenv.push_str(remote::TOKEN_ENV);
    }
    match worker().and_then(|tx| tx.try_send(target).ok()) {
        Some(()) => {},
        None => log::warn!("ai_hook: WSL setup queue unavailable; retry with setup-ai --wsl"),
    }
}

fn worker() -> Option<&'static mpsc::SyncSender<Target>> {
    static WORKER: OnceLock<Option<mpsc::SyncSender<Target>>> = OnceLock::new();
    WORKER
        .get_or_init(|| {
            let (tx, rx) = mpsc::sync_channel::<Target>(16);
            std::thread::Builder::new()
                .name("pebrel-wsl-hooks".into())
                .spawn(move || {
                    let mut attempted: HashMap<Target, Instant> = HashMap::new();
                    while let Ok(target) = rx.recv() {
                        if nebula_settings::RawSettings::load().bool_on("ai_hooks") == Some(false) {
                            continue;
                        }
                        attempted.retain(|_, at| at.elapsed() < RETRY_AFTER);
                        if attempted.contains_key(&target) {
                            continue;
                        }
                        // Cache has a fixed bound even if custom launchers invent users.
                        if attempted.len() >= 32 {
                            attempted.clear();
                        }
                        attempted.insert(target.clone(), Instant::now());
                        if let Err(error) = target.install(Action::Automatic) {
                            log::warn!("ai_hook: WSL integration unavailable: {error}");
                        }
                    }
                })
                .ok()
                .map(|_| tx)
        })
        .as_ref()
}

pub(crate) fn setup_cli(distro: &str, user: Option<&str>, remove: bool) -> i32 {
    let target = Target { distro: Some(distro.into()), user: user.map(str::to_owned) };
    match target.install(if remove { Action::Remove } else { Action::Install }) {
        Ok(()) => {
            println!(
                "{}",
                if remove {
                    "WSL hook integration removed."
                } else {
                    "WSL hook integration installed. Open a new Pebrel WSL terminal. Codex may require review in /hooks."
                }
            );
            0
        },
        Err(error) => {
            eprintln!("WSL hook integration: {error}");
            1
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_targets_the_same_distribution_and_user_without_changing_the_shell() {
        let shell = tty::Shell::new(
            "wsl.exe".into(),
            vec![
                "-d".into(),
                "Debian custom".into(),
                "-u".into(),
                "alice".into(),
                "--cd".into(),
                "/work/my project".into(),
            ],
        );
        let target = Target::from_shell(&shell).unwrap();
        let command = target.command();
        let args: Vec<_> =
            command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
        assert_eq!(
            args,
            [
                "--distribution",
                "Debian custom",
                "--user",
                "alice",
                "--exec",
                "sh",
                "-lc",
                "exec python3 -"
            ]
        );
        assert_eq!(shell.args()[4..], ["--cd", "/work/my project"]);
        assert!(Target::from_shell(&tty::Shell::new("pwsh.exe".into(), vec![])).is_none());
    }
}
