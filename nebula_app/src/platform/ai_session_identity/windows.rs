//! Native handle inspection is confined to a disposable copy of this process.
//! Never seek/read a duplicated live handle: it shares Codex's file position.

use super::*;
use std::collections::HashMap;
use std::mem;
use std::os::windows::io::AsRawHandle;
use std::ptr;

use windows_sys::Wdk::System::Threading::{NtQueryInformationProcess, ProcessHandleInformation};
use windows_sys::Win32::Foundation::{
    CloseHandle, DUPLICATE_SAME_ACCESS, DuplicateHandle, HANDLE, STATUS_BUFFER_TOO_SMALL,
    STATUS_INFO_LENGTH_MISMATCH,
};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, FILE_NAME_OPENED, FILE_TYPE_DISK, GetFileInformationByHandle,
    GetFileType, GetFinalPathNameByHandleW,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetProcessTimes, OpenProcess, PROCESS_DUP_HANDLE, PROCESS_QUERY_INFORMATION,
};

const HELPER_ARG: &str = "--internal-codex-session-probe";
const MAX_HANDLE_BYTES: usize = 2 * 1024 * 1024;

struct OwnedHandle(HANDLE);
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

fn open_process(pid: u32, access: u32) -> Option<OwnedHandle> {
    let handle = unsafe { OpenProcess(access, 0, pid) };
    (!handle.is_null()).then_some(OwnedHandle(handle))
}

fn creation_time(process: HANDLE) -> Option<u64> {
    let mut times = [unsafe { mem::zeroed() }; 4];
    let [created, exited, kernel, user] = &mut times;
    (unsafe { GetProcessTimes(process, created, exited, kernel, user) } != 0)
        .then_some(u64::from(created.dwHighDateTime) << 32 | u64::from(created.dwLowDateTime))
}

pub(super) fn probe(shell_pid: u32) -> Option<CodexSession> {
    // Keep the actual shell object alive across helper startup. A recycled PID
    // cannot silently bind this pane to a different process tree.
    let shell = open_process(shell_pid, PROCESS_QUERY_INFORMATION)?;
    let created = creation_time(shell.0)?;
    let mut command = helper_command(shell_pid, created)?;
    crate::platform::process::hidden_command(&mut command);
    run_probe_command(command)
}

fn helper_command(shell_pid: u32, created: u64) -> Option<Command> {
    let mut command = Command::new(std::env::current_exe().ok()?);
    #[cfg(not(test))]
    command.args([HELPER_ARG, &shell_pid.to_string(), &created.to_string()]);
    #[cfg(test)]
    command
        .args([
            "--exact",
            "platform::ai_session_identity::windows::tests::probe_helper",
            "--nocapture",
        ])
        .env("PEBREL_TEST_PROBE_ROOT", format!("{shell_pid}:{created}"));
    Some(command)
}

pub(super) fn run_helper_if_requested() -> Option<i32> {
    let mut args = std::env::args();
    args.next();
    if args.next().as_deref() != Some(HELPER_ARG) {
        return None;
    }
    let parameters = args
        .next()
        .and_then(|pid| Some((pid.parse::<u32>().ok()?, args.next()?.parse::<u64>().ok()?)));
    let Some((pid, created)) = parameters.filter(|_| args.next().is_none()) else {
        return Some(2);
    };
    Some(emit_records(pid, created))
}

fn emit_records(shell_pid: u32, created: u64) -> i32 {
    match collect_records(shell_pid, created) {
        Some(output) => {
            use std::io::Write as _;
            if std::io::stdout().write_all(output.as_bytes()).is_ok() { 0 } else { 1 }
        },
        None => 1,
    }
}

