//! OpenRC has no systemd DynamicUser/LoadCredential. Read keys and bind first,
//! then permanently drop to nobody before creating threads or accepting peers.
use std::io;

pub(super) fn drop_root() -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        // This function is called only in the single-threaded startup path.
        // getpwnam's borrowed record is consumed before any other libc lookup.
        unsafe {
            if libc::geteuid() != 0 {
                return Err(io::Error::other("root_required"));
            }
            let account = libc::getpwnam(c"nobody".as_ptr());
            if account.is_null() {
                return Err(io::Error::other("unprivileged_account_required"));
            }
            let uid = (*account).pw_uid;
            let gid = (*account).pw_gid;
            if uid == 0 || gid == 0 {
                return Err(io::Error::other("unprivileged_account_required"));
            }
            if libc::setgroups(0, std::ptr::null()) != 0
                || libc::setgid(gid) != 0
                || libc::setuid(uid) != 0
                || libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0
                || libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) != 0
                || libc::geteuid() != uid
                || libc::getegid() != gid
            {
                return Err(io::Error::other("privilege_drop_failed"));
            }
        }
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(io::Error::other("linux_required"))
    }
}
