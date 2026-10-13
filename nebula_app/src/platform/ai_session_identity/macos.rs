//! Darwin 没有 /proc；只查询当前 pane 子进程已打开的 rollout，不按 cwd 猜会话。
use super::*;
use std::collections::HashSet;

#[cfg(target_os = "macos")]
pub(super) fn probe(shell_pid: u32) -> Option<CodexSession> {
    let candidates = || -> Option<HashSet<u32>> {
        Some(
            crate::process_tree::descendants(shell_pid)
                .ok()?
                .into_iter()
                .filter(|process| {
                    is_codex_process_name(process.executable.rsplit('/').next().unwrap_or(""))
                })
                .map(|process| process.pid)
                .collect(),
        )
    };
    let pids = candidates()?;
    if pids.is_empty() || pids.len() > MAX_PROBE_RECORDS {
        return None;
    }
    let list = pids.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
    let mut command = Command::new("/usr/sbin/lsof");
    command.args(["-n", "-P", "-a", "-p", &list, "-F0pfn"]);
    let output = run_probe_output(command)?;
    // 子进程可能在 lsof 执行期间退出；第二次快照禁止把过期的描述符归属用于恢复。
    let live = candidates()?;
    let mut records = String::new();
    let deadline = Instant::now() + PROBE_TIMEOUT;
    for (pid, fd, path) in open_rollouts(&output, &pids)? {
        if !live.contains(&pid) || Instant::now() >= deadline {
            return None;
        }
        let Ok(first) = read_first_line(Path::new(&path)) else { continue };
        use std::fmt::Write as _;
        let _ = writeln!(records, "{pid}\t{fd}\t{path}\t{first}");
        if records.len() > MAX_PROBE_OUTPUT {
            return None;
        }
    }
    parse_probe_context(&records)
}

fn open_rollouts(bytes: &[u8], allowed: &HashSet<u32>) -> Option<Vec<(u32, u32, String)>> {
    let text = std::str::from_utf8(bytes).ok()?;
    let (mut pid, mut fd) = (None, None);
    let mut files = Vec::new();
    // lsof -F0 以 NUL 分隔字段，进程/文件组之间另加一个换行。
    for field in text.split('\0').map(|field| field.trim_start_matches('\n')) {
        if let Some(value) = field.strip_prefix('p') {
            pid = value.parse::<u32>().ok().filter(|pid| allowed.contains(pid));
            fd = None;
        } else if let Some(value) = field.strip_prefix('f') {
            fd = value.parse::<u32>().ok();
        } else if let Some(path) = field.strip_prefix('n') {
            let (Some(pid), Some(fd)) = (pid, fd) else { continue };
            let file = Path::new(path);
            if !path.starts_with('/')
                || path.chars().any(char::is_control)
                || !file.components().any(|part| part.as_os_str() == "sessions")
                || rollout_id(file).is_none()
            {
                continue;
            }
            files.push((pid, fd, path.to_owned()));
            if files.len() > MAX_PROBE_RECORDS {
                return None;
            }
        }
    }
    Some(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lsof_fields_require_numeric_open_descriptors_and_owned_processes() {
        let path = "/Users/user/Codex Home/sessions/2026/09/29/rollout-x-01a079fa-4a9b-7d93-8a4a-4a7a9edaf247.jsonl";
        let raw = format!("p42\0\nf10\0n{path}\0\nf cwd\0n{path}\0\np99\0\nf11\0n{path}\0\n");
        assert_eq!(
            open_rollouts(raw.as_bytes(), &HashSet::from([42])).unwrap(),
            vec![(42, 10, path.into())]
        );
        assert!(open_rollouts(raw.as_bytes(), &HashSet::new()).unwrap().is_empty());
    }
}
