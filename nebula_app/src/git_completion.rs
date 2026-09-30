//! Bounded local Git discovery for the product's background completion request.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nebula_completions::Suggestion;
use nebula_completions::semantic::{Context, Source};

use crate::runtime_exec::PaneExecContext;

#[derive(Clone, Debug)]
struct Branch {
    name: String,
    busy: bool,
}

#[derive(Debug)]
struct Snapshot {
    key: String,
    branches: Arc<[Branch]>,
    fetched: Instant,
}

/// Each pane owns one repository snapshot; prefixes reuse it without another Git process.
#[derive(Debug, Default)]
pub(crate) struct Cache(Mutex<(u64, Option<Snapshot>)>);

impl Cache {
    pub(crate) fn invalidate(&self) {
        let mut state = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        state.0 = state.0.wrapping_add(1);
        state.1 = None;
    }
}

/// The application has already proven a branch argument and its execution scope.
pub(crate) fn complete(
    cache: &Cache,
    execution: &PaneExecContext,
    cwd: &str,
    context: &Context,
    cancelled: &dyn Fn() -> bool,
) -> Vec<Suggestion> {
    let Source::Branches { include_busy } = context.source else {
        return Vec::new();
    };
    if cwd.is_empty() || execution.wsl_distribution().is_some() || cancelled() {
        return Vec::new();
    }
    let key = format!("{cwd}\0{:?}", context.directories);
    let (generation, cached) = {
        let guard = cache.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        (
            guard.0,
            guard
                .1
                .as_ref()
                .filter(|snapshot| {
                    snapshot.key == key && snapshot.fetched.elapsed() < Duration::from_secs(2)
                })
                .map(|snapshot| snapshot.branches.clone()),
        )
    };
    let branches = cached.unwrap_or_else(|| {
        let queried = (|| {
            let mut argv = vec!["git".to_owned()];
            for directory in &context.directories {
                argv.extend(["-C".to_owned(), directory.clone()]);
            }
            // full ref names avoid ambiguity with tags; worktreepath also covers linked worktrees.
            argv.extend(
                ["for-each-ref", "--format=%(refname)%00%(worktreepath)", "refs/heads/"]
                    .map(str::to_owned),
            );
            let (mut command, _) =
                crate::runtime_exec::build_command(execution, cwd, &argv).ok()?;
            command.env("GIT_OPTIONAL_LOCKS", "0").env("GIT_TERMINAL_PROMPT", "0");
            let bytes = crate::platform::process_output::read_cancellable(
                command,
                // CI 的真实 Windows 查询曾超过 750 ms；后台清理上限不能当作按键延迟预算。
                // 输入变化仍由取消信号终止旧进程，正常查询完成后立即返回。
                Duration::from_secs(3),
                1024 * 1024,
                cancelled,
            )
            .inspect_err(|error| {
                log::debug!("Local Git completion query failed: {error}");
                #[cfg(test)]
                eprintln!("Local Git completion query failed: {error}");
            })
            .ok()?;
            let text = std::str::from_utf8(&bytes).ok()?;
            let branches: Arc<[Branch]> = text
                .lines()
                .filter_map(|line| {
                    let (name, worktree) = line.split_once('\0')?;
                    let name = name.strip_prefix("refs/heads/")?;
                    if name.is_empty() || name.chars().any(char::is_control) {
                        return None;
                    }
                    Some(Branch { name: name.to_owned(), busy: !worktree.is_empty() })
                })
                .collect();
            Some(branches)
        })();
        let branches = queried.unwrap_or_else(|| Arc::from([]));
        let mut state = cache.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        // 提交命令可能已使快照失效，较早请求不能把旧仓库状态重新放回缓存。
        if !cancelled() && state.0 == generation {
            state.1 = Some(Snapshot { key, branches: branches.clone(), fetched: Instant::now() });
        }
        branches
    });
    context.candidates(
        branches
            .iter()
            .take_while(|_| !cancelled())
            .filter(|branch| include_busy || !branch.busy)
            .map(|branch| branch.name.as_str()),
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use nebula_completions::command_context::ShellSyntax;

    pub(crate) fn git(cwd: &std::path::Path, args: &[&str]) {
        let mut command = std::process::Command::new("git");
        command.current_dir(cwd).args(args);
        let output = crate::platform::process::hidden_command(&mut command).output().unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    pub(crate) fn repository() -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        git(directory.path(), &["init", "-b", "main"]);
        git(
            directory.path(),
            &[
                "-c",
                "user.name=Completion Test",
                "-c",
                "user.email=test@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--allow-empty",
                "-m",
                "Fixture",
            ],
        );
        git(directory.path(), &["branch", "feature/中文"]);
        git(directory.path(), &["branch", "feature/alpha"]);
        directory
    }

    #[test]
    fn real_branches_cache_invalidation_and_directory_context() {
        let repository = repository();
        let cwd = repository.path().to_str().unwrap();
        let options = nebula_terminal::tty::Options {
            working_directory: Some(repository.path().to_owned()),
            ..Default::default()
        };
        let execution = PaneExecContext::from_pty_options(&options);
        let cache = Cache::default();
        git(repository.path(), &["pack-refs", "--all"]);
        let query = |line: &str| {
            let context = Context::parse(line, line.len(), ShellSyntax::Posix).unwrap();
            complete(&cache, &execution, cwd, &context, &|| false)
        };
        assert_eq!(query("git switch fe").len(), 2);
        git(repository.path(), &["branch", "feature/beta"]);
        assert_eq!(query("git switch fe").len(), 2, "prefix matching reuses the snapshot");
        cache.invalidate();
        assert_eq!(query("git switch fe").len(), 3);
        let linked = tempfile::tempdir().unwrap();
        let worktree = linked.path().join("linked worktree");
        git(repository.path(), &["worktree", "add", worktree.to_str().unwrap(), "feature/alpha"]);
        cache.invalidate();
        assert_eq!(query("git switch fe").len(), 2, "exclude branches checked out elsewhere");
        assert_eq!(query("git switch --ignore-other-worktrees fe").len(), 3);
        assert_eq!(query("git switch --detach ma").len(), 1);
        assert!(query("git switch ma").is_empty(), "checked-out branch is not a switch target");
        assert!(
            query("git -C missing switch fe").is_empty(),
            "never fall back to the wrong repository"
        );
        assert!(
            complete(
                &cache,
                &execution,
                cwd,
                &Context::parse("git switch fe", 13, ShellSyntax::Posix).unwrap(),
                &|| true
            )
            .is_empty()
        );
    }
}
