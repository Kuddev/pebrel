//! Find the current Codex conversation when no session ID was reported by its hook.
//!
//! Codex keeps the active rollout open, and its process environment carries
//! the pane ID supplied by the PTY launcher. This links a pane to its rollout
//! without relying on working-directory matches or file modification
//! time. Only the first metadata record is read, without conversation content.
//! The lookup is bounded and is only called by a background
//! task; UI code must never scan `/proc` or start `wsl.exe` synchronously.

use std::io::{self, Read, Seek as _};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[cfg(any(target_os = "macos", test))]
mod macos;
#[cfg(windows)]
mod windows;

/// Native probe helpers run before GUI, hooks or CLI setup.
pub(crate) fn run_helper_if_requested() -> Option<i32> {
    #[cfg(windows)]
    return windows::run_helper_if_requested();
    #[cfg(not(windows))]
    None
}

/// Keep owned copies of the application executable stopped during installation.
/// Acquisition waits on the background executor; cancellation only releases a gate.
pub(crate) struct UpdatePause {
    #[cfg(windows)]
    inner: windows::UpdatePause,
}

impl UpdatePause {
    pub(crate) fn acquire() -> io::Result<Self> {
        Ok(Self {
            #[cfg(windows)]
            inner: windows::UpdatePause::acquire()?,
        })
    }

    /// A committed handoff must keep probes stopped even when its owner is dropped.
    pub(crate) fn commit(&mut self) {
        #[cfg(windows)]
        self.inner.commit();
    }
}

const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_PROBE_OUTPUT: usize = 64 * 1024;
const MAX_PROBE_LINE: usize = 64 * 1024;
const MAX_PROBE_RECORDS: usize = 128;

const PROBE_SCRIPT: &str = r###"
pane="$1"
instance="$2"
for proc in /proc/[0-9]*; do
    pid=${proc##*/}
    IFS= read -r comm < "$proc/comm" 2>/dev/null || continue
    case "$comm" in
        codex|codex.exe|codex-*) ;;
        *) continue ;;
    esac
    [ -r "$proc/environ" ] || continue
    environment=$(tr "\0" "\n" < "$proc/environ" 2>/dev/null || true)
    daemon=0
    if tr "\0" "\n" < "$proc/cmdline" 2>/dev/null | sed 1d | grep -qx "app-server"; then
        daemon=1
    fi
    pane_id=$(printf '%s\n' "$environment" | sed -n "s/^PEBREL_PANE_ID=//p" | head -n 1)
    if [ "$pane_id" != "$pane" ]; then
        pane_id=$(printf '%s\n' "$environment" | sed -n "s/^NEBULA_PANE_ID=//p" | head -n 1)
    fi
    process_id=$(printf '%s\n' "$environment" | sed -n "s/^PEBREL_PROCESS_ID=//p" | head -n 1)
    owned=0
    if [ "$pane_id" = "$pane" ] && [ "$process_id" = "$instance" ]; then
        owned=1
    fi
    # Another pane's or terminal's client can share this pane's app-server.
    [ "$owned" = 1 ] || [ "$daemon" = 0 ] || continue
    codex_root=$(printf '%s\n' "$environment" | sed -n "s/^CODEX_HOME=//p" | head -n 1)
    if [ -z "$codex_root" ]; then
        guest_home=$(printf '%s\n' "$environment" | sed -n "s/^HOME=//p" | head -n 1)
        [ -n "$guest_home" ] || continue
        codex_root="$guest_home/.codex"
    fi
    case "$codex_root" in
        /*) ;;
        *) codex_root="$proc/cwd/$codex_root" ;;
    esac
    # fd 返回真实路径；每个已匹配进程只解析一次目录，避免逐句柄重复 I/O。
    sessions_root=$(CDPATH= cd -P "$codex_root/sessions" 2>/dev/null && pwd -P) || continue
    if [ "$owned" = 0 ]; then
        printf 'foreign\t%s\n' "$sessions_root"
        continue
    fi
    if [ "$daemon" = 1 ]; then
        printf 'daemon\t%s\t%s\n' "$pid" "$sessions_root"
    fi
    for fd in "$proc"/fd/*; do
        link=$(readlink "$fd" 2>/dev/null || true)
        case "$link" in
            "$sessions_root"/*/rollout-*.jsonl)
                first=$(head -c 65536 "$fd" 2>/dev/null | head -n 1)
                [ -n "$first" ] || continue
                printf '%s\t%s\t%s\t%s\n' "$pid" "${fd##*/}" "$link" "$first"
                ;;
        esac
    done
done
"###;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProbeRecord {
    pid: u32,
    _fd: u32,
    link: String,
    first_line: String,
}

/// Identity and working directory from the active rollout's own metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodexSession {
    pub session_id: String,
    pub session_file: String,
    pub cwd: Option<String>,
}

