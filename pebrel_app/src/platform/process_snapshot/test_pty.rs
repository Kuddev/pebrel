//! Give a fixture process a controlling terminal without changing its pipes.

use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd as _, FromRawFd as _};
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::OpenOptionsExt as _;
use std::os::unix::process::CommandExt as _;
use std::process::Command;

pub(crate) fn attach(command: &mut Command) -> io::Result<File> {
    // SAFETY: a successful posix_openpt returns a new owned descriptor.
    let fd = unsafe { libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let master = unsafe { File::from_raw_fd(fd) };
    let mut name = [0; 128];
    // SAFETY: fd stays open and the path buffer is writable for its full capacity.
    if unsafe { libc::grantpt(fd) } != 0 || unsafe { libc::unlockpt(fd) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let status = unsafe { libc::ptsname_r(fd, name.as_mut_ptr(), name.len()) };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status));
    }
    let path =
        std::ffi::OsStr::from_bytes(unsafe { std::ffi::CStr::from_ptr(name.as_ptr()) }.to_bytes());
    let slave = File::options().read(true).write(true).custom_flags(libc::O_NOCTTY).open(path)?;
    // SAFETY: the child only makes async-signal-safe system calls before exec.
    unsafe {
        command.pre_exec(move || {
            if libc::setsid() < 0 || libc::ioctl(slave.as_raw_fd(), libc::TIOCSCTTY, 0) < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    Ok(master)
}
