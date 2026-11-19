//! The foreign caller owns valid buffers for the duration of each blocking call.
use crate::session::{self, Failure, Geometry, Open, Options, Result, runtime};
use std::{
    ffi::{CStr, CString, c_char},
    panic::{AssertUnwindSafe, catch_unwind},
    ptr, slice,
};
use zeroize::Zeroizing;
mod link;

type Status = *mut c_char;
const INVALID: Failure = Failure("INVALID_INPUT");

fn status(call: impl FnOnce() -> Result<()>) -> Status {
    let code = match catch_unwind(AssertUnwindSafe(call)) {
        Ok(Ok(())) => return ptr::null_mut(),
        Ok(Err(failure)) => failure.0,
        Err(_) => "INTERNAL",
    };
    CString::new(code).unwrap().into_raw()
}

#[unsafe(no_mangle)]
pub extern "C" fn pebrel_string_free(value: Status) {
    if !value.is_null() {
        drop(unsafe { CString::from_raw(value) });
    }
}

fn text<'a>(value: *const c_char, limit: usize) -> Result<&'a str> {
    if value.is_null() {
        return Err(INVALID);
    }
    let bytes = unsafe { CStr::from_ptr(value) };
    if bytes.to_bytes().len() > limit {
        return Err(INVALID);
    }
    bytes.to_str().map_err(|_| INVALID)
}

fn output<'a, T>(value: *mut T) -> Result<&'a mut T> {
    unsafe { value.as_mut().ok_or(INVALID) }
}

fn string_out(target: &mut Status, value: String) -> Result<()> {
    *target = CString::new(value).map_err(|_| Failure("INVALID_INPUT"))?.into_raw();
    Ok(())
}

#[repr(C)]
pub struct Bytes {
    data: *const u8,
    len: usize,
}
impl Bytes {
    fn get(&self, max: usize) -> Result<&[u8]> {
        if self.len == 0 {
            return Ok(&[]);
        }
        if self.data.is_null() || self.len > max {
            return Err(INVALID);
        }
        Ok(unsafe { slice::from_raw_parts(self.data, self.len) })
    }
}

#[repr(C)]
pub struct Size {
    columns: u32,
    rows: u32,
    width: u32,
    height: u32,
}
impl Size {
    fn geometry(self) -> Result<Geometry> {
        if !(1..=4096).contains(&self.columns) || !(1..=4096).contains(&self.rows) {
            return Err(INVALID);
        }
        Ok(Geometry {
            columns: self.columns,
            rows: self.rows,
            width: self.width,
            height: self.height,
        })
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn pebrel_ssh_create(
    host: *const c_char,
    port: u16,
    user: *const c_char,
    password: Bytes,
    fingerprint: *const c_char,
    key: Bytes,
    out: *mut i64,
) -> Status {
    status(|| {
        let out = output(out)?;
        if port == 0 {
            return Err(INVALID);
        }
        *out = session::start(Options {
            host: text(host, 1024)?.to_owned(),
            port,
            user: text(user, 1024)?.to_owned(),
            password: Zeroizing::new(password.get(4096)?.to_vec()),
            private_key: Zeroizing::new(key.get(65536)?.to_vec()),
            fingerprint: text(fingerprint, 256)?.to_owned(),
        })?;
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn pebrel_ssh_next_event(id: i64, out: *mut Status) -> Status {
    status(|| string_out(output(out)?, runtime().block_on(session::get(id)?.event())?))
}
#[unsafe(no_mangle)]
pub extern "C" fn pebrel_ssh_answer_host_key(id: i64, accepted: bool) -> Status {
    status(|| session::get(id)?.answer(accepted))
}
#[unsafe(no_mangle)]
pub extern "C" fn pebrel_ssh_open_shell(id: i64, size: Size) -> Status {
    status(|| runtime().block_on(session::get(id)?.open(Open::Shell(size.geometry()?))))
}
#[unsafe(no_mangle)]
pub extern "C" fn pebrel_ssh_open_exec(id: i64, command: *const c_char) -> Status {
    status(|| {
        runtime().block_on(session::get(id)?.open(Open::Exec(text(command, 8192)?.to_owned())))
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn pebrel_ssh_open_pty_exec(id: i64, command: *const c_char, size: Size) -> Status {
    status(|| {
        let command = text(command, 8192)?;
        if command.is_empty() || command.chars().any(char::is_control) {
            return Err(INVALID);
        }
        runtime()
            .block_on(session::get(id)?.open(Open::PtyExec(size.geometry()?, command.to_owned())))
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn pebrel_ssh_query(id: i64, command: *const c_char, out: *mut Status) -> Status {
    status(|| {
        string_out(
            output(out)?,
            runtime().block_on(session::get(id)?.query(text(command, 8192)?.to_owned()))?,
        )
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn pebrel_ssh_read(
    id: i64,
    stderr: bool,
    bytes: *mut u8,
    capacity: usize,
    count: *mut usize,
) -> Status {
    status(|| {
        let count = output(count)?;
        if bytes.is_null() || !(1..=16384).contains(&capacity) {
            return Err(INVALID);
        }
        let received = runtime().block_on(session::get(id)?.read(stderr, capacity))?;
        unsafe {
            ptr::copy_nonoverlapping(received.as_ptr(), bytes, received.len());
        }
        *count = received.len();
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn pebrel_ssh_write(id: i64, bytes: Bytes) -> Status {
    status(|| {
        if bytes.len == 0 {
            return Err(INVALID);
        }
        runtime().block_on(session::get(id)?.write(bytes.get(8192)?.to_vec()))
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn pebrel_ssh_resize(id: i64, size: Size) -> Status {
    status(|| runtime().block_on(session::get(id)?.resize(size.geometry()?)))
}
#[unsafe(no_mangle)]
pub extern "C" fn pebrel_ssh_await_exit(id: i64, out: *mut i32) -> Status {
    status(|| {
        *output(out)? = runtime().block_on(session::get(id)?.exit())?;
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn pebrel_ssh_sftp(id: i64, request: *const c_char, out: *mut Status) -> Status {
    status(|| {
        let request =
            crate::sftp::parse(text(request, crate::sftp::BATCH.div_ceil(3) * 4 + 8192)?)?;
        string_out(output(out)?, runtime().block_on(session::get(id)?.sftp(request))?.to_string())
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn pebrel_ssh_close(id: i64) -> Status {
    status(|| {
        session::close(id);
        Ok(())
    })
}
