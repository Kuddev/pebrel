//! Completion history follows the active SSH/WSL context, not the tab's launch label.
//! Shell instance reports restore an enclosing context without confusing a remote
//! command's OSC 133;D with the end of the SSH connection.

use sha2::{Digest, Sha256};
use std::fmt::Write as _;

use crate::display::{NebulaPaneState, SuggestEnv};
use crate::nebula_history::{HistoryScope, NebulaHistory};

pub(crate) const SHELL_VAR: &str = "pebrel_shell";
pub(crate) const COMMAND_VAR: &str = "pebrel_command";
pub(crate) const ARGV_VAR: &str = "pebrel_connection";

#[derive(Clone, Debug)]
struct Frame {
    token: Option<String>,
    env: SuggestEnv,
    cwd: String,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct CompletionContext {
    frames: Vec<Frame>,
}

impl CompletionContext {
    fn submit(&mut self, env: &SuggestEnv, cwd: &str, line: &str) -> Option<SuggestEnv> {
        self.submit_argv(env, cwd, &crate::ssh::command_words(line)?)
    }

    fn submit_argv(&mut self, env: &SuggestEnv, cwd: &str, words: &[String]) -> Option<SuggestEnv> {
        let child = connection_argv_environment(env, words)?;
        if self.frames.is_empty() {
            self.frames.push(Frame { token: None, env: env.clone(), cwd: cwd.to_owned() });
        } else if let Some(frame) = self.frames.last_mut() {
            frame.cwd = cwd.to_owned();
        }
        self.frames.push(Frame { token: None, env: child.clone(), cwd: String::new() });
        Some(child)
    }

    fn prompt(&mut self, env: &SuggestEnv, cwd: &str, token: &str) -> Option<(SuggestEnv, String)> {
        if token.is_empty() || token.len() > 256 || token.chars().any(char::is_control) {
            return None;
        }
        if let Some(index) =
            self.frames.iter().position(|frame| frame.token.as_deref() == Some(token))
        {
            self.frames.truncate(index + 1);
            let frame = &self.frames[index];
            return Some((frame.env.clone(), frame.cwd.clone()));
        }
        let reported_env = match (env, token.strip_prefix("wsl|").and_then(|s| s.split_once('|'))) {
            (SuggestEnv::Wsl { distro }, Some((name, _)))
                if distro.is_empty() && !name.is_empty() =>
            {
                SuggestEnv::Wsl { distro: name.to_owned() }
            },
            _ => env.clone(),
        };
        if let Some(frame) = self.frames.last_mut()
            && frame.token.is_none()
        {
            frame.token = Some(token.to_owned());
            frame.env = reported_env.clone();
        } else {
            // A new shell on the current host retains that host's history.
            // Only an SSH/WSL context change changes the source of suggestions.
            self.frames.push(Frame {
                token: Some(token.to_owned()),
                env: reported_env.clone(),
                cwd: cwd.to_owned(),
            });
        }
        (reported_env != *env).then(|| (reported_env, cwd.to_owned()))
    }
}

impl NebulaPaneState {
    pub(crate) fn record_completion_command(&mut self, history: &mut NebulaHistory, line: &str) {
        history.record(&self.suggest_env.history_scope(), line, &self.cwd);
        self.completion_submitted(line);
    }

    pub(crate) fn completion_submitted(&mut self, line: &str) {
        if let Some(env) = self.completion_context.submit(&self.suggest_env, &self.cwd, line) {
            self.switch_completion_environment(env, String::new());
        }
    }

