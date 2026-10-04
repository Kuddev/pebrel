//! Bounded local Git discovery for the product's background completion request.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use pebrel_completions::Suggestion;
use pebrel_completions::semantic::{Context, Source, TrackingMode};

use crate::completion::metadata::Execution;
use crate::runtime_exec::PaneExecContext;

mod tracking;

#[derive(Clone, Debug)]
struct Reference {
    full_name: String,
    short_name: String,
    busy: bool,
    current: bool,
    commit: bool,
    symbolic: bool,
    direct_tracking: bool,
}

#[derive(Debug, Default)]
struct Repository {
    references: Vec<Reference>,
    guesses: Vec<String>,
    remotes: Vec<String>,
    remote_branches: std::collections::BTreeMap<String, Vec<String>>,
    guess_enabled: bool,
    local_branches: HashSet<String>,
}

#[derive(Debug)]
struct Snapshot {
    key: String,
    repository: Arc<Repository>,
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
pub(crate) fn complete_available(
    cache: &Cache,
    execution: &Execution,
    cwd: &str,
    context: &Context,
    cancelled: &dyn Fn() -> bool,
) -> Option<Vec<Suggestion>> {
    let (branches_only, include_busy) = match context.source {
        Source::Branches { include_busy } => (true, include_busy),
        Source::Tracking { .. }
        | Source::Remotes
        | Source::RemoteBranches
        | Source::PushRefs { .. } => (false, true),
        Source::Revisions { include_busy } | Source::RevisionsAndPaths { include_busy } => {
            (false, include_busy)
        },
        _ => return None,
    };
    if cwd.is_empty() || cancelled() {
        return None;
    }
    let key = format!(
        "{}\0{cwd}\0{:?}\0{}",
        execution.key(),
        context.directories,
        matches!(context.source, Source::Remotes)
    );
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
                .map(|snapshot| snapshot.repository.clone()),
        )
    };
    let repository = if let Some(cached) = cached {
        cached
    } else {
        let repository = Arc::new(query(execution, cwd, context, cancelled)?);
        let mut state = cache.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        // 提交命令可能已使快照失效，较早请求不能把旧仓库状态重新放回缓存。
        if !cancelled() && state.0 == generation {
            state.1 =
                Some(Snapshot { key, repository: repository.clone(), fetched: Instant::now() });
        }
        repository
    };
    if matches!(context.source, Source::Remotes) {
        return Some(context.candidates(
            repository.remotes.iter().take_while(|_| !cancelled()).map(String::as_str),
        ));
    }
    if matches!(context.source, Source::RemoteBranches | Source::PushRefs { destination: true }) {
        return Some(
            context.candidates(
                context
                    .remote
                    .as_ref()
                    .and_then(|remote| repository.remote_branches.get(remote))
                    .into_iter()
                    .flatten()
                    .take_while(|_| !cancelled())
                    .map(String::as_str),
            ),
        );
    }
    if matches!(context.source, Source::PushRefs { destination: false }) {
        return Some(
            context.candidates(
                repository
                    .references
                    .iter()
                    .filter(|r| !r.symbolic && r.commit)
                    .filter_map(|r| {
                        let name = r
                            .full_name
                            .strip_prefix("refs/heads/")
                            .or_else(|| r.full_name.strip_prefix("refs/tags/"))?;
                        Some(if context.value_prefix().starts_with("refs/") {
                            r.full_name.as_str()
                        } else if r.short_name.starts_with("heads/")
                            || r.short_name.starts_with("tags/")
                        {
                            r.short_name.as_str()
                        } else {
                            name
                        })
                    })
                    .take_while(|_| !cancelled()),
            ),
        );
    }
    let references = repository
        .references
        .iter()
        .filter(|r| r.commit && !r.symbolic && (include_busy || !r.busy || r.current))
        .filter(|r| {
            !matches!(context.source, Source::Tracking { mode: TrackingMode::Direct, .. })
                || r.direct_tracking
        })
        .filter_map(|reference| {
            let branch = reference.full_name.strip_prefix("refs/heads/");
            if branches_only {
                branch
            } else if context.value_prefix().starts_with("refs/") {
                Some(reference.full_name.as_str())
            } else if branch.is_some() && matches!(context.source, Source::RevisionsAndPaths { .. })
            {
                // 默认 checkout 要保留分支语义；heads/name 会使 checkout 分离 HEAD。
                branch
            } else {
                Some(reference.short_name.as_str())
            }
        })
        .filter(|value| {
            !matches!(context.source, Source::Tracking { infer_name: true, .. })
                || tracking::inferred_name(value)
                    .is_some_and(|name| !repository.local_branches.contains(name))
        });
    let guess = context.guesses_branches(repository.guess_enabled);
    let mut directory = std::path::PathBuf::from(cwd);
    for change in &context.directories {
        if !change.is_empty() {
            directory.push(change);
        }
    }
    let guesses = repository.guesses.iter().filter(|_| guess).map(String::as_str);
    let mut candidates = context.candidates(references.chain(guesses).take_while(|_| !cancelled()));
    if matches!(context.source, Source::RevisionsAndPaths { .. }) {
        let missing = (!execution.is_host()).then(|| {
            let names: Vec<_> = candidates
                .iter()
                .filter_map(|candidate| {
                    repository
                        .guesses
                        .binary_search_by(|name| name.as_str().cmp(candidate.display_value()))
                        .is_ok()
                        .then(|| candidate.display_value().to_owned())
                })
                .collect();
            execution
                .paths_not_found(cwd, &context.directories, &names, cancelled)
                .unwrap_or_default()
        });
        // 先匹配再检查文件重名：磁盘探测最多覆盖本次返回的 256 个候选。
        candidates.retain(|candidate| {
            !cancelled() && (repository.guesses.binary_search_by(|name| name.as_str().cmp(candidate.display_value())).is_err()
                || if let Some(missing) = &missing { missing.contains(candidate.display_value()) }
                else { matches!(std::fs::symlink_metadata(directory.join(candidate.display_value())), Err(error) if error.kind() == std::io::ErrorKind::NotFound) })
        });
    }
    (!cancelled()).then_some(candidates)
}