/// Probe the active Codex rollout for one pane.
///
/// WSL panes are inspected inside the guest because Windows Toolhelp cannot
/// see Linux processes or their open session files. Linux hosts use the same
/// process metadata directly. Windows inspects only the pane shell's verified
/// descendants in one reusable, process-owned helper. Each request remains bounded;
/// a stalled helper is terminated before a later request can replace it.
pub(crate) fn probe_codex_session(
    pane_id: u64,
    exec_context: Option<&crate::runtime_exec::PaneExecContext>,
    shell_pid: Option<u32>,
) -> Option<CodexSession> {
    let pane_id = pane_id.to_string();
    let context = exec_context?;
    let instance = context.process_instance()?;
    if let Some(distro) = context.wsl_distribution() {
        return probe_wsl(distro, context.wsl_user(), &pane_id, instance);
    }

    #[cfg(target_os = "linux")]
    {
        let _ = shell_pid;
        return probe_local_proc(&pane_id, instance);
    }
    #[cfg(target_os = "macos")]
    {
        let _ = (pane_id, instance);
        return macos::probe(shell_pid?);
    }
    #[cfg(windows)]
    {
        let _ = (pane_id, instance);
        windows::probe(shell_pid?)
    }
}

/// Whether a probe can recognize Codex's shared app-server holder. That daemon
/// writes `source=vscode` metadata for TUI threads and serves every client that
/// uses the same Codex home, so its environment alone does not identify a pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppServerThreads {
    /// The probe cannot tell the shared daemon from the pane's own process.
    Rejected,
    /// The probe reported daemon holders and clients outside the pane.
    Attributed,
}

fn parse_probe_context(output: &str) -> Option<CodexSession> {
    parse_probe_output(output, AppServerThreads::Rejected)
}

fn parse_attributed_probe(output: &str) -> Option<CodexSession> {
    parse_probe_output(output, AppServerThreads::Attributed)
}

fn parse_probe_output(output: &str, app_server: AppServerThreads) -> Option<CodexSession> {
    // `daemon\t<pid>\t<sessions>` marks this pane's app-server; `foreign\t<sessions>`
    // is a Codex client outside the pane. Its threads are then indistinguishable.
    let mut daemons = std::collections::HashMap::new();
    let mut foreign = std::collections::HashSet::new();
    for line in output.lines() {
        if let Some(rest) = line.strip_prefix("daemon\t") {
            if let Some((pid, root)) = rest.split_once('\t')
                && let Ok(pid) = pid.parse::<u32>()
            {
                daemons.insert(pid, root);
            }
        } else if let Some(root) = line.strip_prefix("foreign\t") {
            foreign.insert(root);
        }
    }
    let mut candidate = None;
    for record in output.lines().filter_map(parse_probe_record) {
        if daemons.get(&record.pid).is_some_and(|root| foreign.contains(root)) {
            continue;
        }
        let Ok(meta) = serde_json::from_str::<serde_json::Value>(&record.first_line) else {
            continue;
        };
        if meta.get("type").and_then(serde_json::Value::as_str) != Some("session_meta") {
            continue;
        }
        let payload = meta.get("payload").unwrap_or(&meta);
        let Some(id) = payload.get("id").and_then(serde_json::Value::as_str) else {
            continue;
        };
        if !valid_uuid(id) {
            continue;
        }
        if !is_main_thread(payload, &meta, app_server) {
            continue;
        }
        let Some(filename_id) = rollout_id(Path::new(&record.link)) else { continue };
        if filename_id != id {
            continue;
        }
        let context = CodexSession {
            session_id: id.to_owned(),
            session_file: record.link,
            cwd: payload
                .get("cwd")
                .and_then(serde_json::Value::as_str)
                .filter(|cwd| valid_cwd(cwd))
                .map(str::to_owned),
        };
        if candidate.as_ref().is_some_and(|existing| existing != &context) {
            // More than one distinct main/user rollout is ambiguous.  A
            // process can have guardian/sub-agent files open, but it must not
            // have two possible user conversations silently choose one.
            return None;
        }
        candidate = Some(context);
    }
    candidate
}

/// Codex's main interactive thread is a CLI/user thread. Keep accepting older
/// session_meta lines that lack these fields, but reject explicit guardian,
/// sub-agent, exec and other client markers. A TUI served by the shared
/// app-server is recorded as `vscode`/`codex-tui`/`user` instead (#633).
fn is_main_thread(
    payload: &serde_json::Value,
    root: &serde_json::Value,
    app_server: AppServerThreads,
) -> bool {
    let field =
        |key: &str| payload.get(key).or_else(|| root.get(key)).map(serde_json::Value::as_str);
    if !matches_optional_string(payload, root, "thread_source", "user") {
        return false;
    }
    match field("source") {
        None | Some(Some("cli")) => true,
        Some(Some("vscode")) => {
            app_server == AppServerThreads::Attributed
                && field("originator") == Some(Some("codex-tui"))
                && field("thread_source") == Some(Some("user"))
        },
        Some(_) => false,
    }
}

