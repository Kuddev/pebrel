//! 只发现字面 Host 别名；不运行 ssh -G，避免 Match exec 在补齐时执行用户命令。

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub(crate) fn discover(
    config: &Path,
    include_root: &Path,
    home: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Vec<String> {
    let mut scan = Scan {
        include_root,
        home,
        cancelled,
        started: Instant::now(),
        bytes: 1024 * 1024,
        entries: 4096,
        visited: HashSet::new(),
        hosts: Vec::new(),
    };
    scan.file(config, 0);
    if cancelled() { Vec::new() } else { scan.hosts }
}

struct Scan<'a> {
    include_root: &'a Path,
    home: &'a Path,
    cancelled: &'a dyn Fn() -> bool,
    started: Instant,
    bytes: usize,
    entries: usize,
    visited: HashSet<PathBuf>,
    hosts: Vec<String>,
}

impl Scan<'_> {
    fn stopped(&self) -> bool {
        (self.cancelled)()
            || self.started.elapsed() >= Duration::from_millis(250)
            || self.hosts.len() >= 1024
    }

    fn file(&mut self, path: &Path, depth: usize) {
        if self.stopped() || self.bytes == 0 || depth > 8 || self.visited.len() >= 64 {
            return;
        }
        let Ok(path) = path.canonicalize() else { return };
        if !self.visited.insert(path.clone()) {
            return;
        }
        let Ok(metadata) = path.metadata() else { return };
        if !metadata.is_file() || metadata.len() > self.bytes as u64 {
            return;
        }
        let Ok(file) = std::fs::File::open(path) else { return };
        let mut bytes = Vec::new();
        if file.take(self.bytes as u64 + 1).read_to_end(&mut bytes).is_err() {
            return;
        }
        let valid_size = bytes.len() <= self.bytes;
        self.bytes = self.bytes.saturating_sub(bytes.len());
        if !valid_size {
            return;
        }
        let Ok(text) = String::from_utf8(bytes) else { return };
        for line in text.lines() {
            if self.stopped() {
                return;
            }
            let Ok(tokens) = super::ssh_config_tokens_checked(line) else { continue };
            let Some(keyword) = tokens.first() else { continue };
            if keyword.eq_ignore_ascii_case("host") {
                for host in super::parse_ssh_config_hosts(line) {
                    if !self.hosts.contains(&host) {
                        self.hosts.push(host);
                    }
                    if self.stopped() {
                        return;
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
                    let path = pattern.strip_prefix("~/").map_or_else(
                        || self.include_root.join(pattern),
                        |suffix| self.home.join(suffix),
                    );
                    for path in self.expand(&path) {
                        self.file(&path, depth + 1);
                    }
                }
            }
        }
    }

    fn expand(&mut self, pattern: &Path) -> Vec<PathBuf> {
        let mut paths = vec![PathBuf::new()];
        for component in pattern.components() {
            if self.stopped() {
                return Vec::new();
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
            let Ok(pattern) = glob::Pattern::new(&text) else { return Vec::new() };
            let mut matches = Vec::new();
            for directory in paths {
                let Ok(entries) = std::fs::read_dir(directory) else { continue };
                for entry in entries {
                    if self.stopped() || self.entries == 0 {
                        return Vec::new();
                    }
                    self.entries -= 1;
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
        paths
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
    }
}
