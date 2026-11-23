//! 已配对手机的 Git 请求仍由电脑解析窗格、确定工作目录并执行，不暴露任意命令。

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::{ApiError, ApiRequest, RuntimeCommand, parse_params};
use crate::git_worktree::status::{GitStatus, STATUS_ARGS, parse_status};
use crate::runtime_exec::PaneExecContext;

const LIMIT: usize = 256 * 1024;
const DEADLINE: Duration = Duration::from_secs(25);

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    window_id: u64,
    pane_id: u64,
    expected_cwd: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    staged: bool,
    #[serde(default)]
    all: bool,
    #[serde(default)]
    revision: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    commit: String,
    #[serde(default)]
    offset: usize,
    #[serde(skip)]
    operation: String,
}

pub(super) fn command(request: &ApiRequest) -> Result<RuntimeCommand, ApiError> {
    let mut params: Request = parse_params(&request.params)?;
    params.operation = request.method.strip_prefix("git.").unwrap_or_default().into();
    if !matches!(
        params.operation.as_str(),
        "status" | "diff" | "history" | "stage" | "unstage" | "commit" | "fetch" | "pull" | "push"
    ) {
        return Err(ApiError::new("method_not_found", "unknown Git operation"));
    }
    if params.window_id == 0
        || params.pane_id == 0
        || params.expected_cwd.is_empty()
        || params.expected_cwd.len() > 4096
        || params.expected_cwd.contains('\0')
    {
        return Err(ApiError::invalid_params(
            "an explicit pane and working directory are required",
        ));
    }
    if params.path.len() > 4096
        || params.path.contains('\0')
        || params.message.len() > 16 * 1024
        || params.message.contains('\0')
        || params.offset > 10_000
    {
        return Err(ApiError::invalid_params("Git parameter exceeds its limit"));
    }
    if !params.commit.is_empty()
        && (!matches!(params.commit.len(), 40 | 64)
            || !params.commit.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err(ApiError::invalid_params("commit must be a full object ID"));
    }
    if params.is_write()
        && (params.revision.len() != 64 || !params.revision.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err(ApiError::invalid_params("a current Git status revision is required"));
    }
    if params.operation == "commit" && params.message.trim().is_empty() {
        return Err(ApiError::invalid_params("commit message is empty"));
    }
    Ok(RuntimeCommand::Git {
        window_id: Some(params.window_id),
        pane_id: params.pane_id,
        request: params,
    })
}

impl Request {
    fn is_write(&self) -> bool {
        !matches!(self.operation.as_str(), "status" | "diff" | "history")
    }
}

struct Repository {
    context: PaneExecContext,
    cwd: String,
    root: String,
    started: Instant,
}

