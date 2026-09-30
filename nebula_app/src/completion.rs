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
}

impl Session {
    pub(crate) fn invalidate(&self) {
        self.git.invalidate();
    }

    pub(crate) fn request(
        &self,
        cwd: String,
        env: SuggestEnv,
        line: String,
        style: CompletionStyle,
        execution: Option<&PaneExecContext>,
    ) -> Request {
        // 普通按键不复制启动环境；远端输入也不能获得宿主 Git 查询能力。
        let git = if env.is_this_machine()
            && matches!(line.split_whitespace().next(), Some("git" | "git.exe"))
        {
            execution.cloned().map(|execution| (self.git.clone(), execution))
        } else {
            None
        };
        Request { cwd, env, line, style, git }
    }
}

/// 后台只收到当前输入与已确认的执行环境，不借用整个终端状态。
pub(crate) struct Request {
    cwd: String,
    env: SuggestEnv,
    line: String,
    style: CompletionStyle,
    git: Option<(Arc<crate::git_completion::Cache>, PaneExecContext)>,
}

impl Request {
    pub(crate) fn calculate(self, cancellation: &Cancellation) -> Candidates {
        if cancellation.is_cancelled() {
            return Candidates::default();
        }
        let semantic = self.git.and_then(|(cache, execution)| {
            use nebula_completions::command_context::ShellSyntax;
            let syntax = match execution.shell_program() {
                Some(program) => ShellSyntax::for_program(program),
                None => ShellSyntax::for_program(&crate::platform::shell::default_shell_id()),
            };
            crate::git_completion::complete(
                &cache,
                &execution,
                &self.cwd,
                &self.line,
                syntax,
                &|| cancellation.is_cancelled(),
            )
        });
        if cancellation.is_cancelled() {
            return Candidates::default();
        }
        if let Some(candidates) = semantic {
            return suggest_engine::semantic_candidates(&self.line, self.style, candidates);
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
}