fn valid_cwd(cwd: &str) -> bool {
    !cwd.chars().any(char::is_control)
        && (cwd.starts_with('/')
            || cwd.starts_with(r"\\")
            || (cwd.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
                && cwd.as_bytes().get(1) == Some(&b':')
                && matches!(cwd.as_bytes().get(2), Some(b'/' | b'\\'))))
}

#[cfg(test)]
fn parse_probe_records(output: &str) -> Option<String> {
    parse_probe_context(output).map(|context| context.session_id)
}

fn parse_probe_record(line: &str) -> Option<ProbeRecord> {
    let mut fields = line.splitn(4, '\t');
    Some(ProbeRecord {
        pid: fields.next()?.parse().ok()?,
        _fd: fields.next()?.parse().ok()?,
        link: fields.next()?.to_owned(),
        first_line: fields.next()?.to_owned(),
    })
}

fn matches_optional_string(
    payload: &serde_json::Value,
    root: &serde_json::Value,
    key: &str,
    expected: &str,
) -> bool {
    payload
        .get(key)
        .or_else(|| root.get(key))
        .map_or(true, |value| value.as_str() == Some(expected))
}

fn valid_uuid(id: &str) -> bool {
    id.len() == 36
        && id.as_bytes().iter().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) { *byte == b'-' } else { byte.is_ascii_hexdigit() }
        })
}

fn rollout_id(path: &Path) -> Option<String> {
    crate::session::codex_rollout_id(path.to_str()?).map(str::to_owned)
}

fn probe_wsl(
    distro: Option<&str>,
    user: Option<&str>,
    pane_id: &str,
    instance: &str,
) -> Option<CodexSession> {
    let mut command = Command::new("wsl.exe");
    command.args(wsl_probe_args(distro, user, pane_id, instance));
    crate::platform::process::hidden_command(&mut command);
    run_probe_command(command)
}

fn wsl_probe_args(
    distro: Option<&str>,
    user: Option<&str>,
    pane_id: &str,
    instance: &str,
) -> Vec<String> {
    let mut args = Vec::with_capacity(9);
    if let Some(distro) = distro {
        args.extend(["--distribution".to_owned(), distro.to_owned()]);
    }
    if let Some(user) = user {
        args.extend(["--user".to_owned(), user.to_owned()]);
    }
    args.extend([
        "--exec".to_owned(),
        "sh".to_owned(),
        "-c".to_owned(),
        PROBE_SCRIPT.to_owned(),
        "pebrel-ai-session-probe".to_owned(),
        pane_id.to_owned(),
        instance.to_owned(),
    ]);
    args
}

/// Bound the guest lookup by time and output size. A temporary file avoids a
/// reader thread surviving when a descendant keeps the output handle open.
fn run_probe_command(command: Command) -> Option<CodexSession> {
    parse_attributed_probe(&String::from_utf8_lossy(&run_probe_output(command)?))
}

fn run_probe_output(command: Command) -> Option<Vec<u8>> {
    super::process_output::read(command, PROBE_TIMEOUT, MAX_PROBE_OUTPUT).ok()
}

fn read_bounded(mut reader: impl Read, limit: usize) -> io::Result<Vec<u8>> {
    let mut output = Vec::with_capacity(limit.min(8 * 1024));
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            return Ok(output);
        }
        let remaining = limit.saturating_sub(output.len());
        if read > remaining {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "probe output exceeded limit"));
        }
        output.extend_from_slice(&buffer[..read]);
    }
}

