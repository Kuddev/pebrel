//! 只发现字面 Host 别名；不运行 ssh -G，避免 Match exec 在补齐时执行用户命令。

use std::collections::HashSet;
use std::hash::Hash;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

mod wsl;
pub(crate) use wsl::{GuestHosts, GuestScope, WslConfig, discover_wsl};

pub(crate) fn discover(
    config: &Path,
    include_root: &Path,
    home: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Vec<String> {
    scan(
        &mut LocalFiles { include_root, home },
        config.to_path_buf(),
        Duration::from_millis(250),
        cancelled,
    )
    .map_or_else(|_| Vec::new(), |result| result.hosts)
}

/// 解析顺序、循环检测和总预算只有一个实现；后端仅负责各自路径空间里的读取与 glob。
trait ConfigFiles {
    type Path: Clone + Eq + Hash;

    fn canonical(
        &mut self,
        path: &Self::Path,
        budget: &Budget<'_>,
    ) -> io::Result<Option<Self::Path>>;
    fn read(&mut self, path: &Self::Path, budget: &mut Budget<'_>) -> io::Result<Option<String>>;
    fn expand(&mut self, pattern: &str, budget: &mut Budget<'_>) -> io::Result<Vec<Self::Path>>;
}

struct Budget<'a> {
    cancelled: &'a dyn Fn() -> bool,
    deadline: Instant,
    bytes: usize,
    entries: usize,
    limited: bool,
}

impl Budget<'_> {
    fn stopped(&self) -> bool {
        (self.cancelled)() || Instant::now() >= self.deadline
    }

    fn charge_bytes(&mut self, count: usize) -> bool {
        let accepted = count <= self.bytes;
        self.bytes = self.bytes.saturating_sub(count);
        self.limited |= !accepted;
        accepted
    }
}

struct Discovery {
    hosts: Vec<String>,
    limited: bool,
}

fn scan<F: ConfigFiles>(
    files: &mut F,
    config: F::Path,
    timeout: Duration,
    cancelled: &dyn Fn() -> bool,
) -> io::Result<Discovery> {
    let mut scan = Scan {
        files,
        budget: Budget {
            cancelled,
            deadline: Instant::now() + timeout,
            bytes: 1024 * 1024,
            entries: 4096,
            limited: false,
        },
        visited: HashSet::new(),
        hosts: Vec::new(),
    };
    scan.file(&config, 0)?;
    if cancelled() {
        return Err(io::ErrorKind::Interrupted.into());
    }
    Ok(Discovery {
        limited: scan.budget.limited || scan.budget.stopped() || scan.hosts.len() >= 1024,
        hosts: scan.hosts,
    })
}

struct Scan<'a, F: ConfigFiles> {
    files: &'a mut F,
    budget: Budget<'a>,
    visited: HashSet<F::Path>,
    hosts: Vec<String>,
}

impl<F: ConfigFiles> Scan<'_, F> {
    fn stopped(&self) -> bool {
        self.budget.stopped() || self.hosts.len() >= 1024
    }

    fn file(&mut self, path: &F::Path, depth: usize) -> io::Result<()> {
        if self.stopped() || self.budget.bytes == 0 || depth > 8 || self.visited.len() >= 64 {
            self.budget.limited = true;
            return Ok(());
        }
        let Some(path) = self.files.canonical(path, &self.budget)? else { return Ok(()) };
        if !self.visited.insert(path.clone()) {
            return Ok(());
        }
        let Some(text) = self.files.read(&path, &mut self.budget)? else { return Ok(()) };
        for line in text.lines() {
            if self.stopped() {
                return Ok(());
            }
            let Ok(tokens) = super::ssh_config_tokens_checked(line) else { continue };
            let Some(keyword) = tokens.first() else { continue };
            if keyword.eq_ignore_ascii_case("host") {
                for host in super::parse_ssh_config_hosts(line) {
                    if !self.hosts.contains(&host) {
                        self.hosts.push(host);
                    }
                    if self.stopped() {
                        return Ok(());
                    }
                }
            } else if keyword.eq_ignore_ascii_case("include") {
                for pattern in &tokens[1..] {
                    // %h/环境变量/其他用户的 ~ 依赖运行时上下文，不能猜测展开结果。
                    if pattern.contains(['%', '$'])
                        || pattern.starts_with('~') && !pattern.starts_with("~/")
                    {
                        continue;
                    }
                    for path in self.files.expand(pattern, &mut self.budget)? {
                        self.file(&path, depth + 1)?;
                    }
                }
            }
        }
        Ok(())
    }
}

