//! Connection destinations follow the pane's captured host or WSL execution scope.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use pebrel_completions::semantic::{Context, Source};

use crate::display::SuggestEnv;
use crate::runtime_exec::PaneExecContext;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Scope {
    Host,
    Wsl { distro: String, user: Option<String> },
}

impl Scope {
    pub(super) fn capture(env: &SuggestEnv, execution: Option<&PaneExecContext>) -> Option<Self> {
        match env {
            SuggestEnv::Local if execution.is_none_or(|e| e.wsl_distribution().is_none()) => {
                Some(Self::Host)
            },
            SuggestEnv::Wsl { distro } if !distro.is_empty() => {
                // 目录标题不足以证明来宾用户；只复用当前 pane 实际启动时捕获的身份。
                let execution = execution?.for_wsl_distribution(distro)?;
                Some(Self::Wsl {
                    distro: distro.clone(),
                    user: execution.wsl_user().map(str::to_owned),
                })
            },
            _ => None,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Key {
    Ssh { config: PathBuf, home: PathBuf },
    GuestSsh { scope: Scope, config: Option<String>, expand_home: bool, cwd: String },
    Wsl,
}

#[derive(Debug)]
struct Snapshot {
    key: Key,
    fetched: Instant,
    names: Arc<[String]>,
}

#[derive(Debug, Default)]
pub(super) struct Cache(Mutex<(u64, Option<Snapshot>)>);

impl Cache {
    fn key(cwd: &str, context: &Context, scope: &Scope) -> Option<Key> {
        match context.source {
            Source::WslDistributions if *scope == Scope::Host => Some(Key::Wsl),
            Source::SshHosts { .. } if context.ssh_config.as_deref() == Some("none") => None,
            Source::SshHosts { .. } if matches!(scope, Scope::Wsl { .. }) => Some(Key::GuestSsh {
                scope: scope.clone(),
                config: context.ssh_config.clone(),
                expand_home: context.ssh_config_expands_home,
                cwd: cwd.to_owned(),
            }),
            Source::SshHosts { .. } => {
                let home = crate::platform::dirs::home_dir()?;
                let config = match context.ssh_config.as_deref() {
                    Some(path) => {
                        if context.ssh_config_expands_home {
                            return path
                                .strip_prefix("~/")
                                .map(|suffix| Key::Ssh { config: home.join(suffix), home });
                        }
                        if !Path::new(path).is_absolute() && !Path::new(cwd).is_absolute() {
                            return None;
                        }
                        Path::new(cwd).join(path)
                    },
                    None => home.join(".ssh/config"),
                };
                Some(Key::Ssh { config, home })
            },
            _ => None,
        }
    }

    pub(super) fn invalidate(&self) {
        let mut state = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        state.0 = state.0.wrapping_add(1);
        state.1 = None;
    }

    pub(super) fn complete(
        &self,
        cwd: &str,
        context: &Context,
        scope: &Scope,
        cancelled: &dyn Fn() -> bool,
    ) -> Option<Vec<pebrel_completions::Suggestion>> {
        if cancelled() {
            return Some(Vec::new());
        }
        let Some(key) = Self::key(cwd, context, scope) else { return Some(Vec::new()) };
        let (generation, cached) = {
            let state = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            (
                state.0,
                state
                    .1
                    .as_ref()
                    .filter(|snapshot| {
                        snapshot.key == key && snapshot.fetched.elapsed() < Duration::from_secs(2)
                    })
                    .map(|snapshot| snapshot.names.clone()),
            )
        };
        let names = if let Some(names) = cached {
            names
        } else {
            let names: Arc<[String]> = match &key {
                Key::Ssh { config, home } => {
                    crate::ssh::hosts::discover(config, &home.join(".ssh"), home, cancelled)
                },
                Key::GuestSsh { scope: Scope::Wsl { distro, user }, config, expand_home, cwd } => {
                    crate::ssh::hosts::discover_wsl(
                        crate::ssh::hosts::WslConfig {
                            distro,
                            user: user.as_deref(),
                            config: config.as_deref(),
                            expand_home: *expand_home,
                            cwd,
                        },
                        cancelled,
                    )
                    .ok()?
                    .hosts
                },
                Key::GuestSsh { .. } => return None,
                Key::Wsl => crate::platform::shell::registered_wsl_distros(cancelled),
            }
            .into();
            let mut state = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            // 取消/提交使代次失效；旧后台请求不得重新填充当前会话的缓存。
            if !cancelled() && generation == state.0 {
                state.1 = Some(Snapshot { key, fetched: Instant::now(), names: names.clone() });
            }
            names
        };
        Some(if cancelled() {
            Vec::new()
        } else {
            context.candidates(names.iter().map(String::as_str))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pebrel_completions::command_context::ShellSyntax;

    #[test]
    fn guest_connection_scope_keeps_user_config_and_host_boundaries() {
        let execution = |user: &str| {
            PaneExecContext::from_pty_options(&nebula_terminal::tty::Options {
                shell: Some(nebula_terminal::tty::Shell::new(
                    "wsl.exe".into(),
                    vec!["-d".into(), "Debian".into(), "-u".into(), user.into()],
                )),
                ..Default::default()
            })
        };
        let env = SuggestEnv::Wsl { distro: "Debian".into() };
        let alice = Scope::capture(&env, Some(&execution("alice"))).unwrap();
        let bob = Scope::capture(&env, Some(&execution("bob"))).unwrap();
        assert!(Scope::capture(&env, None).is_none());
        assert!(Scope::capture(&SuggestEnv::Local, Some(&execution("alice"))).is_none());
        assert!(
            Scope::capture(
                &SuggestEnv::Wsl { distro: "Ubuntu".into() },
                Some(&execution("alice")),
            )
            .is_none()
        );
        let context = |line: &str| Context::parse(line, line.len(), ShellSyntax::Posix).unwrap();
        let ssh = context("ssh -F config host");
        assert_ne!(Cache::key("/project", &ssh, &alice), Cache::key("/project", &ssh, &bob));
        assert_ne!(Cache::key("/project", &ssh, &alice), Cache::key("/other", &ssh, &alice));
        assert!(Cache::key("/project", &context("ssh -F none host"), &alice).is_none());
        assert!(Cache::key("/project", &context("wsl -d "), &alice).is_none());
        assert_eq!(Cache::key("/project", &context("wsl -d "), &Scope::Host), Some(Key::Wsl));
    }
}