#[cfg(target_os = "linux")]
fn probe_local_proc(pane_id: &str, instance: &str) -> Option<CodexSession> {
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let mut output = String::new();
    let mut records = 0;
    let mut foreign = std::collections::HashSet::new();
    for entry in std::fs::read_dir("/proc").ok()?.flatten() {
        if Instant::now() >= deadline {
            return None;
        }
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(|name| name.parse::<u32>().ok()) else {
            continue;
        };
        let proc_dir = entry.path();
        let Ok(comm) = std::fs::read_to_string(proc_dir.join("comm")) else { continue };
        if !is_codex_process_name(comm.trim()) {
            continue;
        }
        let Ok(environment) = std::fs::read(proc_dir.join("environ")) else { continue };
        let daemon = std::fs::read(proc_dir.join("cmdline"))
            .is_ok_and(|arguments| is_app_server_arguments(&arguments));
        let owned = (environment_has_value(&environment, b"PEBREL_PANE_ID", pane_id.as_bytes())
            || environment_has_value(&environment, b"NEBULA_PANE_ID", pane_id.as_bytes()))
            && environment_has_value(
                &environment,
                crate::agent_env::PROCESS_ENV.as_bytes(),
                instance.as_bytes(),
            );
        // Another pane's or terminal's client can share this pane's app-server.
        if !owned && daemon {
            continue;
        }
        let Some(codex_home) = codex_home_from_environment(&environment) else {
            continue;
        };
        // /proc/fd 返回真实路径；先统一目录身份，再做词法边界检查，且不逐句柄访问磁盘。
        let Ok(sessions_root) =
            proc_dir.join("cwd").join(codex_home).join("sessions").canonicalize()
        else {
            continue;
        };
        use std::fmt::Write as _;
        if !owned {
            if foreign.insert(sessions_root.clone()) {
                let _ = writeln!(output, "foreign\t{}", sessions_root.display());
            }
            continue;
        }
        if daemon {
            let _ = writeln!(output, "daemon\t{pid}\t{}", sessions_root.display());
        }
        if output.len() >= MAX_PROBE_OUTPUT {
            return None;
        }
        let Ok(fds) = std::fs::read_dir(proc_dir.join("fd")) else { continue };
        for fd in fds.flatten() {
            let fd_path = fd.path();
            let Ok(link) = std::fs::read_link(&fd_path) else { continue };
            if !is_rollout_link(&link, &sessions_root) {
                continue;
            }
            let Ok(first_line) = read_first_line(&fd_path) else { continue };
            let fd_number = fd.file_name().to_string_lossy().parse::<u32>().ok();
            let Some(fd_number) = fd_number else { continue };
            let _ = writeln!(output, "{pid}\t{fd_number}\t{}\t{first_line}", link.display());
            records += 1;
            if records >= MAX_PROBE_RECORDS || output.len() >= MAX_PROBE_OUTPUT {
                return None;
            }
        }
    }
    parse_attributed_probe(&output)
}

/// `codex app-server` is the shared daemon, not a pane's own client process.
#[cfg(target_os = "linux")]
fn is_app_server_arguments(arguments: &[u8]) -> bool {
    arguments.split(|byte| *byte == 0).skip(1).any(|argument| argument == b"app-server")
}

/// Whether a Codex hook whose nearest Agent ancestor is `agent_pid` runs under
/// the shared app-server while a client outside the daemon's launching pane
/// uses the same Codex home. Such a hook inherits that pane's environment, so
/// its pane field cannot identify the conversation (#633). An in-process TUI,
/// or a daemon serving only its own pane, keeps environment routing. Anything
/// that cannot be read is treated as shared.
#[cfg(target_os = "linux")]
pub(crate) fn codex_app_server_is_shared(agent_pid: u32) -> bool {
    codex_app_server_is_shared_in(Path::new("/proc"), agent_pid)
}

#[cfg(target_os = "linux")]
fn codex_app_server_is_shared_in(proc_root: &Path, agent_pid: u32) -> bool {
    let daemon = proc_root.join(agent_pid.to_string());
    match std::fs::read(daemon.join("cmdline")) {
        Ok(arguments) if is_app_server_arguments(&arguments) => {},
        _ => return false,
    }
    let owner = |environment: &[u8]| {
        let value = |key: &[u8]| environment_value(environment, key).map(<[u8]>::to_vec);
        (
            value(b"PEBREL_PANE_ID").or_else(|| value(b"NEBULA_PANE_ID")),
            value(crate::agent_env::PROCESS_ENV.as_bytes()),
        )
    };
    let sessions = |process: &Path, environment: &[u8]| {
        let home = codex_home_from_environment(environment)?;
        process.join("cwd").join(home).join("sessions").canonicalize().ok()
    };
    let Ok(environment) = std::fs::read(daemon.join("environ")) else { return true };
    let Some(root) = sessions(&daemon, &environment) else { return true };
    let launched_by = owner(&environment);
    let Ok(entries) = std::fs::read_dir(proc_root) else { return true };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
            .is_none_or(|pid| pid == agent_pid)
        {
            continue;
        }
        let process = entry.path();
        let Ok(comm) = std::fs::read_to_string(process.join("comm")) else { continue };
        if !is_codex_process_name(comm.trim())
            || std::fs::read(process.join("cmdline"))
                .is_ok_and(|arguments| is_app_server_arguments(&arguments))
        {
            continue;
        }
        let Ok(environment) = std::fs::read(process.join("environ")) else { return true };
        if owner(&environment) != launched_by
            && sessions(&process, &environment).is_none_or(|other| other == root)
        {
            return true;
        }
    }
    false
}

#[cfg(unix)]
fn is_codex_process_name(name: &str) -> bool {
    matches!(name, "codex" | "codex.exe") || name.starts_with("codex-")
}

#[cfg(unix)]
fn environment_has_value(environment: &[u8], key: &[u8], expected: &[u8]) -> bool {
    let mut prefix = Vec::with_capacity(key.len() + 1);
    prefix.extend_from_slice(key);
    prefix.push(b'=');
    environment
        .split(|byte| *byte == 0)
        .any(|item| item.strip_prefix(prefix.as_slice()).is_some_and(|value| value == expected))
}

