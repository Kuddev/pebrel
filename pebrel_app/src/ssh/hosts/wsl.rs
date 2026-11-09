//! Guest 文件路径保持 POSIX 语义；同一 Rust 扫描器拥有 Host/Include 顺序与总预算。
use super::{Budget, ConfigFiles, scan};
use std::io;
use std::process::Command;
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_secs(10);
const READER: &[u8] = include_bytes!("../../../res/shell/ssh-config-reader.py");

#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct GuestScope {
    pub distro: String,
    pub user: String,
}

pub(crate) struct WslConfig<'a> {
    pub distro: &'a str,
    pub user: Option<&'a str>,
    pub config: Option<&'a str>,
    pub expand_home: bool,
    pub cwd: &'a str,
}

pub(crate) struct GuestHosts {
    pub scope: GuestScope,
    pub hosts: Vec<String>,
    pub limited: bool,
}

#[derive(serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Reply {
    Identity { user: String, home: String },
    Path { path: Option<String> },
    Text { text: Option<String> },
    Entries { names: Vec<String> },
    Limit,
    Error { code: String },
}

fn valid(value: &str, limit: usize) -> bool {
    !value.is_empty() && value.len() <= limit && !value.chars().any(char::is_control)
}

fn absolute(path: &str) -> bool {
    path.starts_with('/') && valid(path, 4096)
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "Invalid guest SSH configuration response")
}

fn query(
    mut command: Command,
    operation: &str,
    path: &str,
    limit: usize,
    budget: &Budget<'_>,
) -> io::Result<Reply> {
    let remaining = budget.deadline.saturating_duration_since(Instant::now());
    if (budget.cancelled)() {
        return Err(io::ErrorKind::Interrupted.into());
    }
    if remaining.is_zero() {
        return Err(io::ErrorKind::TimedOut.into());
    }
    // -I -S 避免用户 sitecustomize/PYTHONPATH 在“只读配置”探测时执行额外代码。
    command.args(["python3", "-I", "-S", "-", operation, path]);
    command.arg(limit.to_string()).arg(remaining.as_millis().clamp(1, 10000).to_string());
    let output_limit = match operation {
        "read" => limit.saturating_mul(6).saturating_add(1024),
        "list" => limit.saturating_mul(1536).saturating_add(1024),
        _ => 32 * 1024,
    };
    let bytes = crate::platform::process_output::read_with_input(
        command,
        READER,
        remaining,
        output_limit,
        budget.cancelled,
    )?;
    let reply: Reply = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    if let Reply::Error { code } = reply {
        let kind = match code.as_str() {
            "timeout" => io::ErrorKind::TimedOut,
            "permission" => io::ErrorKind::PermissionDenied,
            "encoding" => io::ErrorKind::InvalidData,
            _ => io::ErrorKind::Other,
        };
        return Err(io::Error::new(kind, "Guest SSH configuration could not be read"));
    }
    Ok(reply)
}

