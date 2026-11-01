//! Trust-file replacement preserves the target DACL and never ignores ACL merge errors.
use std::io;
use std::os::windows::ffi::OsStrExt as _;
use std::path::Path;
use windows_sys::Win32::Storage::FileSystem::ReplaceFileW;

pub(crate) fn replace(source: &Path, destination: &Path) -> io::Result<()> {
    let backup = tempfile::Builder::new()
        .prefix(".known-hosts-backup-")
        .tempfile_in(destination.parent().ok_or_else(|| io::Error::other("No trust directory"))?)?
        .into_temp_path();
    std::fs::remove_file(&backup)?;
    let wide = |path: &Path| path.as_os_str().encode_wide().chain(Some(0)).collect::<Vec<_>>();
    let original = wide(destination);
    let replacement = wide(source);
    let saved = wide(&backup);
    // No IGNORE_ACL_ERRORS / IGNORE_MERGE_ERRORS: permission preservation is
    // mandatory. A backup is required because error 1176 without one can remove
    // the destination, and 1177 can move it to the backup before returning failure.
    let success = unsafe {
        ReplaceFileW(
            original.as_ptr(),
            replacement.as_ptr(),
            saved.as_ptr(),
            0,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if success != 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if backup.exists() {
        // An exclusive hard-link creation restores the original file identity
        // and DACL if the destination was moved away, without overwriting any
        // concurrent writer. If recovery cannot complete, retain the backup.
        if !destination.exists() && std::fs::hard_link(&backup, destination).is_ok() {
            return Err(error);
        }
        let preserved = backup.keep().map_err(io::Error::other)?;
        return Err(io::Error::new(
            error.kind(),
            format!("{error}; original trust file retained at {}", preserved.display()),
        ));
    }
    Err(error)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acl(path: &Path, setup: bool) -> String {
        let script = if setup {
            r#"$ErrorActionPreference='Stop'; $p=$env:PEBREL_KNOWN_HOSTS; $parent=Split-Path $p; $a=Get-Acl $parent; $rule=[System.Security.AccessControl.FileSystemAccessRule]::new([System.Security.Principal.SecurityIdentifier]::new('S-1-1-0'),'Modify','ContainerInherit,ObjectInherit','None','Allow'); $a.AddAccessRule($rule); Set-Acl $parent $a; $a=Get-Acl $p; $a.SetAccessRuleProtection($true,$false); foreach($rule in @($a.Access)) { $a.RemoveAccessRuleSpecific($rule) }; $user=[System.Security.Principal.WindowsIdentity]::GetCurrent().User; $a.AddAccessRule([System.Security.AccessControl.FileSystemAccessRule]::new($user,'FullControl','Allow')); Set-Acl $p $a; $a=Get-Acl $p; $parentAcl=Get-Acl $parent; if($a.GetSecurityDescriptorSddlForm('Access') -eq $parentAcl.GetSecurityDescriptorSddlForm('Access')) { throw 'Target must have a stricter DACL than its parent' }; $a.GetSecurityDescriptorSddlForm('Access')"#
        } else {
            r#"$ErrorActionPreference='Stop'; (Get-Acl $env:PEBREL_KNOWN_HOSTS).GetSecurityDescriptorSddlForm('Access')"#
        };
        // Indirect launches from pwsh inherit incompatible PowerShell 7 modules.
        // Let Windows PowerShell rebuild its own module paths.
        let output = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .env_remove("PSModulePath")
            .env("PEBREL_KNOWN_HOSTS", path)
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    #[test]
    fn changed_trust_file_keeps_its_stricter_windows_dacl() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("known_hosts");
        let replacement = directory.path().join("replacement");
        std::fs::write(&path, "original trusted key\n").unwrap();
        std::fs::write(&replacement, "new trusted key\n").unwrap();
        let before = acl(&path, true);
        replace(&replacement, &path).unwrap();
        assert_eq!(
            acl(&path, false),
            before,
            "Parent's broader inherited DACL must not replace target DACL"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new trusted key\n");
    }

    #[test]
    fn locked_destination_failure_retains_original_trust_file() {
        use std::os::windows::fs::OpenOptionsExt as _;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("known_hosts");
        let replacement = directory.path().join("replacement");
        std::fs::write(&path, "original trust").unwrap();
        std::fs::write(&replacement, "changed trust").unwrap();
        let locked = std::fs::OpenOptions::new().read(true).share_mode(0).open(&path).unwrap();
        assert!(replace(&replacement, &path).is_err());
        drop(locked);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "original trust");
        assert_eq!(std::fs::read_to_string(&replacement).unwrap(), "changed trust");
    }
}