#[cfg(unix)]
fn environment_value<'a>(environment: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    let mut prefix = Vec::with_capacity(key.len() + 1);
    prefix.extend_from_slice(key);
    prefix.push(b'=');
    environment
        .split(|byte| *byte == 0)
        .find_map(|item| item.strip_prefix(prefix.as_slice()))
        .filter(|value| !value.is_empty())
}

#[cfg(unix)]
fn codex_home_from_environment(environment: &[u8]) -> Option<std::path::PathBuf> {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt as _;

    let home = environment_value(environment, b"CODEX_HOME")
        .or_else(|| environment_value(environment, b"HOME"))?;
    let home = std::path::PathBuf::from(OsStr::from_bytes(home));
    if environment_value(environment, b"CODEX_HOME").is_some() {
        Some(home)
    } else {
        Some(home.join(".codex"))
    }
}

#[cfg(unix)]
fn is_rollout_link(link: &Path, sessions_root: &Path) -> bool {
    link.strip_prefix(sessions_root).is_ok_and(|relative| {
        relative.components().count() >= 2
            && link
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("rollout-") && name.ends_with(".jsonl"))
    })
}

fn read_first_line(path: &Path) -> std::io::Result<String> {
    let file = std::fs::File::open(path)?;
    read_metadata_line(file)
}