#[cfg(test)]
fn complete(
    cache: &Cache,
    execution: &Execution,
    cwd: &str,
    context: &Context,
    cancelled: &dyn Fn() -> bool,
) -> Vec<Suggestion> {
    complete_available(cache, execution, cwd, context, cancelled).unwrap_or_default()
}

fn query(
    execution: &Execution,
    cwd: &str,
    context: &Context,
    cancelled: &dyn Fn() -> bool,
) -> Option<Repository> {
    // 两次元数据读取共用总预算；缓存命中及逐字筛选不再启动子进程。
    let deadline = Instant::now() + Duration::from_secs(3);
    let stopped = || cancelled() || Instant::now() >= deadline;
    let mut remaining = 1024 * 1024;
    let mut read = |args: &[&str]| {
        if stopped() {
            return None;
        }
        let bytes =
            execution.git(cwd, &context.directories, args, deadline, remaining, &stopped)?;
        remaining -= bytes.len();
        String::from_utf8(bytes).ok()
    };
    if matches!(context.source, Source::Remotes) {
        let remotes = read(&["remote"])?;
        return (!stopped()).then_some(Repository {
            remotes: remotes
                .lines()
                .filter(|name| {
                    !name.is_empty()
                        && !name.starts_with('-')
                        && !name.chars().any(char::is_control)
                })
                .map(str::to_owned)
                .collect(),
            ..Default::default()
        });
    }
    // 哨兵保证无相关配置也是成功的空结果；只读取所需键，不收集 URL、凭据等配置。
    let config = tracking::Config::parse(&read(&[
        "config",
        "--null",
        "--get-regexp",
        "^(checkout\\.(guess|defaultremote)|remote\\..*\\.fetch|completion\\.snapshot)$",
    ])?);
    let text = read(&[
        "for-each-ref",
        "--format=%(refname)%00%(refname:short)%00%(symref)%00%(worktreepath)%00%(objecttype)%00%(*objecttype)%00%(HEAD)",
    ])?;
    let references: Vec<_> = text
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\0');
            let [
                Some(full_name),
                Some(short_name),
                Some(symref),
                Some(worktree),
                Some(object_type),
                Some(peeled_type),
                Some(head),
            ] = std::array::from_fn(|_| fields.next())
            else {
                return None;
            };
            if fields.next().is_some()
                || short_name.is_empty()
                || full_name.chars().chain(short_name.chars()).any(char::is_control)
            {
                return None;
            }
            Some(Reference {
                full_name: full_name.to_owned(),
                short_name: short_name.to_owned(),
                busy: !worktree.is_empty(),
                current: head == "*",
                commit: object_type == "commit" || peeled_type == "commit",
                symbolic: !symref.is_empty(),
                direct_tracking: config.can_track(full_name, &stopped),
            })
        })
        .collect();
    let guesses = config.guesses(&references, &stopped);
    let remote_branches = config.remote_branches(&references, &stopped);
    let local_branches = references
        .iter()
        .filter_map(|r| r.full_name.strip_prefix("refs/heads/").map(str::to_owned))
        .collect();
    (!stopped()).then_some(Repository {
        references,
        guesses,
        guess_enabled: config.guess,
        remotes: Vec::new(),
        remote_branches,
        local_branches,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn retain_snapshot_for_test(cache: &Cache) {
        let mut state = cache.0.lock().unwrap();
        state.1.as_mut().expect("queried repository").fetched =
            Instant::now() + Duration::from_secs(60);
    }
    use pebrel_completions::command_context::ShellSyntax;

    pub(crate) fn git(cwd: &std::path::Path, args: &[&str]) {
        git_output(cwd, args);
    }

    pub(crate) fn git_output(cwd: &std::path::Path, args: &[&str]) -> String {
        let mut command = std::process::Command::new("git");
        command.current_dir(cwd).args(args);
        let output = crate::platform::process::hidden_command(&mut command).output().unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
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
    fn real_explicit_tracking_creates_and_inherits_the_expected_upstream() {
        let repository = repository();
        let cwd = repository.path().to_str().unwrap();
        let execution = PaneExecContext::from_pty_options(&nebula_terminal::tty::Options {
            working_directory: Some(repository.path().to_owned()),
            ..Default::default()
        });
        let cache = Cache::default();
        let values = |line: &str| {
            let context = Context::parse(line, line.len(), ShellSyntax::Posix).unwrap();
            complete(
                &cache,
                &Execution::Process {
                    context: execution.clone(),
                    scope: crate::display::SuggestEnv::Local,
                },
                cwd,
                &context,
                &|| false,
            )
            .into_iter()
            .map(|s| s.value)
            .collect::<Vec<_>>()
        };
        git(repository.path(), &["remote", "add", "origin", "https://example.invalid/tracking"]);
        for name in ["only", "off", "source", "excluded"] {
            git(repository.path(), &["update-ref", &format!("refs/remotes/origin/{name}"), "HEAD"]);
        }
        git(repository.path(), &["update-ref", "refs/remotes/unknown/unmapped", "HEAD"]);
        git(repository.path(), &["branch", "--track", "local/source", "origin/source"]);
        git(repository.path(), &["tag", "release/one"]);
        git(repository.path(), &["config", "--add", "remote.origin.fetch", "^refs/heads/excluded"]);
        assert_eq!(values("git switch -t origin/on"), ["origin/only"]);
        assert_eq!(
            values("git switch --track refs/remotes/origin/on"),
            ["refs/remotes/origin/only"]
        );
        assert_eq!(values("git switch --track -c fresh ma"), ["main"]);
        assert!(values("git switch --track -c fresh release/").is_empty());
        assert!(values("git switch --track unknown/").is_empty());
        assert!(values("git switch --track origin/excl").is_empty());
        assert_eq!(values("git switch --no-track origin/excl"), ["origin/excluded"]);
        assert_eq!(values("git switch --track=inherit -c fresh local/so"), ["local/source"]);
        git(repository.path(), &["switch", "-t", "origin/only"]);
        assert_eq!(
            git_output(repository.path(), &["rev-parse", "--symbolic-full-name", "@{upstream}"])
                .trim(),
            "refs/remotes/origin/only"
        );
        cache.invalidate();
        assert!(
            values("git switch -t origin/on").is_empty(),
            "cannot infer an already-existing branch name"
        );
        let inherited = values("git switch --track=inherit -c inherited local/so");
        assert_eq!(inherited, ["local/source"]);
        git(repository.path(), &["switch", "--track=inherit", "-c", "inherited", &inherited[0]]);
        assert_eq!(
            git_output(repository.path(), &["rev-parse", "--symbolic-full-name", "@{upstream}"])
                .trim(),
            "refs/remotes/origin/source"
        );
        assert_eq!(values("git switch --no-track origin/of"), ["origin/off"]);
        git(repository.path(), &["switch", "--no-track", "origin/off"]);
        assert!(
            git_output(
                repository.path(),
                &["for-each-ref", "--format=%(upstream)", "refs/heads/off"]
            )
            .trim()
            .is_empty()
        );
        git(repository.path(), &["switch", "--track", "-c", "local-direct", "main"]);
        assert_eq!(
            git_output(repository.path(), &["rev-parse", "--symbolic-full-name", "@{upstream}"])
                .trim(),
            "refs/heads/main"
        );
        git(
            repository.path(),
            &["config", "remote.duplicate.fetch", "+refs/heads/*:refs/remotes/origin/*"],
        );
        git(repository.path(), &["config", "checkout.defaultRemote", "origin"]);
        cache.invalidate();
        assert!(
            values("git switch --track -c fresh origin/").is_empty(),
            "explicit tracking cannot use defaultRemote to resolve duplicate destination mappings"
        );
        assert_eq!(values("git switch --no-track -c fresh origin/of"), ["origin/off"]);
    }

    #[test]
    fn real_remote_guesses_follow_configuration_and_refspecs() {
        let repository = repository();
        let cwd = repository.path().to_str().unwrap();
        let execution = PaneExecContext::from_pty_options(&nebula_terminal::tty::Options {
            working_directory: Some(repository.path().to_owned()),
            ..Default::default()
        });
        let cache = Cache::default();
        let values = |line: &str| {
            let context = Context::parse(line, line.len(), ShellSyntax::Posix).unwrap();
            // Unavailable metadata is a valid bounded-request outcome, not an
            // empty repository. Establish fixture readiness before asserting
            // semantics; a successful empty result is never retried.
            (1..=3)
                .find_map(|read| {
                    let result = complete_available(
                        &cache,
                        &Execution::Process {
                            context: execution.clone(),
                            scope: crate::display::SuggestEnv::Local,
                        },
                        cwd,
                        &context,
                        &|| false,
                    );
                    if result.is_none() {
                        eprintln!("Git fixture metadata unavailable: read {read}/3, {line:?}");
                    }
                    result
                })
                .expect("valid Git fixture metadata must become available before assertions")
                .into_iter()
                .map(|s| s.value)
                .collect::<Vec<_>>()
        };
        for remote in ["origin", "upstream"] {
            git(
                repository.path(),
                &["remote", "add", remote, "https://example.invalid/completion"],
            );
        }
        for name in [
            "origin/discover/one",
            "origin/discover/中文",
            "origin/shared/topic",
            "upstream/shared/topic",
            "unknown/phantom",
            "origin/tag-shadow",
            "origin/excluded",
        ] {
            git(repository.path(), &["update-ref", &format!("refs/remotes/{name}"), "HEAD"]);
        }
        git(repository.path(), &["tag", "tag-shadow", "HEAD^{tree}"]);
        git(repository.path(), &["config", "--add", "remote.origin.fetch", "^refs/heads/excluded"]);
        assert_eq!(values("git switch discover/o"), ["discover/one"]);
        assert_eq!(values("git switch discover/中"), ["discover/中文"]);
        assert!(values("git switch shared/").is_empty());
        assert!(values("git switch phantom").is_empty());
        assert!(values("git switch tag-shadow").is_empty());
        assert!(values("git switch excluded").is_empty());
        assert!(!values("git switch --no-track discover/").contains(&"discover/one".to_owned()));
        assert!(values("git switch --no-guess discover/").is_empty());
        git(repository.path(), &["config", "checkout.defaultRemote", "upstream"]);
        cache.invalidate();
        assert_eq!(values("git switch shared/"), ["shared/topic"]);
        // 执行补出的命令并核对 upstream，不能仅凭候选文本认定推断正确。
        git(repository.path(), &["switch", "shared/topic"]);
        let upstream =
            git_output(repository.path(), &["rev-parse", "--symbolic-full-name", "@{upstream}"]);
        assert_eq!(upstream.trim(), "refs/remotes/upstream/shared/topic");
        git(repository.path(), &["switch", "main"]);
        git(repository.path(), &["config", "checkout.guess", "false"]);
        cache.invalidate();
        assert!(values("git switch discover/").is_empty());
        assert_eq!(values("git switch --guess discover/o"), ["discover/one"]);
        git(repository.path(), &["config", "checkout.guess", "true"]);
        cache.invalidate();
        assert_eq!(values("git checkout discover/o"), ["discover/one", "origin/discover/one"]);
        std::fs::create_dir(repository.path().join("discover")).unwrap();
        std::fs::write(repository.path().join("discover/one"), "path collision").unwrap();
        assert_eq!(values("git checkout discover/o"), ["origin/discover/one"]);
        assert_eq!(values("git switch discover/o"), ["discover/one"]);

        git(repository.path(), &["remote", "add", "custom", "https://example.invalid/custom"]);
        git(
            repository.path(),
            &["config", "remote.custom.fetch", "+refs/heads/custom/*:refs/vendor/pre-*-post"],
        );
        git(repository.path(), &["update-ref", "refs/vendor/pre-中文-post", "HEAD"]);
        git(
            repository.path(),
            &["config", "--add", "remote.custom.fetch", "refs/heads/exact:refs/vendor/single"],
        );
        git(repository.path(), &["update-ref", "refs/vendor/single", "HEAD"]);
        git(
            repository.path(),
            &["config", "--add", "remote.custom.fetch", "refs/heads/ordered:refs/vendor/missing"],
        );
        git(
            repository.path(),
            &["config", "--add", "remote.custom.fetch", "refs/heads/ordered:refs/vendor/present"],
        );
        git(repository.path(), &["update-ref", "refs/vendor/present", "HEAD"]);
        cache.invalidate();
        assert_eq!(values("git switch custom/"), ["custom/中文"]);
        assert_eq!(values("git switch exact"), ["exact"]);
        assert!(values("git switch ordered").is_empty());
        git(repository.path(), &["switch", "custom/中文"]);
        git(repository.path(), &["switch", "main"]);
        git(repository.path(), &["switch", "exact"]);
        git(repository.path(), &["switch", "main"]);
        git(
            repository.path(),
            &["update-ref", "refs/remotes/upstream/discover/one", "HEAD^{tree}"],
        );
        git(repository.path(), &["config", "--unset", "checkout.defaultRemote"]);
        cache.invalidate();
        assert!(
            values("git switch discover/o").is_empty(),
            "non-commit targets still make remote selection ambiguous"
        );
        git(repository.path(), &["config", "checkout.defaultRemote", "origin"]);
        cache.invalidate();
        assert_eq!(values("git switch discover/o"), ["discover/one"]);
        for (value, expected) in
            [("off", false), ("yes", true), ("0x1", true), ("0k", false), ("1M", true)]
        {
            git(repository.path(), &["config", "checkout.guess", value]);
            cache.invalidate();
            assert_eq!(!values("git switch discover/o").is_empty(), expected, "{value}");
        }
        git(repository.path(), &["pack-refs", "--all"]);
        git(repository.path(), &["update-ref", "-d", "refs/remotes/origin/discover/中文"]);
        cache.invalidate();
        assert!(values("git switch discover/中").is_empty());
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
            complete(
                &cache,
                &Execution::Process {
                    context: execution.clone(),
                    scope: crate::display::SuggestEnv::Local,
                },
                cwd,
                &context,
                &|| false,
            )
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
        assert_eq!(
            query("git switch ma")[0].value,
            "main",
            "the current branch remains a valid switch target"
        );
        assert!(
            query("git -C missing switch fe").is_empty(),
            "never fall back to the wrong repository"
        );
        assert!(
            complete(
                &cache,
                &Execution::Process {
                    context: execution.clone(),
                    scope: crate::display::SuggestEnv::Local
                },
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
