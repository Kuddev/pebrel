//! Bounded local Git discovery for the product's background completion request.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nebula_completions::Suggestion;
use nebula_completions::semantic::{Context, Source};

use crate::runtime_exec::PaneExecContext;

#[derive(Clone, Debug)]
struct Reference {
    full_name: String,
    short_name: String,
    busy: bool,
}

#[derive(Debug)]
struct Snapshot {
    key: String,
    references: Arc<[Reference]>,
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

/// The application has already proven a reference argument and its execution scope.
pub(crate) fn complete(
    cache: &Cache,
    execution: &PaneExecContext,
    cwd: &str,
    context: &Context,
    cancelled: &dyn Fn() -> bool,
) -> Vec<Suggestion> {
    let (branches_only, include_busy) = match context.source {
        Source::Branches { include_busy } => (true, include_busy),
        Source::Revisions { include_busy } | Source::RevisionsAndPaths { include_busy } => {
            (false, include_busy)
        },
        _ => return Vec::new(),
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
                .map(|snapshot| snapshot.references.clone()),
        )
    };
    let references = cached.unwrap_or_else(|| {
        let queried = (|| {
            // 让 Git 处理引用重名，避免用户关闭歧义警告后生成指向错误对象的短名。
            let mut argv = ["git", "-c", "core.warnAmbiguousRefs=true"].map(str::to_owned).to_vec();
            for directory in &context.directories {
                argv.extend(["-C".to_owned(), directory.clone()]);
            }
            // 一次快照供分支和修订参数复用；*objecttype 会剥离嵌套的附注标签。
            argv.extend(
                [
                    "for-each-ref",
                    "--format=%(refname)%00%(refname:short)%00%(symref)%00%(worktreepath)%00%(objecttype)%00%(*objecttype)",
                    "refs/heads/", "refs/remotes/", "refs/tags/",
                ].map(str::to_owned),
            );
            let (mut command, _) =
                crate::runtime_exec::build_command(execution, cwd, &argv).ok()?;
            command
                .env("GIT_OPTIONAL_LOCKS", "0")
                .env("GIT_TERMINAL_PROMPT", "0")
                .env("GIT_NO_LAZY_FETCH", "1")
                // 较旧 Git 不认识 NO_LAZY_FETCH；空协议白名单也禁止按需拉取访问远端。
                .env("GIT_ALLOW_PROTOCOL", "");
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
            let references: Arc<[Reference]> = text
                .lines()
                .filter_map(|line| {
                    let mut fields = line.split('\0');
                    let [Some(full_name), Some(short_name), Some(symref), Some(worktree), Some(object_type), Some(peeled_type)] =
                        std::array::from_fn(|_| fields.next())
                    else {
                        return None;
                    };
                    if fields.next().is_some() || short_name.is_empty()
                        || full_name.chars().chain(short_name.chars()).any(char::is_control)
                        || object_type != "commit" && peeled_type != "commit"
                        || full_name.starts_with("refs/remotes/") && !symref.is_empty()
                    {
                        return None;
                    }
                    Some(Reference {
                        full_name: full_name.to_owned(),
                        short_name: short_name.to_owned(),
                        busy: !worktree.is_empty(),
                    })
                })
                .collect();
            Some(references)
        })();
        let references = queried.unwrap_or_else(|| Arc::from([]));
        let mut state = cache.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        // 提交命令可能已使快照失效，较早请求不能把旧仓库状态重新放回缓存。
        if !cancelled() && state.0 == generation {
            state.1 = Some(Snapshot { key, references: references.clone(), fetched: Instant::now() });
        }
        references
    });
    context.candidates(
        references
            .iter()
            .take_while(|_| !cancelled())
            .filter(|reference| include_busy || !reference.busy)
            .filter_map(|reference| {
                let branch = reference.full_name.strip_prefix("refs/heads/");
                if branches_only {
                    branch
                } else if context.value_prefix().starts_with("refs/") {
                    Some(reference.full_name.as_str())
                } else if branch.is_some()
                    && matches!(context.source, Source::RevisionsAndPaths { .. })
                {
                    // 默认 checkout/switch 要保留分支语义；heads/name 会使 checkout 分离 HEAD。
                    branch
                } else {
                    Some(reference.short_name.as_str())
                }
            }),
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

        git(repository.path(), &["tag", "release/light"]);
        for (name, target) in
            [("release/annotated", "HEAD"), ("release/nested", "release/annotated")]
        {
            git(
                repository.path(),
                &[
                    "-c",
                    "user.name=Fixture",
                    "-c",
                    "user.email=fixture@example.invalid",
                    "-c",
                    "tag.gpgSign=false",
                    "tag",
                    "-a",
                    name,
                    target,
                    "-m",
                    "Fixture",
                ],
            );
        }
        git(repository.path(), &["tag", "release/tree", "HEAD^{tree}"]);
        git(repository.path(), &["update-ref", "refs/remotes/origin/remote-topic", "HEAD"]);
        git(
            repository.path(),
            &["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/remote-topic"],
        );
        git(repository.path(), &["branch", "collision"]);
        git(repository.path(), &["tag", "collision"]);
        git(repository.path(), &["config", "core.warnAmbiguousRefs", "false"]);
        cache.invalidate();
        let values = |line: &str| query(line).into_iter().map(|s| s.value).collect::<Vec<_>>();
        assert!(values("git switch release/").is_empty(), "ordinary switch must not offer tags");
        assert!(
            values("git switch origin/").is_empty(),
            "remote refs require an explicit start point"
        );
        assert_eq!(values("git switch collision"), ["collision"]);
        assert_eq!(values("git checkout collision"), ["collision", "tags/collision"]);
        for line in
            ["git switch --detach collision", "git switch -c new collision", "git merge collision"]
        {
            let mut found = values(line);
            found.sort();
            assert_eq!(found, ["heads/collision", "tags/collision"], "{line}");
        }
        for line in
            ["git switch --detach release/", "git merge release/", "git rebase --onto release/"]
        {
            let mut found = values(line);
            found.sort();
            assert_eq!(found, ["release/annotated", "release/light", "release/nested"], "{line}");
        }
        assert_eq!(values("git merge origin/"), ["origin/remote-topic"]);
        assert_eq!(
            values("git rebase --onto=refs/tags/release/li"),
            ["--onto=refs/tags/release/light"]
        );
        git(repository.path(), &["pack-refs", "--all"]);
        git(repository.path(), &["tag", "-d", "release/light"]);
        cache.invalidate();
        assert!(
            values("git merge release/li").is_empty(),
            "deleted packed tags cannot survive invalidation"
        );
    }
}
