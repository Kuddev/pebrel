//! Connection destinations are discovered on the completion worker, scoped to this host.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nebula_completions::semantic::{Context, Source};

#[derive(Debug, PartialEq, Eq)]
enum Key {
    Ssh { config: PathBuf, home: PathBuf },
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
    fn key(cwd: &str, context: &Context) -> Option<Key> {
        match context.source {
            Source::WslDistributions => Some(Key::Wsl),
            Source::SshHosts { .. } => {
                let home = crate::platform::dirs::home_dir()?;
                let config = match context.ssh_config.as_deref() {
                    Some("none") => return None,
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
        cancelled: &dyn Fn() -> bool,
    ) -> Vec<nebula_completions::Suggestion> {
        if cancelled() {
            return Vec::new();
        }
        let Some(key) = Self::key(cwd, context) else { return Vec::new() };
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
        let names = cached.unwrap_or_else(|| {
            let names: Arc<[String]> = match &key {
                Key::Ssh { config, home } => {
                    crate::ssh::hosts::discover(config, &home.join(".ssh"), home, cancelled)
                },
                Key::Wsl => crate::platform::shell::registered_wsl_distros(cancelled),
            }
            .into();
            let mut state = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            // 取消/提交使代次失效；旧后台请求不得重新填充当前会话的缓存。
            if !cancelled() && generation == state.0 {
                state.1 = Some(Snapshot { key, fetched: Instant::now(), names: names.clone() });
            }
            names
        });
        if cancelled() { Vec::new() } else { context.candidates(names.iter().map(String::as_str)) }
    }
}
