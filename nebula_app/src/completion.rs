//! 应用补齐入口：管理共享来源和请求快照，不持有窗口、终端网格或执行器。

use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, Ordering},
};

use crate::directory_history::DirectoryHistory;
use crate::display::suggest_engine::{self, HistorySource, Input, SuggestSources};
use crate::display::{CompletionStyle, SuggestEnv};
use crate::nebula_history::{HistoryScope, NebulaHistory};
use crate::runtime_exec::PaneExecContext;
use nebula_completions::command_context::ShellSyntax;
use nebula_completions::semantic::{Context as SemanticContext, Source};

mod connections;
pub(crate) mod paths;
mod project_scripts;

pub(crate) use suggest_engine::Candidates;

// 历史只加载一份，避免多个 pane 在退出时互相覆盖；扫描期间不持有历史锁。
struct Shared {
    history: Mutex<NebulaHistory>,
    directories: DirectoryHistory,
    commands: Arc<Mutex<Vec<String>>>,
}

fn shared() -> &'static Shared {
    static SHARED: OnceLock<Shared> = OnceLock::new();
    SHARED.get_or_init(|| Shared {
        history: Mutex::new(NebulaHistory::load()),
        directories: crate::directory_history::global(),
        commands: crate::display::nebula_commands_handle(),
    })
}

#[derive(Clone, Default)]
pub(crate) struct Cancellation(Arc<AtomicBool>);

impl Cancellation {
    pub(crate) fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    pub(crate) fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// 来源缓存跟随 pane，具体来源及其适用条件不再由界面决定。
#[derive(Debug, Default)]
pub(crate) struct Session {
    git: Arc<crate::git_completion::Cache>,
    scripts: Arc<project_scripts::Cache>,
    connections: Arc<connections::Cache>,
}

impl Session {
    pub(crate) fn invalidate(&self) {
        self.git.invalidate();
        self.scripts.invalidate();
        self.connections.invalidate();
    }

