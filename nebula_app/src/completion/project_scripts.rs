//! Read project script names without invoking a package manager or project code.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use pebrel_completions::semantic::Context;

#[derive(Debug)]
struct Snapshot {
    key: (PathBuf, Vec<String>),
    fetched: Instant,
    names: Arc<[String]>,
}

#[derive(Debug, Default)]
pub(super) struct Cache(Mutex<(u64, Option<Snapshot>)>);

impl Cache {
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
    ) -> Vec<pebrel_completions::Suggestion> {
        let cwd = Path::new(cwd);
        if !cwd.is_absolute() || cancelled() {
            return Vec::new();
        }
        let key = (cwd.to_owned(), context.directories.clone());
        let (generation, cached) = {
            let state = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            (
                state.0,
                state
                    .1
                    .as_ref()
                    .filter(|s| s.key == key && s.fetched.elapsed() < Duration::from_secs(2))
                    .map(|s| s.names.clone()),
            )
        };
        let names = cached.unwrap_or_else(|| {
            let names: Arc<[String]> =
                read_names(cwd, &context.directories, cancelled).unwrap_or_default().into();
            let mut state = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            // 不持锁读磁盘；提交或关闭期间返回的旧快照不能重新进入缓存。
            if !cancelled() && generation == state.0 {
                state.1 = Some(Snapshot { key, names: names.clone(), fetched: Instant::now() });
            }
            names
        });
        context.candidates(names.iter().take_while(|_| !cancelled()).map(String::as_str))
    }
}

fn read_names(
    cwd: &Path,
    directories: &[String],
    cancelled: &dyn Fn() -> bool,
) -> Option<Vec<String>> {
    const MAX_BYTES: u64 = 1024 * 1024;
    let mut directory = directories.last().map_or_else(|| cwd.to_owned(), |dir| cwd.join(dir));
    for _ in 0..64 {
        if cancelled() || !directory.is_dir() {
            return None;
        }
        let manifest = directory.join("package.json");
        match std::fs::metadata(&manifest) {
            Ok(metadata) => {
                // 找到最近项目即停止：损坏或缺少 scripts 时不能借用父项目的脚本。
                if !metadata.is_file() || metadata.len() > MAX_BYTES {
                    return None;
                }
                let mut bytes = Vec::new();
                std::fs::File::open(manifest)
                    .ok()?
                    .take(MAX_BYTES + 1)
                    .read_to_end(&mut bytes)
                    .ok()?;
                if cancelled() || bytes.len() as u64 > MAX_BYTES {
                    return None;
                }
                let json: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
                return Some(
                    json.get("scripts")?
                        .as_object()?
                        .iter()
                        .filter(|(name, script)| !name.starts_with('-') && script.is_string())
                        .map(|(name, _)| name.clone())
                        .collect(),
                );
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
            Err(_) => return None,
        }
        if !directories.is_empty() || directory.join("node_modules").is_dir() || !directory.pop() {
            return None;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use pebrel_completions::command_context::ShellSyntax;

    #[test]
    fn scripts_follow_project_scope_and_refresh_without_running_project_code() {
        let root = tempfile::tempdir().unwrap();
        let cwd = root.path().join("src");
        std::fs::create_dir(&cwd).unwrap();
        let manifest = root.path().join("package.json");
        std::fs::write(
            &manifest,
            r#"{"scripts":{"build":"touch must-not-run","build:中文":"echo ok","bad":3}}"#,
        )
        .unwrap();
        let cache = Cache::default();
        let query = |line: &str| {
            let context = Context::parse(line, line.len(), ShellSyntax::Posix).unwrap();
            cache.complete(cwd.to_str().unwrap(), &context, &|| false)
        };
        assert_eq!(query("npm run bu").len(), 2);
        assert!(!root.path().join("must-not-run").exists());
        std::fs::write(&manifest, r#"{"scripts":{"build:new":"echo ok"}}"#).unwrap();
        assert_eq!(query("npm run bu").len(), 2);
        cache.invalidate();
        assert_eq!(query("npm run bu")[0].value, "build:new");
        assert!(query("npm --prefix missing run bu").is_empty());
        std::fs::write(cwd.join("package.json"), b"broken").unwrap();
        cache.invalidate();
        assert!(query("npm run bu").is_empty());
        std::fs::write(cwd.join("package.json"), r#"{"scripts":{"build:local":"echo ok"}}"#)
            .unwrap();
        cache.invalidate();
        assert_eq!(query("pnpm run bu")[0].value, "build:local");
        let context = Context::parse("npm run bu", 10, ShellSyntax::Posix).unwrap();
        assert!(cache.complete(cwd.to_str().unwrap(), &context, &|| true).is_empty());
    }
}