impl Repository {
    fn run(&self, args: &[&str], diff_exit: bool) -> Result<String, ApiError> {
        let remaining = DEADLINE.saturating_sub(self.started.elapsed()).as_millis() as u64;
        if remaining == 0 {
            return Err(ApiError::new(
                "git_outcome_unknown",
                "Git deadline reached; refresh before another operation",
            ));
        }
        let mut argv: Vec<String> = [
            "git",
            "--no-pager",
            "--literal-pathspecs",
            "--no-optional-locks",
            "-c",
            "credential.interactive=never",
            "-c",
            "core.askPass=",
            "-C",
            &self.root,
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        argv.extend(args.iter().map(|s| (*s).into()));
        let result = crate::runtime_exec::execute(
            self.context.clone(),
            self.cwd.clone(),
            argv,
            remaining,
            LIMIT,
        )?;
        if result["timed_out"] == true {
            return Err(ApiError::new(
                "git_outcome_unknown",
                "Git timed out; refresh before another operation",
            ));
        }
        if result["success"] != true && !(diff_exit && result["exit_code"] == 1) {
            return Err(ApiError::new("git_failed", "Git did not complete the operation")
                .details(json!({"stderr": result["stderr"], "exit_code": result["exit_code"]})));
        }
        if result["capture"]["stdout"]["truncated"] == true {
            return Err(ApiError::new("git_too_large", "Git output exceeds the mobile view limit"));
        }
        if result["capture"]["stdout"]["encoding"] != "utf-8" {
            return Err(ApiError::new("git_encoding", "Git output is not UTF-8"));
        }
        Ok(result["stdout"].as_str().unwrap_or_default().into())
    }

    fn status(&self) -> Result<(GitStatus, String), ApiError> {
        let mut args = STATUS_ARGS.to_vec();
        args.push("--untracked-files=all");
        let raw = self.run(&args, false)?;
        let status = parse_status(&raw)
            .map_err(|code| ApiError::new(code, "invalid or oversized Git status"))?;
        if status.entries.len() > 8_000 {
            return Err(ApiError::new("git_too_large", "too many changes for the mobile view"));
        }
        // 同名文件被重新暂存也会改变 blob ID，提交前据此拒绝旧页面里的整仓库操作。
        let index = self.run(
            &["diff", "--cached", "--raw", "--no-abbrev", "--no-ext-diff", "--no-textconv", "-z"],
            false,
        )?;
        let mut digest = Sha256::new();
        digest.update(self.root.as_bytes());
        digest.update([0]);
        digest.update(raw.as_bytes());
        digest.update(index.as_bytes());
        let revision: String = digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
        Ok((status, revision))
    }

    fn diff(&self, request: &Request) -> Result<Value, ApiError> {
        let mut args = vec!["diff", "--no-ext-diff", "--no-textconv", "--no-color"];
        let mut no_index = false;
        if !request.commit.is_empty() {
            args = vec![
                "show",
                "--format=",
                "--first-parent",
                "--no-ext-diff",
                "--no-textconv",
                "--no-color",
                &request.commit,
                "--",
            ];
        } else {
            let (status, _) = self.status()?;
            let entry =
                status.entries.iter().find(|e| e.path == request.path).ok_or_else(|| {
                    ApiError::new("git_stale", "the selected change no longer exists")
                })?;
            if entry.index == '?' {
                no_index = true;
                args.extend(["--no-index", "--", "/dev/null", &request.path]);
            } else {
                if request.staged {
                    args.push("--cached");
                }
                args.push("--");
                args.push(&request.path);
                if let Some(original) = &entry.original {
                    args.push(original);
                }
            }
            return Ok(json!({"text": self.run(&args, no_index)?}));
        }
        Ok(json!({"text": self.run(&args, no_index)?}))
    }

    fn mutate(&self, request: &Request, status: &GitStatus) -> Result<Value, ApiError> {
        let mut args = match request.operation.as_str() {
            "stage" => vec!["add"],
            "unstage" if status.head == "(initial)" => vec!["rm", "--cached", "-r"],
            "unstage" => vec!["restore", "--staged"],
            "commit" => {
                if status.entries.iter().any(|e| e.conflict)
                    || !status.entries.iter().any(|e| e.staged())
                {
                    return Err(ApiError::new(
                        "git_not_committable",
                        "resolve conflicts and stage changes before committing",
                    ));
                }
                vec!["commit", "-m", &request.message]
            },
            "fetch" => vec!["fetch"],
            "pull" => vec!["pull", "--ff-only"],
            "push" => vec!["push"],
            _ => return Err(ApiError::invalid_params("unknown Git mutation")),
        };
        if matches!(request.operation.as_str(), "stage" | "unstage") {
            if request.all {
                args.extend(["--", "."]);
            } else {
                let entry =
                    status.entries.iter().find(|e| e.path == request.path).ok_or_else(|| {
                        ApiError::new("git_stale", "the selected change no longer exists")
                    })?;
                args.extend(["--", &entry.path]);
                if request.operation == "unstage" {
                    if let Some(original) = &entry.original {
                        args.push(original);
                    }
                }
            }
        }
        let output = self.run(&args, false)?;
        // 写入回执与刷新分开；随后刷新失败不能把已经成功的提交变成“可重试”。
        Ok(json!({"success": true, "output": output}))
    }
}

fn repository_lock(key: String) -> Arc<Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<String, Weak<Mutex<()>>>>> = OnceLock::new();
    let mut locks = LOCKS.get_or_init(Mutex::default).lock().unwrap_or_else(|e| e.into_inner());
    locks.retain(|_, value| value.strong_count() > 0);
    if let Some(lock) = locks.get(&key).and_then(Weak::upgrade) {
        return lock;
    }
    let lock = Arc::new(Mutex::new(()));
    locks.insert(key, Arc::downgrade(&lock));
    lock
}

pub(crate) fn execute(
    context: PaneExecContext,
    cwd: String,
    request: &Request,
) -> Result<Value, ApiError> {
    if cwd != request.expected_cwd {
        return Err(ApiError::new(
            "git_target_changed",
            "the pane working directory changed; reopen Git",
        ));
    }
    let mut repo =
        Repository { context: context.for_git(), root: cwd.clone(), cwd, started: Instant::now() };
    let root = repo.run(&["rev-parse", "--show-toplevel"], false)?;
    repo.root = root.strip_suffix('\n').unwrap_or(&root).into();
    let lock = repository_lock(format!(
        "{:?}:{:?}:{}",
        repo.context.wsl_distribution(),
        repo.context.wsl_user(),
        repo.root
    ));
    let _guard = lock.try_lock().map_err(|_| {
        ApiError::new("git_busy", "another Git operation is running for this repository")
    })?;
    if request.operation == "diff" {
        return repo.diff(request);
    }
    let (status, revision) = repo.status()?;
    if request.operation == "status" {
        return Ok(json!({"root": repo.root, "status": status, "revision": revision}));
    }
    if request.operation == "history" {
        if status.head == "(initial)" {
            return Ok(json!({"commits": [], "has_more": false}));
        }
        let skip = format!("--skip={}", request.offset);
        let output = repo.run(
            &[
                "log",
                "--all",
                "--date-order",
                "--decorate=full",
                "--max-count=51",
                &skip,
                "--format=%x1e%H%x1f%h%x1f%D%x1f%s%x1f%an%x1f%ct%x1f%P",
            ],
            false,
        )?;
        let commits = crate::display::side_panel::parse_git_history(&output);
        let values: Vec<_> = commits.iter().take(50).map(|c| json!({"hash": c.full_hash, "short_hash": c.short_hash,
            "subject": c.subject, "author": c.author, "timestamp": c.timestamp, "parents": c.parent_hashes, "refs": c.decorations})).collect();
        return Ok(json!({"commits": values, "has_more": commits.len() > 50}));
    }
    if revision != request.revision {
        return Err(ApiError::new("git_stale", "repository state changed; refresh before writing"));
    }
    repo.mutate(request, &status)
}

#[cfg(test)]
mod tests;
