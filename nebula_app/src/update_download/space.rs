//! Conservative Windows update reservations, combined by volume (including mounts).
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

const RESERVE_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Debug)]
pub(super) enum Error {
    Query { path: PathBuf, source: io::Error },
    Shortage { path: PathBuf, required: u64, available: u64 },
}

pub(super) fn check(requests: &[(&Path, u64)]) -> Result<(), Error> {
    #[cfg(windows)]
    return check_with(requests, volume_space);
    #[cfg(not(windows))]
    {
        // This change covers the Windows ZIP handoff; other native adapters retain
        // their existing preparation checks.
        let _ = requests;
        Ok(())
    }
}

fn check_with(
    requests: &[(&Path, u64)],
    query: impl Fn(&Path) -> io::Result<(String, u64)>,
) -> Result<(), Error> {
    let mut volumes: BTreeMap<String, (PathBuf, u64, u64)> = BTreeMap::new();
    for (path, bytes) in requests {
        let (volume, available) =
            query(path).map_err(|source| Error::Query { path: path.to_path_buf(), source })?;
        let entry = volumes.entry(volume).or_insert((path.to_path_buf(), RESERVE_BYTES, available));
        entry.1 = entry.1.checked_add(*bytes).ok_or_else(|| Error::Query {
            path: path.to_path_buf(),
            source: io::Error::other("Update reservation overflow"),
        })?;
        entry.2 = entry.2.min(available);
    }
    for (_, (path, required, available)) in volumes {
        if required > available {
            return Err(Error::Shortage { path, required, available });
        }
    }
    Ok(())
}

#[cfg(windows)]
fn volume_space(path: &Path) -> io::Result<(String, u64)> {
    use std::os::windows::ffi::OsStrExt as _;
    use windows_sys::Win32::Storage::FileSystem::{
        GetDiskFreeSpaceExW, GetVolumeNameForVolumeMountPointW, GetVolumePathNameW,
    };
    let path = crate::platform::update_installation::canonical(path)?;
    let wide: Vec<_> =
        path.as_os_str().encode_wide().chain(Some(b'\\' as u16)).chain(Some(0)).collect();
    let mut available = 0;
    let mut root = vec![0; 32768];
    // SAFETY: paths are NUL-terminated; output pointers/buffers remain live.
    if unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut available,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    } == 0
        || unsafe { GetVolumePathNameW(wide.as_ptr(), root.as_mut_ptr(), root.len() as u32) } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let mut identity = vec![0; 64];
    if unsafe {
        GetVolumeNameForVolumeMountPointW(
            root.as_ptr(),
            identity.as_mut_ptr(),
            identity.len() as u32,
        )
    } == 0
    {
        // UNC shares have no local volume GUID; the resolved share root is their key.
        let end = root.iter().position(|value| *value == 0).unwrap_or(root.len());
        let root = String::from_utf16_lossy(&root[..end]);
        if !root.starts_with(r"\\") {
            return Err(io::Error::last_os_error());
        }
        return Ok((root.to_lowercase(), available));
    }
    let end = identity.iter().position(|value| *value == 0).unwrap_or(identity.len());
    Ok((String::from_utf16_lossy(&identity[..end]).to_lowercase(), available))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_volume_reservations_are_added_with_one_margin() {
        let paths = [(Path::new("download"), 40), (Path::new("install"), 60)];
        assert!(check_with(&paths, |_| Ok(("same".into(), RESERVE_BYTES + 100))).is_ok());
        assert!(matches!(check_with(&paths, |_| Ok(("same".into(), RESERVE_BYTES + 99))),
            Err(Error::Shortage { required, .. }) if required == RESERVE_BYTES + 100));
    }

    #[test]
    fn different_volumes_and_quota_changes_are_checked_separately() {
        let paths = [(Path::new("download"), 40), (Path::new("install"), 60)];
        assert!(
            check_with(&paths, |path| Ok((path.to_string_lossy().into(), RESERVE_BYTES + 60)))
                .is_ok()
        );
        assert!(
            matches!(check_with(&paths, |path| Ok((path.to_string_lossy().into(), RESERVE_BYTES + 50))),
            Err(Error::Shortage { path, .. }) if path == Path::new("install"))
        );
        assert!(
            matches!(check_with(&paths, |path| Ok(("same".into(), RESERVE_BYTES + if path == Path::new("install") { 90 } else { 100 }))),
            Err(Error::Shortage { available, .. }) if available == RESERVE_BYTES + 90)
        );
    }

    #[test]
    fn query_failures_and_overflow_never_pass_preparation() {
        assert!(matches!(
            check_with(&[(Path::new("missing"), 1)], |_| Err(io::Error::other("denied"))),
            Err(Error::Query { .. })
        ));
        assert!(matches!(
            check_with(&[(Path::new("huge"), u64::MAX)], |_| Ok(("same".into(), u64::MAX))),
            Err(Error::Query { .. })
        ));
    }

    #[test]
    #[cfg(windows)]
    fn native_query_resolves_existing_directories_and_rejects_missing_paths() {
        let root = tempfile::tempdir().unwrap();
        let (identity, free) = volume_space(root.path()).unwrap();
        assert!(!identity.is_empty());
        assert!(free > 0);
        assert!(volume_space(&root.path().join("missing")).is_err());
    }
}