    pub(crate) fn completion_shell_report(&mut self, name: &str, value: &str) {
        if name == COMMAND_VAR || name == ARGV_VAR {
            let Some((token, line)) = value.split_once('\n') else { return };
            let Some(index) = self
                .completion_context
                .frames
                .iter()
                .position(|frame| frame.token.as_deref() == Some(token))
            else {
                return;
            };
            // PSReadLine reports the submitted text and its owning shell. This
            // also reconciles native recall/alias expansion with an Enter that
            // already changed the provisional completion context.
            self.completion_context.frames.truncate(index + 1);
            let frame = self.completion_context.frames[index].clone();
            if frame.env != self.suggest_env {
                self.switch_completion_environment(frame.env, frame.cwd);
            }
            if name == ARGV_VAR {
                let words: Vec<_> = line.split('\0').map(str::to_owned).collect();
                if let Some(env) =
                    self.completion_context.submit_argv(&self.suggest_env, &self.cwd, &words)
                {
                    self.switch_completion_environment(env, String::new());
                }
            } else {
                self.completion_submitted(line);
            }
        } else if name == SHELL_VAR {
            if let Some((env, cwd)) =
                self.completion_context.prompt(&self.suggest_env, &self.cwd, value)
            {
                if env != self.suggest_env {
                    self.switch_completion_environment(env, cwd);
                }
            }
        }
    }

    fn switch_completion_environment(&mut self, env: SuggestEnv, cwd: String) {
        self.suggest_env = env;
        self.cwd = cwd;
        self.pending_remote_dir = None;
        self.clear_completion_hints();
        self.completion_suppressed_line = None;
        self.screen_line.clear();
        self.line_buf.clear();
    }
}

pub(crate) fn initial_state(
    options: &nebula_terminal::tty::Options,
    cwd: String,
) -> NebulaPaneState {
    let mut state = NebulaPaneState::default();
    state.cwd = cwd;
    state.suggest_env = options
        .shell
        .as_ref()
        .map(|shell| launch_environment(shell.program(), shell.args()))
        .unwrap_or_default();
    state
}

#[cfg(test)]
fn connection_environment(parent: &SuggestEnv, line: &str) -> Option<SuggestEnv> {
    let words = crate::ssh::command_words(line)?;
    connection_argv_environment(parent, &words)
}

fn connection_argv_environment(parent: &SuggestEnv, words: &[String]) -> Option<SuggestEnv> {
    if crate::ssh::ssh_destination_words(words).is_some() {
        return Some(typed_environment(parent, words, false));
    }
    let program = words.first()?.rsplit(['/', '\\']).next()?;
    if crate::display::extract_program(program).as_deref() == Some("wsl") {
        return Some(typed_environment(parent, words, true));
    }
    None
}

fn typed_environment(parent: &SuggestEnv, words: &[String], wsl: bool) -> SuggestEnv {
    // A destination alias is resolved in its parent environment. Keep both
    // that ancestry and connection options in the history identity. The typed
    // SSH route cannot be substituted with the outer pane's SFTP connection.
    let mut hash = Sha256::new();
    hash.update(format!("{:?}", parent.history_scope()));
    for word in words {
        hash.update((word.len() as u64).to_le_bytes());
        hash.update(word.as_bytes());
    }
    let digest = hash.finalize();
    let mut identity = String::from("typed:");
    for byte in digest {
        let _ = write!(identity, "{byte:02x}");
    }
    let scope = if wsl { HistoryScope::Wsl(identity) } else { HistoryScope::Ssh(identity) };
    SuggestEnv::Shell { scope }
}

pub(crate) fn launch_environment(program: &str, args: &[String]) -> SuggestEnv {
    let program_name = program.rsplit(['/', '\\']).next().unwrap_or(program);
    match crate::display::extract_program(program_name).as_deref() {
        Some("wsl") => {
            let distro = crate::shell_detect::wsl_launch_distro(program, args)
                .map(str::to_owned)
                .or_else(crate::platform::shell::default_wsl_distro)
                .unwrap_or_default();
            SuggestEnv::Wsl { distro }
        },
        Some("ssh") => {
            let words: Vec<_> =
                std::iter::once(program.to_owned()).chain(args.iter().cloned()).collect();
            typed_environment(&SuggestEnv::Local, &words, false)
        },
        _ => SuggestEnv::Local,
    }
}

#[cfg(test)]
mod tests;
