//! Windows named-pipe transport. Supplies kernel process identity to parsed events.

use super::helper_path;
use crate::ai_hook::{
    AiHookEvent, HOOK_EXE_ENV, LEGACY_HOOK_EXE_ENV, LEGACY_PIPE_ENV, PIPE_ENV, parse_envelope,
};
#[cfg(feature = "legacy-shell")]
use crate::event::{Event, EventType};
#[cfg(feature = "legacy-shell")]
use winit::event_loop::EventLoopProxy;

// ─── pipe server ────────────────────────────────────────────────────────

/// Create the per-instance pipe, export its name to future children, and
/// start the accept loop. Must run before the first PTY spawns.
#[cfg(feature = "legacy-shell")]
pub fn spawn_server(proxy: EventLoopProxy<Event>) {
    spawn_pipe_server(move |event| {
        proxy.send_event(Event::new(EventType::AiHook(event), None)).is_ok()
    });
}

/// GPUI owns a different event loop, but hook parsing and pipe ownership
/// stay identical. The workspace drains this channel on its foreground
/// executor and routes events by the same stable pane id contract.
pub fn spawn_gpui_server() -> std::sync::mpsc::Receiver<AiHookEvent> {
    let (tx, rx) = std::sync::mpsc::channel();
    spawn_pipe_server(move |event| tx.send(event).is_ok());
    rx
}

fn spawn_pipe_server(sink: impl Fn(AiHookEvent) -> bool + Send + 'static) {
    let name = format!(r"\\.\pipe\pebrel-notify-{}", std::process::id());
    // SAFETY: single-threaded startup; no other thread reads the env yet.
    unsafe {
        std::env::set_var(PIPE_ENV, &name);
        std::env::set_var(LEGACY_PIPE_ENV, &name);
    };
    // Export nebula-hook.exe's path for the opencode plugin (best-effort:
    // if the helper isn't found, the plugin simply no-ops like anywhere
    // outside Nebula). Forward slashes: the path is interpolated into
    // Bun's `$` shell inside the plugin, matching `helper_command`.
    if let Some(helper) = helper_path() {
        let p = helper.display().to_string().replace('\\', "/");
        unsafe {
            std::env::set_var(HOOK_EXE_ENV, &p);
            std::env::set_var(LEGACY_HOOK_EXE_ENV, &p);
        };
    }
    if let Err(err) =
        std::thread::Builder::new().name("pebrel-ai-pipe".into()).spawn(move || serve(&name, sink))
    {
        log::warn!("ai_hook: failed to spawn pipe server: {err}");
    }
}

/// Accept loop. One fresh pipe instance per connection: a client racing
/// the turnaround sees a failed open for microseconds and retries (the
/// helper retries for ~100 ms — an eternity at this message rate).
fn serve(name: &str, sink: impl Fn(AiHookEvent) -> bool) {
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_PIPE_CONNECTED, GetLastError, INVALID_HANDLE_VALUE,
    };
    // PIPE_ACCESS_INBOUND is a FILE_FLAGS_AND_ATTRIBUTES constant, hence
    // its home in the FileSystem module rather than Pipes.
    use windows_sys::Win32::Storage::FileSystem::{PIPE_ACCESS_INBOUND, ReadFile};
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeClientProcessId,
        PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    };

    let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    loop {
        // SAFETY: `wide` is NUL-terminated and outlives the call. Null
        // security attributes = default DACL, same-user access only.
        let pipe = unsafe {
            CreateNamedPipeW(
                wide.as_ptr(),
                PIPE_ACCESS_INBOUND,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                PIPE_UNLIMITED_INSTANCES,
                0,
                64 * 1024,
                0,
                std::ptr::null(),
            )
        };
        if pipe == INVALID_HANDLE_VALUE {
            log::warn!("ai_hook: CreateNamedPipeW failed; AI turn events disabled");
            return;
        }

        // SAFETY: `pipe` is a valid handle owned by this frame.
        // ERROR_PIPE_CONNECTED = the client connected first; still good.
        let connected = unsafe { ConnectNamedPipe(pipe, std::ptr::null_mut()) } != 0
            || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
        if connected {
            // 客户端身份必须在断开连接之前问：这是内核对「谁在写这条管道」
            // 的回答，载荷里自报的任何 pid 都可以伪造，这个不行。helper 此刻
            // 一定还活着（它正连着我们），所以随后的祖先链查询能命中。
            let client_pid = {
                let mut pid = 0u32;
                // SAFETY: `pipe` 是本帧持有的有效句柄；`pid` 先于读取写入。
                let ok = unsafe { GetNamedPipeClientProcessId(pipe, &mut pid) };
                (ok != 0 && pid != 0).then_some(pid)
            };
            let mut buf = Vec::with_capacity(4096);
            let mut chunk = [0u8; 4096];
            loop {
                let mut read = 0u32;
                // SAFETY: `chunk` outlives the call; `read` written first.
                let ok = unsafe {
                    ReadFile(
                        pipe,
                        chunk.as_mut_ptr(),
                        chunk.len() as u32,
                        &mut read,
                        std::ptr::null_mut(),
                    )
                };
                // ok == 0 is the normal EOF (BROKEN_PIPE on client close).
                if ok == 0 || read == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..read as usize]);
                if buf.len() > (1 << 20) {
                    break;
                }
            }
            if buf.len() <= (1 << 20)
                && let Some(mut event) = parse_envelope(&buf)
            {
                event.client_pid = client_pid;
                // agent 的进程身份：helper 的父进程往往是执行 hook 命令的
                // shell，agent 在更上一层，所以要沿祖先链找。用它区分嵌套
                // 子代理，见 `AiHookEvent::agent_pid`。
                event.agent_pid = client_pid
                    .and_then(crate::process_tree::nearest_agent_ancestor)
                    .map(|(pid, _)| pid);
                log::debug!("ai_hook: {event:?}");
                if !sink(event) {
                    // Event loop gone: shutting down.
                    // SAFETY: `pipe` is still the valid handle from above.
                    unsafe {
                        DisconnectNamedPipe(pipe);
                        CloseHandle(pipe);
                    }
                    return;
                }
            }
        }
        // SAFETY: `pipe` is valid; failures past this point only cost
        // this one instance, the loop creates a fresh one.
        unsafe {
            DisconnectNamedPipe(pipe);
            CloseHandle(pipe);
        }
    }
}