struct LocalFiles<'a> {
    include_root: &'a Path,
    home: &'a Path,
}

impl ConfigFiles for LocalFiles<'_> {
    type Path = PathBuf;

    fn canonical(&mut self, path: &PathBuf, _: &Budget<'_>) -> io::Result<Option<PathBuf>> {
        Ok(path.canonicalize().ok())
    }

    fn read(&mut self, path: &PathBuf, budget: &mut Budget<'_>) -> io::Result<Option<String>> {
        let Ok(metadata) = path.metadata() else { return Ok(None) };
        if !metadata.is_file() {
            return Ok(None);
        }
        if metadata.len() > budget.bytes as u64 {
            budget.limited = true;
            return Ok(None);
        }
        let Ok(file) = std::fs::File::open(path) else { return Ok(None) };
        let mut bytes = Vec::new();
        if file.take(budget.bytes as u64 + 1).read_to_end(&mut bytes).is_err() {
            return Ok(None);
        }
        if !budget.charge_bytes(bytes.len()) {
            return Ok(None);
        }
        Ok(String::from_utf8(bytes).ok())
    }

    fn expand(&mut self, pattern: &str, budget: &mut Budget<'_>) -> io::Result<Vec<PathBuf>> {
        let pattern = pattern
            .strip_prefix("~/")
            .map_or_else(|| self.include_root.join(pattern), |suffix| self.home.join(suffix));
        let mut paths = vec![PathBuf::new()];
        for component in pattern.components() {
            if budget.stopped() {
                return Ok(Vec::new());
            }
            let part = component.as_os_str();
            let text = part.to_string_lossy();
            if !matches!(component, std::path::Component::Normal(_))
                || !text.contains(['*', '?', '['])
            {
                for path in &mut paths {
                    path.push(part);
                }
                continue;
            }
            let Ok(pattern) = glob::Pattern::new(&text) else { return Ok(Vec::new()) };
            let mut matches = Vec::new();
            for directory in paths {
                let Ok(entries) = std::fs::read_dir(directory) else { continue };
                for entry in entries {
                    if budget.stopped() || budget.entries == 0 {
                        budget.limited = true;
                        return Ok(Vec::new());
                    }
                    budget.entries -= 1;
                    let Ok(entry) = entry else { continue };
                    if pattern.matches_with(
                        &entry.file_name().to_string_lossy(),
                        glob::MatchOptions {
                            case_sensitive: crate::platform::local_paths::completion_case_sensitive(
                            ),
                            require_literal_separator: true,
                            require_literal_leading_dot: true,
                        },
                    ) {
                        matches.push(entry.path());
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
    fn ssh_completion_includes_are_bounded_ordered_and_never_executed() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path();
        let ssh = home.join(".ssh");
        std::fs::create_dir_all(ssh.join("parts")).unwrap();
        let config = ssh.join("config");
        std::fs::write(
            &config,
            "Host primary\nInclude parts/*.conf\nMatch exec \"touch must-not-run\"\nHost final\n",
        )
        .unwrap();
        std::fs::write(
            ssh.join("parts/a.conf"),
            "Host alpha\nInclude config\nHost * !no wildcard?\n",
        )
        .unwrap();
        std::fs::write(ssh.join("parts/b.conf"), "Host beta alpha\n").unwrap();
        assert_eq!(discover(&config, &ssh, home, &|| false), ["primary", "alpha", "beta", "final"]);
        assert!(!home.join("must-not-run").exists());
        assert!(discover(&config, &ssh, home, &|| true).is_empty());
        std::fs::write(&config, vec![b'x'; 1024 * 1024 + 1]).unwrap();
        assert!(discover(&config, &ssh, home, &|| false).is_empty());
        std::fs::write(&config, b"Host bad\xff").unwrap();
        assert!(discover(&config, &ssh, home, &|| false).is_empty());

        std::fs::create_dir_all(ssh.join("含 空格/nested")).unwrap();
        std::fs::write(&config, "Host first\nInclude \"含 空格/a.conf\"\nHost last\n").unwrap();
        std::fs::write(
            ssh.join("含 空格/a.conf"),
            "Host included\nInclude \"含 空格/nested/*.conf\"\nHost after-nested\n",
        )
        .unwrap();
        std::fs::write(ssh.join("含 空格/nested/b.conf"), "Host deep\nInclude config\n").unwrap();
        assert_eq!(
            discover(&config, &ssh, home, &|| false),
            ["first", "included", "deep", "after-nested", "last"]
        );
    }
}
