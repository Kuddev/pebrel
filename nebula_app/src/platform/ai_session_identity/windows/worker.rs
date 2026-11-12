//! One process-owned probe worker, with a single bounded request in flight.
//! Private inherited pipes carry identities; no endpoint or user files are created.

use super::{MAX_PROBE_OUTPUT, PROBE_TIMEOUT, collect_records};
use crate::platform::process::CREATE_NO_WINDOW;
use std::fs::{File, OpenOptions};
use std::io::{self, Read as _, Write as _};
use std::mem;
use std::os::windows::ffi::OsStrExt as _;
use std::os::windows::io::{AsHandle as _, AsRawHandle as _, FromRawHandle as _, OwnedHandle};
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, TryLockError};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    DUPLICATE_SAME_ACCESS, DuplicateHandle, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::System::Pipes::{CreatePipe, PeekNamedPipe};
use windows_sys::Win32::System::Threading::{
    CREATE_SUSPENDED, CreateProcessW, DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT,
    GetCurrentProcess, InitializeProcThreadAttributeList, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
    PROCESS_INFORMATION, ResumeThread, STARTF_FORCEOFFFEEDBACK, STARTF_USESTDHANDLES,
    STARTUPINFOEXW, TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject,
};

pub(super) const ARG: &str = "--internal-codex-session-worker";
const READY: &[u8] = b"PEBREL_SESSION_PROBE_V1\n";
const POLL: Duration = Duration::from_millis(5);
static WORKER: Mutex<Option<Worker>> = Mutex::new(None);
static UPDATE_PAUSES: AtomicUsize = AtomicUsize::new(0);

pub(crate) struct UpdatePause {
    resume_on_drop: bool,
}

impl UpdatePause {
    pub(crate) fn acquire() -> io::Result<Self> {
        // Close the gate before waiting for an in-flight probe, so queued probes
        // cannot create another executable after we have reaped the current one.
        UPDATE_PAUSES.fetch_add(1, Ordering::AcqRel);
        let pause = Self { resume_on_drop: true };
        let mut slot = lock_worker(Instant::now() + PROBE_TIMEOUT + Duration::from_secs(1))?;
        if let Some(mut worker) = slot.take() {
            worker.stop()?;
        }
        Ok(pause)
    }

    pub(crate) fn commit(&mut self) {
        self.resume_on_drop = false;
    }
}

impl Drop for UpdatePause {
    fn drop(&mut self) {
        if self.resume_on_drop {
            // This guard may return to the UI thread. Never wait for a probe here.
            UPDATE_PAUSES.fetch_sub(1, Ordering::AcqRel);
        }
    }
}

pub(super) fn probe(pid: u32, created: u64) -> Option<super::CodexSession> {
    if UPDATE_PAUSES.load(Ordering::Acquire) != 0 {
        return None;
    }
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let mut slot = lock_worker(deadline).ok()?;
    if UPDATE_PAUSES.load(Ordering::Acquire) != 0 {
        return None;
    }
    let output = query_slot(&mut slot, pid, created, deadline).ok()?;
    super::parse_probe_context(&output)
}

fn lock_worker(deadline: Instant) -> io::Result<MutexGuard<'static, Option<Worker>>> {
    // Several panes may ask together. Waiting for the shared worker uses the
    // same request deadline, rather than serially adding unbounded waits.
    loop {
        if Instant::now() >= deadline {
            return Err(io::ErrorKind::TimedOut.into());
        }
        match WORKER.try_lock() {
            Ok(slot) => return Ok(slot),
            Err(TryLockError::Poisoned(error)) => return Ok(error.into_inner()),
            Err(TryLockError::WouldBlock) => std::thread::sleep(POLL),
        }
    }
}

fn query_slot(
    slot: &mut Option<Worker>,
    pid: u32,
    created: u64,
    deadline: Instant,
) -> io::Result<String> {
    // Restore the worker only after a complete reply. Errors, unwinding and
    // timeouts discard the entire channel; an old reply can never satisfy a new request.
    let mut worker = match slot.take() {
        Some(worker) => worker,
        None => Worker::start(&arguments(), deadline)?,
    };
    let output = worker.query(pid, created, deadline)?;
    let output = String::from_utf8(output).map_err(|_| io::ErrorKind::InvalidData)?;
    *slot = Some(worker);
    Ok(output)
}

fn arguments() -> Vec<String> {
    #[cfg(not(test))]
    return vec![ARG.into()];
    #[cfg(test)]
    ["--exact", tests::HELPER, "--nocapture", "--skip", tests::CHILD].map(str::to_owned).to_vec()
}