fn collect_records(shell_pid: u32, created: u64) -> Option<String> {
    let rows = crate::platform::process_snapshot::snapshot().ok()?;
    let times: HashMap<_, _> = rows.iter().map(|row| (row.pid, row.created)).collect();
    if created == 0 || times.get(&shell_pid) != Some(&created) {
        return None;
    }
    let processes = crate::process_tree::descendants_from_rows(shell_pid, rows).ok()?;
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let mut output = String::new();
    let mut records = 0;
    for row in processes {
        let name = row.executable.to_ascii_lowercase();
        if name != "codex.exe" && !(name.starts_with("codex-") && name.ends_with(".exe")) {
            continue;
        }
        let process = open_process(row.pid, PROCESS_QUERY_INFORMATION | PROCESS_DUP_HANDLE)?;
        if creation_time(process.0) != times.get(&row.pid).copied() {
            return None;
        }
        for handle in process_handles(process.0)? {
            if Instant::now() >= deadline {
                return None;
            }
            let mut copied = ptr::null_mut();
            if unsafe {
                DuplicateHandle(
                    process.0,
                    handle,
                    GetCurrentProcess(),
                    &mut copied,
                    0,
                    0,
                    DUPLICATE_SAME_ACCESS,
                )
            } == 0
            {
                continue;
            }
            let copied = OwnedHandle(copied);
            if unsafe { GetFileType(copied.0) } != FILE_TYPE_DISK {
                continue;
            }
            let Some(path) = file_path(copied.0) else { continue };
            if crate::session::codex_rollout_id(&path).is_none() {
                continue;
            }
            // Open a separate file object, checking it still denotes the same
            // file after resolving the live handle's path (rename/replacement).
            let file = std::fs::File::open(&path).ok()?;
            if file_identity(copied.0)? != file_identity(file.as_raw_handle())? {
                return None;
            }
            let line = read_metadata_line(file).ok()?;
            let meta: serde_json::Value = serde_json::from_str(&line).ok()?;
            if meta.get("type").and_then(serde_json::Value::as_str) != Some("session_meta") {
                continue;
            }
            // Transport identity metadata only, even if a provider adds other
            // fields to its first record in a future version.
            let payload = meta.get("payload").unwrap_or(&meta);
            let mut safe = serde_json::Map::new();
            for key in ["id", "cwd", "source", "thread_source"] {
                if let Some(value) = payload.get(key).or_else(|| meta.get(key)) {
                    safe.insert(key.to_owned(), value.clone());
                }
            }
            let metadata = serde_json::json!({"type": "session_meta", "payload": safe});
            use std::fmt::Write as _;
            writeln!(output, "{}\t0\t{path}\t{metadata}", row.pid).ok()?;
            records += 1;
            if records >= MAX_PROBE_RECORDS || output.len() >= MAX_PROBE_OUTPUT {
                return None;
            }
        }
    }
    Some(output)
}

#[repr(C)]
#[derive(Clone, Copy)]
struct HandleEntry {
    value: HANDLE,
    handle_count: usize,
    pointer_count: usize,
    granted_access: u32,
    object_type_index: u32,
    attributes: u32,
    reserved: u32,
}

fn process_handles(process: HANDLE) -> Option<Vec<HANDLE>> {
    let mut storage = vec![0usize; 1024];
    loop {
        let capacity = mem::size_of_val(storage.as_slice());
        let mut needed = 0;
        let status = unsafe {
            NtQueryInformationProcess(
                process,
                ProcessHandleInformation,
                storage.as_mut_ptr().cast(),
                capacity as u32,
                &mut needed,
            )
        };
        if matches!(status, STATUS_INFO_LENGTH_MISMATCH | STATUS_BUFFER_TOO_SMALL) {
            let next = (needed as usize).max(capacity * 2);
            if next > MAX_HANDLE_BYTES {
                return None;
            }
            storage.resize(next.div_ceil(mem::size_of::<usize>()), 0);
            continue;
        }
        if status < 0
            || needed as usize > capacity
            || (needed as usize) < 2 * mem::size_of::<usize>()
        {
            return None;
        }
        let count = storage[0];
        let bytes = (needed as usize) - 2 * mem::size_of::<usize>();
        if count > bytes / mem::size_of::<HandleEntry>() {
            return None;
        }
        let entries = unsafe { storage.as_ptr().add(2).cast::<HandleEntry>() };
        return Some(
            (0..count)
                .map(|index| unsafe { ptr::read_unaligned(entries.add(index)).value })
                .collect(),
        );
    }
}

fn file_path(handle: HANDLE) -> Option<String> {
    let mut buffer = vec![0u16; 4096];
    let length = unsafe {
        GetFinalPathNameByHandleW(
            handle,
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            FILE_NAME_OPENED,
        )
    } as usize;
    if length == 0 || length >= buffer.len() {
        return None;
    }
    let path = String::from_utf16(&buffer[..length]).ok()?;
    // Keep network paths valid while making local saved paths readable.
    Some(if let Some(unc) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        path.strip_prefix(r"\\?\").unwrap_or(&path).to_owned()
    })
}

fn file_identity(handle: HANDLE) -> Option<(u32, u32, u32)> {
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { mem::zeroed() };
    (unsafe { GetFileInformationByHandle(handle, &mut info) } != 0).then_some((
        info.dwVolumeSerialNumber,
        info.nFileIndexHigh,
        info.nFileIndexLow,
    ))
}

#[cfg(test)]
mod tests;