pub(crate) fn discover_wsl(
    request: WslConfig<'_>,
    cancelled: &dyn Fn() -> bool,
) -> io::Result<GuestHosts> {
    if crate::platform::Platform::current() != crate::platform::Platform::Windows {
        return Err(io::ErrorKind::Unsupported.into());
    }
    if !valid(request.distro, 256) || request.user.is_some_and(|user| !valid(user, 256)) {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let budget = Budget {
        cancelled,
        deadline: Instant::now() + TIMEOUT,
        bytes: 1024 * 1024,
        entries: 4096,
        limited: false,
    };
    // 后台发现只接受此刻仍运行的发行版；不能为列出别名而唤醒已停止的 guest。
    if !crate::platform::shell::running_wsl_distros(TIMEOUT, cancelled)?
        .iter()
        .any(|distro| distro == request.distro)
    {
        return Err(io::ErrorKind::NotConnected.into());
    }
    let command = crate::shell_detect::wsl_exec_command(request.distro, request.user);
    let Reply::Identity { user, home } = query(command, "identity", "", 0, &budget)? else {
        return Err(invalid());
    };
    if !valid(&user, 256) || !absolute(&home) {
        return Err(invalid());
    }
    let scope = GuestScope { distro: request.distro.to_owned(), user };
    let config = match request.config {
        None => join(&join(&home, ".ssh"), "config"),
        Some(path) if request.expand_home && path.starts_with("~/") => join(&home, &path[2..]),
        Some(path) if absolute(path) => path.to_owned(),
        Some(path) if valid(path, 4096) && absolute(request.cwd) => join(request.cwd, path),
        _ => return Err(io::ErrorKind::InvalidInput.into()),
    };
    if !absolute(&config) {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let mut files = GuestFiles { scope: scope.clone(), home };
    let result = scan(
        &mut files,
        config,
        budget.deadline.saturating_duration_since(Instant::now()),
        cancelled,
    )?;
    Ok(GuestHosts { scope, hosts: result.hosts, limited: result.limited })
}

fn join(base: &str, name: &str) -> String {
    format!("{}/{}", base.trim_end_matches('/'), name)
}

struct GuestFiles {
    scope: GuestScope,
    home: String,
}

impl GuestFiles {
    fn request(
        &self,
        operation: &str,
        path: &str,
        limit: usize,
        budget: &Budget<'_>,
    ) -> io::Result<Reply> {
        if !absolute(path) {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        query(
            crate::shell_detect::wsl_exec_command(&self.scope.distro, Some(&self.scope.user)),
            operation,
            path,
            limit,
            budget,
        )
    }
}

impl ConfigFiles for GuestFiles {
    type Path = String;

    fn canonical(&mut self, path: &String, budget: &Budget<'_>) -> io::Result<Option<String>> {
        let Reply::Path { path } = self.request("canonical", path, 0, budget)? else {
            return Err(invalid());
        };
        if path.as_ref().is_some_and(|path| !absolute(path)) {
            return Err(invalid());
        }
        Ok(path)
    }

    fn read(&mut self, path: &String, budget: &mut Budget<'_>) -> io::Result<Option<String>> {
        match self.request("read", path, budget.bytes, budget)? {
            Reply::Text { text } => {
                if let Some(text) = &text
                    && !budget.charge_bytes(text.len())
                {
                    return Ok(None);
                }
                Ok(text)
            },
            Reply::Limit => {
                budget.bytes = 0;
                budget.limited = true;
                Ok(None)
            },
            _ => Err(invalid()),
        }
    }

    fn expand(&mut self, pattern: &str, budget: &mut Budget<'_>) -> io::Result<Vec<String>> {
        let path = if let Some(path) = pattern.strip_prefix("~/") {
            join(&self.home, path)
        } else if pattern.starts_with('/') {
            pattern.to_owned()
        } else {
            join(&join(&self.home, ".ssh"), pattern)
        };
        if !absolute(&path) {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let mut paths = vec!["/".to_owned()];
        for part in path.split('/').filter(|part| !part.is_empty()) {
            if budget.stopped() {
                return Ok(Vec::new());
            }
            if !part.contains(['*', '?', '[']) {
                paths = paths.into_iter().map(|path| join(&path, part)).collect();
                continue;
            }
            let Ok(pattern) = glob::Pattern::new(part) else { return Ok(Vec::new()) };
            let mut matches = Vec::new();
            for directory in paths {
                if budget.entries == 0 {
                    budget.limited = true;
                    return Ok(Vec::new());
                }
                let names = match self.request("list", &directory, budget.entries, budget)? {
                    Reply::Entries { names } if names.len() <= budget.entries => names,
                    Reply::Limit => {
                        budget.entries = 0;
                        budget.limited = true;
                        return Ok(Vec::new());
                    },
                    _ => return Err(invalid()),
                };
                budget.entries -= names.len();
                for name in names {
                    if !valid(&name, 255)
                        || name.contains('/')
                        || matches!(name.as_str(), "." | "..")
                    {
                        return Err(invalid());
                    }
                    if pattern.matches_with(
                        &name,
                        glob::MatchOptions {
                            case_sensitive: true,
                            require_literal_separator: true,
                            require_literal_leading_dot: true,
                        },
                    ) {
                        matches.push(join(&directory, &name));
                    }
                }
            }
            matches.sort();
            paths = matches;
        }
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guest_reader_preserves_literal_utf8_and_bounds_real_files() {
        // 探针在 Linux guest 内运行；Windows 宿主 Python 不提供 pwd/SIGALRM。
        if crate::platform::Platform::current() == crate::platform::Platform::Windows {
            return;
        }
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("含 空格;not-a-command.conf");
        let source =
            "Host production\nInclude \"parts dir/*.conf\"\nMatch exec \"touch must-not-run\"\n";
        std::fs::write(&path, source).unwrap();
        let path = path.to_str().unwrap();
        let budget = Budget {
            cancelled: &|| false,
            deadline: Instant::now() + TIMEOUT,
            bytes: 1024,
            entries: 10,
            limited: false,
        };
        let Reply::Text { text } = query(Command::new("env"), "read", path, 1024, &budget).unwrap()
        else {
            panic!("file response")
        };
        assert_eq!(text.as_deref(), Some(source));
        assert!(matches!(
            query(Command::new("env"), "read", path, 1, &budget).unwrap(),
            Reply::Limit
        ));
        let Reply::Path { path: canonical } =
            query(Command::new("env"), "canonical", path, 0, &budget).unwrap()
        else {
            panic!("path response")
        };
        assert_eq!(
            canonical.as_deref(),
            std::path::Path::new(path).canonicalize().unwrap().to_str()
        );
        assert!(matches!(
            query(Command::new("env"), "list", directory.path().to_str().unwrap(), 0, &budget)
                .unwrap(),
            Reply::Limit
        ));
        assert!(!directory.path().join("must-not-run").exists());
        let cancelled = Budget { cancelled: &|| true, ..budget };
        assert_eq!(
            query(Command::new("env"), "read", path, 1024, &cancelled).err().unwrap().kind(),
            io::ErrorKind::Interrupted
        );
    }
}