pub(super) fn run() -> i32 {
    let stdin = io::stdin();
    let stdout = io::stdout();
    serve(&mut stdin.lock(), &mut stdout.lock(), collect_records).map_or(1, |()| 0)
}

fn serve(
    input: &mut impl io::Read,
    output: &mut impl io::Write,
    mut collect: impl FnMut(u32, u64) -> Option<String>,
) -> io::Result<()> {
    output.write_all(READY)?;
    output.flush()?;
    loop {
        let mut request = [0; 12];
        match input.read_exact(&mut request) {
            Ok(()) => {},
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(error) => return Err(error),
        }
        let pid = u32::from_le_bytes(request[..4].try_into().unwrap());
        let created = u64::from_le_bytes(request[4..].try_into().unwrap());
        let records = collect(pid, created).unwrap_or_default();
        if records.len() > MAX_PROBE_OUTPUT {
            return Err(io::ErrorKind::InvalidData.into());
        }
        output.write_all(&(records.len() as u32).to_le_bytes())?;
        output.write_all(records.as_bytes())?;
        output.flush()?;
    }
}

struct Worker {
    process: OwnedHandle,
    group: Option<crate::platform::process::ProcessGroup>,
    input: File,
    output: File,
}

impl Worker {
    fn stop(&mut self) -> io::Result<()> {
        drop(self.group.take());
        // SAFETY: this is our owned process object, including startup failures.
        // An already exited child is successful; no PID/name lookup can kill a peer.
        unsafe {
            TerminateProcess(self.process.as_raw_handle(), 1);
            match WaitForSingleObject(self.process.as_raw_handle(), 500) {
                WAIT_OBJECT_0 => Ok(()),
                WAIT_TIMEOUT => Err(io::ErrorKind::TimedOut.into()),
                _ => Err(io::Error::last_os_error()),
            }
        }
    }

    fn start(args: &[String], deadline: Instant) -> io::Result<Self> {
        let (input_read, input_write) = pipe()?;
        let (output_read, output_write) = pipe()?;
        let errors = OpenOptions::new().read(true).write(true).open("NUL")?;
        let (process, thread) = spawn(args, [&input_read, &output_write, &errors])?;
        // Only the child retains these pipe ends after launch, so its exit is
        // observable as EOF. No inherited parent pipe end can keep it alive.
        drop((input_read, output_write, errors));
        let mut worker = Self { process, group: None, input: input_write, output: output_read };
        worker.group = Some(crate::platform::process::ProcessGroup::attach_suspended(
            worker.process.as_handle(),
        )?);
        // SAFETY: this is the owned, still-suspended initial thread. Its process
        // cannot execute until after successful kill-on-close job assignment.
        if unsafe { ResumeThread(thread.as_raw_handle()) } == u32::MAX {
            return Err(io::Error::last_os_error());
        }
        // Test executables emit a short harness preamble. Bound it too; product
        // helpers begin with READY and never initialize GUI, hooks or logging.
        let mut line = Vec::new();
        for _ in 0..1024 {
            let byte = worker.receive(1, deadline)?[0];
            line.push(byte);
            if byte == b'\n' {
                if line == READY {
                    return Ok(worker);
                }
                line.clear();
            }
        }
        Err(io::ErrorKind::InvalidData.into())
    }

    fn query(&mut self, pid: u32, created: u64, deadline: Instant) -> io::Result<Vec<u8>> {
        if Instant::now() >= deadline {
            return Err(io::ErrorKind::TimedOut.into());
        }
        let mut request = [0; 12];
        request[..4].copy_from_slice(&pid.to_le_bytes());
        request[4..].copy_from_slice(&created.to_le_bytes());
        // The protocol has one outstanding 12-byte request, below pipe capacity.
        self.input.write_all(&request)?;
        let length = u32::from_le_bytes(self.receive(4, deadline)?.try_into().unwrap()) as usize;
        if length > MAX_PROBE_OUTPUT {
            return Err(io::ErrorKind::InvalidData.into());
        }
        self.receive(length, deadline)
    }

