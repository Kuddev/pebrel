//! 凭据临时文件权限的原生边界；原子写入与回滚仍由 atomic_file 持有。
use std::fs::OpenOptions;

pub(crate) fn restrict_creation(options: &mut OpenOptions) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = options;
}

#[cfg(test)]
pub(crate) fn assert_private(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(path).unwrap().permissions().mode() & 0o777, 0o600);
    }
    #[cfg(not(unix))]
    assert!(path.is_file());
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn private_write_restricts_new_and_replaced_files() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.json");
        crate::atomic_file::write_private(&path, b"first").unwrap();
        assert_private(&path);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        crate::atomic_file::write_private(&path, b"replacement").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"replacement");
        assert_private(&path);
    }
}