fn read_metadata_line(file: impl Read) -> std::io::Result<String> {
    use std::io::{BufRead as _, BufReader};
    let mut line = String::new();
    BufReader::new(file).take(MAX_PROBE_LINE as u64).read_line(&mut line)?;
    if !line.ends_with(['\r', '\n']) && line.len() >= MAX_PROBE_LINE {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "session metadata line too long"));
    }
    Ok(line.trim_end_matches(['\r', '\n']).to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    mod proc_fixture;

    const ROOT_ID: &str = "01a079fa-4a9b-7d93-8a4a-4a7a9edaf247";
    const GUARDIAN_ID: &str = "01a079f7-7e36-7232-882b-f06d3bde9df8";

    #[cfg(unix)]
    #[test]
    fn guest_lookup_returns_within_its_deadline() {
        let mut command = Command::new("sh");
        command.args(["-c", "exec sleep 10"]);
        let started = Instant::now();
        assert_eq!(run_probe_command(command), None);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn oversized_output_is_rejected_instead_of_selecting_a_partial_candidate() {
        assert_eq!(read_bounded(&b"1234"[..], 4).unwrap(), b"1234");
        assert!(read_bounded(&b"12345"[..], 4).is_err());
    }

    #[test]
    fn review_regression_active_rollout_supplies_cwd_without_accepting_invalid_paths() {
        for (cwd, expected) in [
            ("/mnt/d/temp_build/project", Some("/mnt/d/temp_build/project")),
            ("/home/hello/项目 with spaces", Some("/home/hello/项目 with spaces")),
            ("relative/path", None),
            ("/bad\npath", None),
        ] {
            let metadata = serde_json::json!({"type":"session_meta", "payload": {
                "id": ROOT_ID, "cwd": cwd, "source": "cli", "thread_source": "user",
            }});
            let output = format!(
                "42\t7\t/home/hello/.codex/sessions/rollout-x-{ROOT_ID}.jsonl\t{metadata}\n"
            );
            let context = parse_probe_context(&output).unwrap();
            assert_eq!(context.session_id, ROOT_ID);
            assert_eq!(context.cwd.as_deref(), expected);
        }
    }

    #[test]
    fn selects_cli_user_rollout_and_rejects_guardian_thread() {
        let output = format!(
            "1996\t37\t/home/hello/.codex/sessions/2026/09/07/rollout-x-{ROOT_ID}.jsonl\t{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{ROOT_ID}\",\"source\":\"cli\",\"thread_source\":\"user\"}}}}\n1996\t80\t/home/hello/.codex/sessions/2026/09/07/rollout-x-{GUARDIAN_ID}.jsonl\t{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{GUARDIAN_ID}\",\"source\":\"cli\",\"thread_source\":\"guardian\"}}}}\n"
        );
        assert_eq!(parse_probe_records(&output).as_deref(), Some(ROOT_ID));
    }

    #[test]
    fn rejects_a_rollout_bound_to_a_different_filename() {
        let output = format!(
            "1996\t37\t/home/hello/.codex/sessions/2026/09/07/rollout-x-{ROOT_ID}.jsonl\t{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{GUARDIAN_ID}\"}}}}\n"
        );
        assert_eq!(parse_probe_records(&output), None);
    }

    #[test]
    fn accepts_legacy_session_meta_without_thread_markers() {
        let output = format!(
            "1996\t37\t/home/hello/.codex/sessions/2026/09/07/rollout-x-{ROOT_ID}.jsonl\t{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{ROOT_ID}\"}}}}\n"
        );
        assert_eq!(parse_probe_records(&output).as_deref(), Some(ROOT_ID));
    }

    fn tui_thread(payload: serde_json::Value) -> String {
        let meta = serde_json::json!({"type":"session_meta", "payload": payload});
        format!(
            "1996\t47\t/home/hello/.codex/sessions/2026/11/04/rollout-x-{ROOT_ID}.jsonl\t{meta}\n"
        )
    }

    #[test]
    fn app_server_tui_threads_are_accepted_only_with_daemon_evidence() {
        // Codex 0.160+ TUI threads are written by the shared app-server (#633).
        let record = tui_thread(serde_json::json!({
            "id": ROOT_ID, "originator": "codex-tui", "source": "vscode", "thread_source": "user",
        }));
        assert_eq!(
            parse_attributed_probe(&record).map(|found| found.session_id).as_deref(),
            Some(ROOT_ID)
        );
        // Probes that cannot recognize the shared daemon keep rejecting them.
        assert_eq!(parse_probe_context(&record), None);
        for payload in [
            serde_json::json!({"id": ROOT_ID, "originator": "codex_vscode", "source": "vscode", "thread_source": "user"}),
            serde_json::json!({"id": ROOT_ID, "source": "vscode", "thread_source": "user"}),
            serde_json::json!({"id": ROOT_ID, "originator": "codex-tui", "source": "vscode"}),
            serde_json::json!({"id": ROOT_ID, "originator": "codex-tui", "source": "vscode", "thread_source": "subagent"}),
            serde_json::json!({"id": ROOT_ID, "originator": "codex-tui", "source": "vscode", "thread_source": "guardian"}),
            serde_json::json!({"id": ROOT_ID, "originator": "codex_exec", "source": "exec", "thread_source": "user"}),
            serde_json::json!({"id": ROOT_ID, "originator": "codex-tui", "source": {"subagent": "review"}}),
        ] {
            assert_eq!(parse_attributed_probe(&tui_thread(payload.clone())), None, "{payload}");
        }
    }

    #[test]
    fn shared_app_server_threads_are_ambiguous_while_another_client_uses_its_home() {
        let root = "/home/hello/.codex/sessions";
        let record = tui_thread(serde_json::json!({
            "id": ROOT_ID, "originator": "codex-tui", "source": "vscode", "thread_source": "user",
        }));
        let daemon = format!("daemon\t1996\t{root}\n");
        let found = |output: String| parse_attributed_probe(&output).map(|found| found.session_id);
        assert_eq!(found(format!("{daemon}{record}")).as_deref(), Some(ROOT_ID));
        // Another pane's client may be the one showing this thread.
        assert_eq!(found(format!("{daemon}{record}foreign\t{root}\n")), None);
        assert_eq!(found(format!("foreign\t{root}\n{daemon}{record}")), None);
        // Clients of another Codex home use another daemon.
        assert_eq!(
            found(format!("{daemon}foreign\t/srv/other/sessions\n{record}")).as_deref(),
            Some(ROOT_ID)
        );
        // A pane's own in-process TUI remains authoritative for its rollout.
        let own = tui_thread(
            serde_json::json!({"id": ROOT_ID, "source": "cli", "thread_source": "user"}),
        );
        assert_eq!(found(format!("foreign\t{root}\n{own}")).as_deref(), Some(ROOT_ID));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn shared_app_server_rollout_belongs_to_a_pane_only_without_other_clients() {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::symlink;

        struct OwnedChild(std::process::Child);
        impl Drop for OwnedChild {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }

        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("shared home");
        let other_home = root.path().join("other home");
        let sessions = home.join(".codex/sessions/2026/11/04");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::create_dir_all(other_home.join(".codex/sessions")).unwrap();
        let rollout = sessions.join(format!("rollout-x-{ROOT_ID}.jsonl"));
        let metadata = serde_json::json!({"type":"session_meta", "payload": {
            "id": ROOT_ID, "originator": "codex-tui", "source": "vscode", "thread_source": "user",
        }});
        std::fs::write(&rollout, format!("{metadata}\n")).unwrap();
        let program = root.path().join("codex");
        symlink("/bin/sh", &program).unwrap();
        let instance = std::process::id().to_string();
        // `$0` becomes `app-server`, matching the daemon's argv without running Codex.
        let spawn = |pane: &str, home: &Path, daemon: bool| {
            let mut command = Command::new(&program);
            command.args(["-c", "printf R; read -r _"]);
            if daemon {
                command.arg("app-server");
            }
            command
                .current_dir(root.path())
                .env_clear()
                .env("HOME", home)
                .env("PEBREL_PANE_ID", pane)
                .env("NEBULA_PANE_ID", pane)
                .env(crate::agent_env::PROCESS_ENV, &instance)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(if daemon {
                    Stdio::from(std::fs::File::open(&rollout).unwrap())
                } else {
                    Stdio::null()
                });
            let mut child = OwnedChild(command.spawn().unwrap());
            let mut ready = child.0.stdout.take().unwrap();
            let mut event =
                libc::pollfd { fd: ready.as_raw_fd(), events: libc::POLLIN, revents: 0 };
            // SAFETY: pollfd 在调用期间有效；只等待样本 exec 完成后的就绪字节。
            let signaled =
                unsafe { libc::poll(&mut event, 1, PROBE_TIMEOUT.as_millis() as libc::c_int) };
            assert_eq!(signaled, 1, "fixture did not signal readiness");
            let mut signal = [0];
            ready.read_exact(&mut signal).unwrap();
            assert_eq!(signal, [b'R']);
            child
        };
        let mut daemon = spawn("pane-a", &home, true);
        let fixture_cwd = root.path().canonicalize().unwrap();
        let fixture_rollout = rollout.canonicalize().unwrap();
        let first_line = metadata.to_string();
        proc_fixture::wait_for_child(
            &mut daemon.0,
            &proc_fixture::Expected {
                pane: "pane-a",
                instance: &instance,
                home: &home,
                codex_home: None,
                cwd: &fixture_cwd,
                rollout: &fixture_rollout,
                first_line: &first_line,
            },
        )
        .unwrap();
        let probe = |pane: &str| {
            let local = probe_local_proc(pane, &instance).map(|found| found.session_id);
            let mut guest = Command::new("/bin/sh");
            guest.env_clear().env("PATH", "/usr/bin:/bin").args([
                "-c",
                PROBE_SCRIPT,
                "probe",
                pane,
                &instance,
            ]);
            let guest = run_probe_command(guest).map(|found| found.session_id);
            assert_eq!(local, guest, "local and guest probes disagree for {pane}");
            local
        };
        let shared = || codex_app_server_is_shared(daemon.0.id());
        // The pane whose client started the daemon owns it while no other client exists.
        let own_client = spawn("pane-a", &home, false);
        assert_eq!(probe("pane-a").as_deref(), Some(ROOT_ID));
        assert_eq!(probe("pane-b"), None);
        assert!(!shared(), "an exclusive daemon keeps environment routing");
        assert!(!codex_app_server_is_shared(own_client.0.id()), "a TUI is not the daemon");
        // A second pane's client shares the daemon: its environment no longer identifies a pane.
        let shared_client = spawn("pane-b", &home, false);
        assert_eq!(probe("pane-a"), None);
        assert_eq!(probe("pane-b"), None);
        assert!(shared(), "hooks for either pane now carry pane-a's environment");
        drop(shared_client);
        // A client of another Codex home uses another daemon.
        let separate_client = spawn("pane-b", &other_home, false);
        assert_eq!(probe("pane-a").as_deref(), Some(ROOT_ID));
        assert!(!shared());
        drop((separate_client, own_client, daemon));
    }

    #[test]
    fn wsl_probe_keeps_named_distribution_and_pane_as_arguments() {
        let full = wsl_probe_args(Some("Debian"), Some("hello"), "7", "42");
        let named: Vec<_> = full.iter().map(String::as_str).take(5).collect();
        assert_eq!(named, vec!["--distribution", "Debian", "--user", "hello", "--exec"]);
        let args = wsl_probe_args(None, None, "7", "42");
        assert_eq!(args.first().map(String::as_str), Some("--exec"));
        assert_eq!(args[args.len() - 2], "7");
        assert_eq!(args.last().map(String::as_str), Some("42"));
        assert_eq!(args[args.len() - 3], "pebrel-ai-session-probe");
    }

    #[test]
    fn explicit_subagent_markers_are_not_accepted_as_the_main_thread() {
        let output = format!(
            "1996\t37\t/home/hello/.codex/sessions/2026/09/07/rollout-x-{ROOT_ID}.jsonl\t{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{ROOT_ID}\",\"source\":\"subagent\",\"thread_source\":\"user\"}}}}\n"
        );
        assert_eq!(parse_probe_records(&output), None);
    }

    #[test]
    fn multiple_distinct_main_rollouts_are_rejected_as_ambiguous() {
        let output = format!(
            "1996\t37\t/home/hello/.codex/sessions/2026/09/07/rollout-x-{ROOT_ID}.jsonl\t{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{ROOT_ID}\",\"source\":\"cli\"}}}}\n1996\t38\t/home/hello/.codex/sessions/2026/09/07/rollout-x-{GUARDIAN_ID}.jsonl\t{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{GUARDIAN_ID}\",\"source\":\"cli\"}}}}\n"
        );
        assert_eq!(parse_probe_records(&output), None);
    }

    #[cfg(unix)]
    #[test]
    fn custom_codex_home_is_used_instead_of_the_default_directory() {
        let environment = b"HOME=/home/hello\0CODEX_HOME=/srv/codex-data\0";
        assert_eq!(
            codex_home_from_environment(environment).as_deref(),
            Some(Path::new("/srv/codex-data"))
        );
        let environment = b"HOME=/home/hello\0";
        assert_eq!(
            codex_home_from_environment(environment).as_deref(),
            Some(Path::new("/home/hello/.codex"))
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn proc_probes_resolve_session_symlinks_and_preserve_ownership_boundaries() {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::symlink;

        struct OwnedChild(std::process::Child);
        impl Drop for OwnedChild {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }

        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("user");
        let physical = root.path().join("physical data");
        let sessions = physical.join("sessions/2026/10/06");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::create_dir(&home).unwrap();
        symlink(&physical, home.join(".codex")).unwrap();
        let linked_sessions = root.path().join("linked sessions");
        std::fs::create_dir(&linked_sessions).unwrap();
        symlink(physical.join("sessions"), linked_sessions.join("sessions")).unwrap();
        let unrelated = root.path().join("unrelated");
        std::fs::create_dir_all(unrelated.join("sessions")).unwrap();
        let rollout = sessions.join(format!("rollout-x-{ROOT_ID}.jsonl"));
        let metadata = serde_json::json!({"type":"session_meta", "payload": {
            "id": ROOT_ID, "source": "cli", "thread_source": "user",
        }});
        std::fs::write(&rollout, format!("{metadata}\n")).unwrap();
        // 样本只持有日志文件；就绪字节保证 exec 已完成，避免读到继承的测试线程名。
        let program = root.path().join("codex");
        symlink("/bin/sh", &program).unwrap();
        let pane = "session-symlink-fixture";
        let instance = std::process::id().to_string();
        let fixture_cwd = root.path().canonicalize().unwrap();
        let fixture_rollout = rollout.canonicalize().unwrap();
        let first_line = metadata.to_string();
        for (case, (codex_home, expected)) in [
            (Some(physical.clone()), true),
            (Some(home.join(".codex")), true),
            (None, true),
            (Some(linked_sessions), true),
            (Some(std::path::PathBuf::from("physical data")), true),
            (Some(unrelated), false),
        ]
        .into_iter()
        .enumerate()
        {
            let mut command = Command::new(&program);
            command
                .args(["-c", "printf R; read -r _"])
                .current_dir(root.path())
                .env_clear()
                .env("HOME", &home)
                .env("PEBREL_PANE_ID", pane)
                .env("NEBULA_PANE_ID", pane)
                .env(crate::agent_env::PROCESS_ENV, &instance)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(std::fs::File::open(&rollout).unwrap());
            if let Some(value) = &codex_home {
                command.env("CODEX_HOME", value);
            }
            let mut child = OwnedChild(command.spawn().unwrap());
            // stdin 管道维持样本存活；父进程退出时 EOF 也能结束样本，不遗留休眠进程。
            let mut ready = child.0.stdout.take().unwrap();
            let mut event =
                libc::pollfd { fd: ready.as_raw_fd(), events: libc::POLLIN, revents: 0 };
            // SAFETY: pollfd 在调用期间有效；只等待就绪，不重试被测探测器或延长其期限。
            let signaled =
                unsafe { libc::poll(&mut event, 1, PROBE_TIMEOUT.as_millis() as libc::c_int) };
            assert_eq!(signaled, 1, "fixture did not signal readiness");
            let mut signal = [0];
            ready.read_exact(&mut signal).unwrap();
            assert_eq!(signal, [b'R']);
            let expected_child = proc_fixture::Expected {
                pane,
                instance: &instance,
                home: &home,
                codex_home: codex_home.as_deref(),
                cwd: &fixture_cwd,
                rollout: &fixture_rollout,
                first_line: &first_line,
            };
            proc_fixture::wait_for_child(&mut child.0, &expected_child).unwrap_or_else(|error| {
                panic!("case={case} home={codex_home:?}: fixture not ready: {error}")
            });
            for (query_pane, query_instance, matches) in [
                (pane, instance.as_str(), expected),
                ("other-pane", instance.as_str(), false),
                (pane, "other-instance", false),
            ] {
                let assert_found = |probe, found: Option<CodexSession>, elapsed: Duration| {
                    let context = format!(
                        "probe={probe} case={case} child={} pane={query_pane} \
                         instance={query_instance} home={codex_home:?} elapsed={elapsed:?}",
                        child.0.id(),
                    );
                    assert_eq!(found.is_some(), matches, "{context}");
                    if let Some(found) = found {
                        assert_eq!(found.session_id, ROOT_ID, "{context}");
                        assert_eq!(Path::new(&found.session_file), rollout, "{context}");
                    }
                };
                // Assert immediately after each single probe. An eager array
                // erased which implementation failed in the original CI log.
                let started = Instant::now();
                let found = probe_local_proc(query_pane, query_instance);
                assert_found("local_proc", found, started.elapsed());

                let mut guest = Command::new("/bin/sh");
                guest.env_clear().env("PATH", "/usr/bin:/bin").env("HOME", &home).args([
                    "-c",
                    PROBE_SCRIPT,
                    "probe",
                    query_pane,
                    query_instance,
                ]);
                let started = Instant::now();
                let found = run_probe_command(guest);
                assert_found("guest_shell", found, started.elapsed());
            }
        }
    }
}