    fn receive(&mut self, length: usize, deadline: Instant) -> io::Result<Vec<u8>> {
        let mut bytes = vec![0; length];
        let mut read = 0;
        while read < length {
            if Instant::now() >= deadline {
                return Err(io::ErrorKind::TimedOut.into());
            }
            let mut available = 0;
            // SAFETY: the pipe is owned by this client, with only one reader.
            // Reading at most the available bytes cannot block on helper I/O.
            if unsafe {
                PeekNamedPipe(
                    self.output.as_raw_handle(),
                    ptr::null_mut(),
                    0,
                    ptr::null_mut(),
                    &mut available,
                    ptr::null_mut(),
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            if available == 0 {
                std::thread::sleep(POLL);
                continue;
            }
            let count = (available as usize).min(length - read);
            self.output.read_exact(&mut bytes[read..read + count])?;
            read += count;
        }
        Ok(bytes)
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn pipe() -> io::Result<(File, File)> {
    let mut reader = ptr::null_mut();
    let mut writer = ptr::null_mut();
    // SAFETY: valid out-pointers; null security attributes make both ends
    // non-inheritable. Only explicit duplicates below are passed to the child.
    unsafe {
        if CreatePipe(&mut reader, &mut writer, ptr::null(), 0) == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok((File::from_raw_handle(reader), File::from_raw_handle(writer)))
    }
}

fn inherited(file: &File) -> io::Result<OwnedHandle> {
    let mut copy = ptr::null_mut();
    // SAFETY: both process pseudo-handles are current; the returned duplicate is owned.
    unsafe {
        if DuplicateHandle(
            GetCurrentProcess(),
            file.as_raw_handle(),
            GetCurrentProcess(),
            &mut copy,
            0,
            1,
            DUPLICATE_SAME_ACCESS,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(OwnedHandle::from_raw_handle(copy))
    }
}

struct Attributes(Vec<usize>);

impl Drop for Attributes {
    fn drop(&mut self) {
        // SAFETY: construction only returns after successful initialization.
        unsafe { DeleteProcThreadAttributeList(self.0.as_mut_ptr().cast()) };
    }
}

fn spawn(args: &[String], files: [&File; 3]) -> io::Result<(OwnedHandle, OwnedHandle)> {
    let executable = std::env::current_exe()?;
    let application: Vec<_> = executable.as_os_str().encode_wide().chain(Some(0)).collect();
    // These are fixed internal flags and test selectors, never shell/user arguments.
    if args.iter().any(|arg| arg.is_empty() || arg.contains(['\0', '"', ' ', '\t'])) {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let mut command = vec![b'"' as u16];
    command.extend(executable.as_os_str().encode_wide());
    command.push(b'"' as u16);
    for arg in args {
        command.push(b' ' as u16);
        command.extend(arg.encode_utf16());
    }
    command.push(0);
    let inherited = [inherited(files[0])?, inherited(files[1])?, inherited(files[2])?];
    let handles: Vec<HANDLE> = inherited.iter().map(|handle| handle.as_raw_handle()).collect();
    let mut size = 0;
    // SAFETY: the first call obtains storage size. The aligned allocation, handles
    // and both command buffers remain live through CreateProcessW. HANDLE_LIST
    // excludes every other inheritable handle, including credentials and jobs.
    unsafe {
        InitializeProcThreadAttributeList(ptr::null_mut(), 1, 0, &mut size);
        if size == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut storage = vec![0usize; size.div_ceil(mem::size_of::<usize>())];
        if InitializeProcThreadAttributeList(storage.as_mut_ptr().cast(), 1, 0, &mut size) == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut attributes = Attributes(storage);
        let list = attributes.0.as_mut_ptr().cast();
        if UpdateProcThreadAttribute(
            list,
            0,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
            handles.as_ptr().cast(),
            mem::size_of_val(handles.as_slice()),
            ptr::null_mut(),
            ptr::null(),
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut startup: STARTUPINFOEXW = mem::zeroed();
        startup.StartupInfo.cb = mem::size_of::<STARTUPINFOEXW>() as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES | STARTF_FORCEOFFFEEDBACK;
        startup.StartupInfo.hStdInput = handles[0];
        startup.StartupInfo.hStdOutput = handles[1];
        startup.StartupInfo.hStdError = handles[2];
        startup.lpAttributeList = list;
        let mut process: PROCESS_INFORMATION = mem::zeroed();
        if CreateProcessW(
            application.as_ptr(),
            command.as_mut_ptr(),
            ptr::null(),
            ptr::null(),
            1,
            CREATE_SUSPENDED | CREATE_NO_WINDOW | EXTENDED_STARTUPINFO_PRESENT,
            ptr::null(),
            ptr::null(),
            &startup.StartupInfo,
            &mut process,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok((
            OwnedHandle::from_raw_handle(process.hProcess),
            OwnedHandle::from_raw_handle(process.hThread),
        ))
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod update_tests;