    pub(crate) fn request(
        &self,
        cwd: String,
        env: SuggestEnv,
        line: String,
        style: CompletionStyle,
        execution: Option<&PaneExecContext>,
    ) -> Request {
        let env = if env.is_this_machine()
            && let Some(distro) = execution.and_then(PaneExecContext::wsl_distribution)
        {
            // 执行快照已确认是 WSL 时，尚未刷新的视图标签不能放行宿主文件系统。
            SuggestEnv::Wsl { distro: distro.unwrap_or_default().to_owned() }
        } else {
            env
        };
        let local =
            env.is_this_machine() && execution.is_none_or(|e| e.wsl_distribution().is_none());
        // 方言是输入事实；远端/嵌套 shell 未证明方言时只接受通用字面量。
        let syntax = if local {
            execution
                .and_then(PaneExecContext::shell_program)
                .map(ShellSyntax::for_program)
                .unwrap_or_else(|| {
                    ShellSyntax::for_program(&crate::platform::shell::default_shell_id())
                })
        } else {
            ShellSyntax::Literal
        };
        let semantic = SemanticContext::parse(&line, line.len(), syntax).filter(|context| {
            // 无目录通道的嵌套 shell 仍能召回本会话历史；空路径来源不应把历史挡住。
            !matches!(context.source, Source::Paths { .. }) || local || env.can_query_remote_paths()
        });
        // 只有需要本机 Git I/O 的请求才复制启动环境。
        let git = if local
            && semantic.as_ref().is_some_and(|c| {
                matches!(
                    c.source,
                    Source::Branches { .. }
                        | Source::Revisions { .. }
                        | Source::RevisionsAndPaths { .. }
                )
            }) {
            execution.cloned().map(|execution| (self.git.clone(), execution))
        } else {
            None
        };
        let scripts = local.then(|| self.scripts.clone());
        let connections = local.then(|| self.connections.clone());
        Request { cwd, env, line, style, git, semantic, scripts, connections, syntax }
    }
}

/// 后台只收到当前输入与已确认的执行环境，不借用整个终端状态。
pub(crate) struct Request {
    cwd: String,
    env: SuggestEnv,
    line: String,
    style: CompletionStyle,
    git: Option<(Arc<crate::git_completion::Cache>, PaneExecContext)>,
    semantic: Option<SemanticContext>,
    scripts: Option<Arc<project_scripts::Cache>>,
    connections: Option<Arc<connections::Cache>>,
    syntax: ShellSyntax,
}

impl Request {
    pub(crate) fn calculate(self, cancellation: &Cancellation) -> Candidates {
        if cancellation.is_cancelled() {
            return Candidates::default();
        }
        let semantic = self.semantic.as_ref().map(|context| match context.source {
            Source::Words(_) | Source::Options => context.static_candidates(),
            Source::Branches { .. }
            | Source::Revisions { .. }
            | Source::RevisionsAndPaths { .. } => {
                self.git.as_ref().map_or_else(Vec::new, |(cache, execution)| {
                    crate::git_completion::complete(cache, execution, &self.cwd, context, &|| {
                        cancellation.is_cancelled()
                    })
                })
            },
            Source::ProjectScripts => self.scripts.as_ref().map_or_else(Vec::new, |cache| {
                cache.complete(&self.cwd, context, &|| cancellation.is_cancelled())
            }),
            Source::SshHosts { .. } | Source::WslDistributions => {
                self.connections.as_ref().map_or_else(Vec::new, |cache| {
                    cache.complete(&self.cwd, context, &|| cancellation.is_cancelled())
                })
            },
            Source::None | Source::Paths { .. } => Vec::new(),
        });
        if cancellation.is_cancelled() {
            return Candidates::default();
        }
        if let Some(candidates) = semantic {
            let mut candidates: Vec<nebula_completions::SemanticSuggestion> =
                candidates.into_iter().map(Into::into).collect();
            let mut pending = None;
            if let Some(context) = self.semantic.as_ref().filter(|c| {
                matches!(c.source, Source::Paths { .. } | Source::RevisionsAndPaths { .. })
            }) {
                let (paths, demand) = paths::complete(
                    &Input { cwd: &self.cwd, env: &self.env, line: &self.line },
                    self.syntax,
                    &shared().directories,
                    self.style,
                    Some(context),
                    &|| cancellation.is_cancelled(),
                );
                candidates.extend(paths);
                pending = demand;
            }
            // 历史只给仍然有效的语义候选提权，不能复活已删除的分支/脚本。
            if !candidates.is_empty() {
                let recent = shared()
                    .history
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .hint_with_cancel(&self.env.history_scope(), &self.line, &|| {
                        cancellation.is_cancelled()
                    })
                    .map(|suffix| format!("{}{suffix}", self.line));
                if let Some(recent) = recent {
                    if let Some(index) = candidates.iter().position(|candidate| {
                        let candidate = &candidate.suggestion;
                        self.line.get(..candidate.span.start).is_some_and(|head| {
                            recent.strip_prefix(head) == Some(candidate.value.as_str())
                        })
                    }) {
                        candidates[..=index].rotate_right(1);
                    }
                }
            }
            if cancellation.is_cancelled() {
                return Candidates::default();
            }
            let mut result =
                suggest_engine::semantic_candidates(&self.line, self.style, candidates);
            result.pending_remote_dir = pending;
            return result;
        }
        let sources = shared();
        suggest_engine::calculate(
            &SuggestSources {
                history: HistorySource::Shared(&sources.history),
                directories: &sources.directories,
                commands: &sources.commands,
                enabled: true,
                style: self.style,
            },
            &Input { cwd: &self.cwd, env: &self.env, line: &self.line },
            self.syntax,
            &|| cancellation.is_cancelled(),
        )
    }
}

pub(crate) fn cache_key(cwd: &str, env: &SuggestEnv, line: &str, style: CompletionStyle) -> String {
    let commands = crate::display::nebula_commands_handle();
    let generation = commands.lock().map(|commands| commands.len()).unwrap_or(0);
    suggest_engine::suggestion_key(&Input { cwd, env, line }, style, generation)
}

pub(crate) fn record_command(scope: &HistoryScope, line: &str, cwd: &str) {
    shared()
        .history
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .record(scope, line, cwd);
}

pub(crate) fn record_directory(cwd: &str) {
    if !cwd.is_empty() {
        shared().directories.record(cwd);
    }
}

#[cfg(test)]
pub(crate) fn history_hint_for_test(scope: &HistoryScope, prefix: &str) -> Option<String> {
    shared()
        .history
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .hint(scope, prefix)
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_completion_uses_config_includes_and_never_leaks_host_data() {
        let root = tempfile::tempdir().unwrap();
        let included = root.path().join("included.conf");
        std::fs::write(&included, "Host completion-prod completion-stage\nHost * !excluded bad?\n")
            .unwrap();
        std::fs::write(
            root.path().join("config"),
            format!("Include \"{}\"\n", included.to_string_lossy().replace('\\', "/")),
        )
        .unwrap();
        std::fs::write(root.path().join("identity.pem"), b"fixture").unwrap();
        let session = Session::default();
        let query = |env, line: &str, style| {
            session
                .request(root.path().to_string_lossy().into(), env, line.into(), style, None)
                .calculate(&Cancellation::default())
        };
        for style in [CompletionStyle::Inline, CompletionStyle::Popup, CompletionStyle::Hybrid] {
            let r = query(SuggestEnv::Local, "ssh -F config me@completion-pr", style);
            if style == CompletionStyle::Popup {
                assert_eq!(r.completion_items[0].insert, "od");
            } else {
                assert_eq!(r.suggestion, "od");
            }
        }
        for (program, expected) in
            [("sh", "-iidentity.pem"), ("pwsh", "'-iidentity.pem'"), ("cmd", "-iidentity.pem")]
        {
            let execution = PaneExecContext::from_pty_options(&nebula_terminal::tty::Options {
                shell: Some(nebula_terminal::tty::Shell::new(program.into(), vec![])),
                ..Default::default()
            });
            let line = "ssh -F config -iidentity";
            for style in [CompletionStyle::Inline, CompletionStyle::Popup, CompletionStyle::Hybrid]
            {
                let result = session
                    .request(
                        root.path().to_string_lossy().into(),
                        SuggestEnv::Local,
                        line.into(),
                        style,
                        Some(&execution),
                    )
                    .calculate(&Cancellation::default());
                let edit = if style == CompletionStyle::Popup {
                    &result.completion_items[0]
                } else {
                    result.suggestion_edit.as_ref().unwrap()
                };
                let accepted: String = line
                    .chars()
                    .take(line.chars().count() - edit.replace_chars)
                    .chain(edit.insert.chars())
                    .collect();
                assert_eq!(accepted, format!("ssh -F config {expected}"), "{program} {style:?}");
            }
        }
        assert!(
            query(SuggestEnv::Local, "ssh -F none completion-", CompletionStyle::Popup)
                .completion_items
                .is_empty()
        );
        std::fs::write(&included, "Host completion-new\n").unwrap();
        session.invalidate();
        assert_eq!(
            query(SuggestEnv::Local, "ssh -F config completion-", CompletionStyle::Popup)
                .completion_items[0]
                .insert,
            "new"
        );
        for env in [
            SuggestEnv::Wsl { distro: "source-isolation".into() },
            SuggestEnv::Ssh { destination: "source-isolation.invalid".into() },
        ] {
            assert!(
                query(env.clone(), "ssh -F config completion-", CompletionStyle::Popup)
                    .completion_items
                    .is_empty()
            );
            assert!(
                query(env.clone(), "wsl -d ", CompletionStyle::Popup).completion_items.is_empty()
            );
            crate::remote_dirs::finish_fetch(
                &env,
                "/project",
                Some(vec![crate::remote_dirs::RemoteEntry {
                    name: "file.txt".into(),
                    is_dir: false,
                }]),
            );
            let r = session
                .request("/project".into(), env, "ls -al fi".into(), CompletionStyle::Popup, None)
                .calculate(&Cancellation::default());
            assert_eq!(r.completion_items[0].insert, "le.txt");
        }
        let names = crate::platform::shell::registered_wsl_distros(&|| false);
        let r = query(SuggestEnv::Local, "wsl -d ", CompletionStyle::Popup);
        assert_eq!(r.completion_items.len(), names.len());
        let execution = PaneExecContext::from_pty_options(&nebula_terminal::tty::Options {
            shell: Some(nebula_terminal::tty::Shell::new(
                "wsl.exe".into(),
                vec!["-d".into(), "source-isolation".into()],
            )),
            ..Default::default()
        });
        let result = session
            .request(
                root.path().to_string_lossy().into(),
                SuggestEnv::Local,
                "cat identity".into(),
                CompletionStyle::Popup,
                Some(&execution),
            )
            .calculate(&Cancellation::default());
        assert!(result.completion_items.is_empty(), "stale local labels must not read host paths");
    }

    #[test]
    fn path_completion_preserves_quotes_utf8_types_and_directory_roles_in_all_modes() {
        use crate::display::NebulaCompletionKind;
        use nebula_completions::command_context::CommandContext;
        let directory = tempfile::tempdir().unwrap();
        for name in ["repo 中文", "'quote", "quote", "a...b", "~"] {
            std::fs::create_dir(directory.path().join(name)).unwrap();
            std::fs::write(directory.path().join(name).join("child file.txt"), b"").unwrap();
        }
        std::fs::write(directory.path().join("repo other.txt"), b"").unwrap();
        let session = Session::default();
        for (program, syntax) in [
            ("sh", ShellSyntax::Posix),
            ("pwsh", ShellSyntax::PowerShell),
            ("cmd", ShellSyntax::Cmd),
        ] {
            let execution = PaneExecContext::from_pty_options(&nebula_terminal::tty::Options {
                shell: Some(nebula_terminal::tty::Shell::new(program.into(), vec![])),
                ..Default::default()
            });
            for style in [CompletionStyle::Inline, CompletionStyle::Popup, CompletionStyle::Hybrid]
            {
                for line in [
                    "git -C repo",
                    "git -C \"repo\"",
                    "cat \"repo 中文/ch",
                    "cat 'a...b/ch",
                    "cat \"'quote/ch",
                    "cat '~/ch",
                ] {
                    if syntax == ShellSyntax::Cmd && line.contains('\'') {
                        continue;
                    }
                    let result = session
                        .request(
                            directory.path().to_str().unwrap().into(),
                            SuggestEnv::Local,
                            line.into(),
                            style,
                            Some(&execution),
                        )
                        .calculate(&Cancellation::default());
                    let item = if style == CompletionStyle::Popup {
                        assert_eq!(result.completion_items.len(), 1, "{program}: {line}");
                        &result.completion_items[0]
                    } else {
                        result.suggestion_edit.as_ref().expect(line)
                    };
                    let accepted: String = line
                        .chars()
                        .take(line.chars().count() - item.replace_chars)
                        .chain(item.insert.chars())
                        .collect();
                    let decoded = CommandContext::parse(&accepted, accepted.len(), syntax).unwrap();
                    let target = directory.path().join(decoded.prefix());
                    assert!(target.exists(), "{program}: {accepted}");
                    assert_eq!(
                        item.kind,
                        if target.is_dir() {
                            NebulaCompletionKind::Dir
                        } else {
                            NebulaCompletionKind::File
                        }
                    );
                }
            }
        }
        let line = "git -C \"repo 中文\" checkout -- child";
        let result = session
            .request(
                directory.path().to_str().unwrap().into(),
                SuggestEnv::Local,
                line.into(),
                CompletionStyle::Popup,
                None,
            )
            .calculate(&Cancellation::default());
        assert_eq!(result.completion_items.len(), 1);
        assert_eq!(result.completion_items[0].kind, NebulaCompletionKind::File);
    }

    #[test]
    fn checkout_completion_combines_branches_and_paths_and_scopes_remote_demand() {
        use crate::display::NebulaCompletionKind;
        let repository = crate::git_completion::tests::repository();
        std::fs::write(repository.path().join("feature-file.txt"), b"").unwrap();
        let execution = PaneExecContext::from_pty_options(&nebula_terminal::tty::Options {
            working_directory: Some(repository.path().to_owned()),
            ..Default::default()
        });
        let session = Session::default();
        let result = session
            .request(
                repository.path().to_str().unwrap().into(),
                SuggestEnv::Local,
                "git checkout fe".into(),
                CompletionStyle::Popup,
                Some(&execution),
            )
            .calculate(&Cancellation::default());
        assert_eq!(result.completion_items[0].kind, NebulaCompletionKind::Command);
        assert!(result.completion_items.iter().any(|item| item.kind == NebulaCompletionKind::File));
        let env = SuggestEnv::Ssh { destination: "path-context-test.invalid".into() };
        let query = || {
            session
                .request(
                    "/project".into(),
                    env.clone(),
                    "git -C sub checkout -- fi".into(),
                    CompletionStyle::Popup,
                    None,
                )
                .calculate(&Cancellation::default())
        };
        let result = query();
        assert_eq!(result.pending_remote_dir.as_deref(), Some("/project/sub"));
        crate::remote_dirs::finish_fetch(
            &env,
            "/project/sub",
            Some(vec![crate::remote_dirs::RemoteEntry { name: "file.txt".into(), is_dir: false }]),
        );
        assert_eq!(query().completion_items[0].insert, "le.txt");
    }

    #[test]
    fn completion_requests_work_without_a_view_and_keep_repository_invalidation() {
        let repository = crate::git_completion::tests::repository();
        let cwd = repository.path().to_str().unwrap();
        let execution = PaneExecContext::from_pty_options(&nebula_terminal::tty::Options {
            working_directory: Some(repository.path().to_owned()),
            ..Default::default()
        });
        let session = Session::default();
        let query = |line: &str, cancellation: &Cancellation| {
            session
                .request(
                    cwd.into(),
                    SuggestEnv::Local,
                    line.into(),
                    CompletionStyle::Popup,
                    Some(&execution),
                )
                .calculate(cancellation)
        };
        let active = Cancellation::default();
        assert_eq!(query("git switch feature/", &active).completion_items.len(), 2);
        crate::git_completion::tests::git(repository.path(), &["branch", "feature/beta"]);
        assert_eq!(query("git switch feature/", &active).completion_items.len(), 2);
        session.invalidate();
        assert_eq!(query("git switch feature/", &active).completion_items.len(), 3);

        let cancelled = Cancellation::default();
        cancelled.cancel();
        let result = query("git switch feature/", &cancelled);
        assert!(result.completion_items.is_empty());
        assert!(result.suggestion.is_empty());
        assert!(result.pending_remote_dir.is_none());
        assert_eq!(query("git switch feature/", &active).completion_items.len(), 3);
    }

    #[test]
    fn completion_request_routes_foreign_filesystems_without_host_candidates() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("request-only.txt"), b"").unwrap();
        let cwd = directory.path().to_str().unwrap();
        let session = Session::default();
        let query = |env| {
            session
                .request(cwd.into(), env, "cat request-only".into(), CompletionStyle::Popup, None)
                .calculate(&Cancellation::default())
        };
        assert!(query(SuggestEnv::Local).completion_items.iter().any(|item| item.insert == ".txt"));
        for env in [
            SuggestEnv::Wsl { distro: "completion-request-test".into() },
            SuggestEnv::Ssh { destination: "completion-request-test.invalid".into() },
            SuggestEnv::Shell {
                scope: HistoryScope::Ssh("nested-completion-request.invalid".into()),
            },
        ] {
            assert!(query(env).completion_items.is_empty());
        }
    }

    #[test]
    fn semantic_requests_share_modes_and_never_read_a_foreign_project() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("package.json"),
            r#"{"scripts":{"build:中文":"echo ok"}}"#,
        )
        .unwrap();
        let session = Session::default();
        let query = |env, line: &str, style| {
            session
                .request(directory.path().to_str().unwrap().into(), env, line.into(), style, None)
                .calculate(&Cancellation::default())
        };
        for style in [CompletionStyle::Inline, CompletionStyle::Popup, CompletionStyle::Hybrid] {
            for (line, insert) in
                [("npm run build:中", "文"), ("git sw", "itch"), ("git rebase --empty=k", "eep")]
            {
                let result = query(SuggestEnv::Local, line, style);
                if style == CompletionStyle::Popup {
                    assert_eq!(result.completion_items[0].insert, insert);
                } else {
                    assert_eq!(result.suggestion, insert);
                }
            }
        }
        for env in [
            SuggestEnv::Wsl { distro: "semantic-isolation".into() },
            SuggestEnv::Ssh { destination: "semantic-isolation.invalid".into() },
            SuggestEnv::Shell { scope: HistoryScope::Ssh("nested-semantic.invalid".into()) },
        ] {
            assert!(
                query(env.clone(), "npm run bu", CompletionStyle::Popup)
                    .completion_items
                    .is_empty()
            );
            assert_eq!(
                query(env, "git sw", CompletionStyle::Popup).completion_items[0].insert,
                "itch"
            );
        }
        assert!(
            query(SuggestEnv::Local, "git switch -c ", CompletionStyle::Popup)
                .completion_items
                .is_empty()
        );
    }

    #[test]
    fn semantic_history_only_promotes_candidates_in_the_current_project() {
        let directory = tempfile::tempdir().unwrap();
        let cwd = directory.path().to_str().unwrap();
        let manifest = directory.path().join("package.json");
        std::fs::write(
            &manifest,
            r#"{"scripts":{"semantic-rank-a":"echo a","semantic-rank-z":"echo z"}}"#,
        )
        .unwrap();
        let session = Session::default();
        record_command(&HistoryScope::Local, "npm run semantic-rank-z", cwd);
        let query = || {
            session
                .request(
                    cwd.into(),
                    SuggestEnv::Local,
                    "npm run semantic-rank-".into(),
                    CompletionStyle::Popup,
                    None,
                )
                .calculate(&Cancellation::default())
        };
        assert_eq!(query().completion_items[0].insert, "z");
        std::fs::write(&manifest, r#"{"scripts":{"semantic-rank-a":"echo a"}}"#).unwrap();
        session.invalidate();
        let result = query();
        assert_eq!(result.completion_items.len(), 1);
        assert_eq!(result.completion_items[0].insert, "a");
    }
}
