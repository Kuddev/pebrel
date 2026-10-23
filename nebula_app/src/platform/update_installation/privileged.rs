//! Windows administrator update storage. No ordinary profile file grants authority.
use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
use std::os::windows::fs::{MetadataExt as _, OpenOptionsExt as _};
use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};
use std::path::{Component, Path, PathBuf, Prefix};
use std::sync::OnceLock;

use sha2::{Digest as _, Sha256};
use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo,
    SDDL_REVISION_1, SE_FILE_OBJECT, SetSecurityInfo,
};
use windows_sys::Win32::Security::*;
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::System::SystemServices::{
    ACCESS_ALLOWED_ACE_TYPE, FILE_PERSISTENT_ACLS, SYSTEM_MANDATORY_LABEL_ACE_TYPE,
    SYSTEM_MANDATORY_LABEL_NO_READ_UP, SYSTEM_MANDATORY_LABEL_NO_WRITE_UP,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows_sys::Win32::UI::Shell::{FOLDERID_ProgramData, SHGetKnownFolderPath};

const POLICY: &str = "O:BAG:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)S:(ML;OICI;NWNR;;;HI)";

struct Root {
    path: PathBuf,
    user: String,
    // 逐级保留真正的目录读取句柄，阻止祖先在路径校验后被重命名替换。
    _pins: Vec<File>,
}

static ROOT: OnceLock<Result<Root, String>> = OnceLock::new();

pub(super) fn directory(relative: &Path) -> io::Result<PathBuf> {
    if relative.components().any(|part| !matches!(part, Component::Normal(_))) {
        return Err(denied("Invalid administrator update subdirectory"));
    }
    let root = ROOT.get_or_init(|| Root::open().map_err(|error| error.to_string()));
    let root = root.as_ref().map_err(|error| io::Error::other(error.clone()))?;
    let mut identity = Sha256::new();
    identity.update(root.user.as_bytes());
    for path in [std::env::current_exe()?, nebula_settings::settings_dir()] {
        let path = super::canonical(&path)?;
        let units: Vec<_> = path.as_os_str().encode_wide().collect();
        identity.update((units.len() as u64).to_le_bytes());
        for unit in units {
            identity.update(unit.to_le_bytes());
        }
    }
    let identity: String = identity.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
    let mut path = root.path.join(identity);
    let descriptor = Descriptor::parse(POLICY)?;
    let mut pins = vec![secure_directory(&path, &descriptor)?];
    for component in relative.components() {
        path.push(component);
        pins.push(secure_directory(&path, &descriptor)?);
    }
    // 根不可被普通进程访问；子目录亦逐级核验，不修补或信任预先植入的对象。
    Ok(path)
}

impl Root {
    fn open() -> io::Result<Self> {
        let path = super::canonical(&program_data()?)?;
        persistent_acls(&path)?;
        let mut pins = pin_ancestors(&path)?;
        let path = path.join("PebrelAdminUpdates");
        pins.push(secure_directory(&path, &Descriptor::parse(POLICY)?)?);
        Ok(Self { path, user: current_user()?, _pins: pins })
    }
}

fn denied(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

fn program_data() -> io::Result<PathBuf> {
    let mut text = std::ptr::null_mut();
    // SAFETY: the shell allocates a terminated string; CoTaskMemFree owns its release.
    unsafe {
        let result =
            SHGetKnownFolderPath(&FOLDERID_ProgramData, 0, std::ptr::null_mut(), &mut text);
        let path = if result >= 0 && !text.is_null() {
            let mut length = 0;
            while *text.add(length) != 0 {
                length += 1;
            }
            Ok(PathBuf::from(std::ffi::OsString::from_wide(std::slice::from_raw_parts(
                text, length,
            ))))
        } else {
            Err(io::Error::other(format!("Could not locate ProgramData: {result:#x}")))
        };
        CoTaskMemFree(text.cast());
        path
    }
}

fn current_user() -> io::Result<String> {
    // SAFETY: aligned token storage outlives SID conversion; both allocations are released.
    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        let token = OwnedHandle::from_raw_handle(token);
        let mut length = 0;
        GetTokenInformation(token.as_raw_handle(), TokenUser, std::ptr::null_mut(), 0, &mut length);
        if length < std::mem::size_of::<TOKEN_USER>() as u32 {
            return Err(io::Error::last_os_error());
        }
        let mut buffer = vec![0usize; (length as usize).div_ceil(std::mem::size_of::<usize>())];
        if GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            buffer.as_mut_ptr().cast(),
            length,
            &mut length,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut text = std::ptr::null_mut();
        if ConvertSidToStringSidW((*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid, &mut text) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut length = 0;
        while *text.add(length) != 0 {
            length += 1;
        }
        let result = String::from_utf16_lossy(std::slice::from_raw_parts(text, length));
        LocalFree(text.cast());
        Ok(result)
    }
}

fn persistent_acls(path: &Path) -> io::Result<()> {
    let mut volume = vec![0u16; 32768];
    let mut flags = 0;
    // SAFETY: all output buffers remain live and match their declared capacities.
    unsafe {
        if GetVolumePathNameW(wide(path).as_ptr(), volume.as_mut_ptr(), volume.len() as u32) == 0
            || GetVolumeInformationW(
                volume.as_ptr(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut flags,
                std::ptr::null_mut(),
                0,
            ) == 0
        {
            return Err(io::Error::last_os_error());
        }
    }
    if flags & FILE_PERSISTENT_ACLS == 0 {
        return Err(denied("Administrator updates require a filesystem with persistent ACLs"));
    }
    Ok(())
}

fn pin_ancestors(path: &Path) -> io::Result<Vec<File>> {
    if !matches!(path.components().next(), Some(Component::Prefix(prefix))
        if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)))
        || !path.is_absolute()
    {
        return Err(denied("Administrator update storage must be on a local volume"));
    }
    path.ancestors()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|path| pin_directory(path, false))
        .collect()
}

fn pin_directory(path: &Path, set_label: bool) -> io::Result<File> {
    // 属性访问本身不建立完整共享锁；GENERIC_READ 包含实际的 FILE_LIST_DIRECTORY。
    let file = OpenOptions::new()
        .read(true)
        .access_mode(FILE_GENERIC_READ | if set_label { WRITE_OWNER } else { 0 })
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let attributes = file.metadata()?.file_attributes();
    if attributes & FILE_ATTRIBUTE_DIRECTORY == 0 || attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err(denied("Administrator update storage cannot contain reparse points"));
    }
    Ok(file)
}

struct Descriptor(PSECURITY_DESCRIPTOR);

impl Descriptor {
    fn parse(value: &str) -> io::Result<Self> {
        let text: Vec<_> = value.encode_utf16().chain(Some(0)).collect();
        let mut descriptor = std::ptr::null_mut();
        // SAFETY: the input is terminated; the returned LocalAlloc buffer is owned by Self.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                text.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(descriptor))
    }
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        // SAFETY: this descriptor was allocated by a Windows security API.
        unsafe {
            LocalFree(self.0);
        }
    }
}

fn secure_directory(path: &Path, descriptor: &Descriptor) -> io::Result<File> {
    // 创建完整 SACL 需要 SeSecurityPrivilege；先原子建立 BA/SY-only DACL，
    // 再用只需 WRITE_OWNER 的 LABEL_SECURITY_INFORMATION 设置标签，全程不向普通进程开放。
    let access = Descriptor::parse(POLICY.split_once("S:").expect("policy label").0)?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: access.0,
        bInheritHandle: 0,
    };
    // SAFETY: both path and descriptor live through directory creation.
    let created = unsafe { CreateDirectoryW(wide(path).as_ptr(), &attributes) } != 0;
    if !created {
        let error = unsafe { GetLastError() };
        if error != ERROR_ALREADY_EXISTS {
            return Err(io::Error::from_raw_os_error(error as i32));
        }
    }
    let file = pin_directory(path, created)?;
    if created {
        // 先核对实际对象，不能替在创建与打开之间被置换的目录修权限。
        if !has_access_policy(&security_descriptor(&file)?) {
            return Err(denied("New administrator update directory has an untrusted owner or ACL"));
        }
        let mut present = 0;
        let mut defaulted = 0;
        let mut sacl = std::ptr::null_mut();
        // SAFETY: the trusted parsed descriptor owns the label ACL until SetSecurityInfo returns.
        unsafe {
            if GetSecurityDescriptorSacl(descriptor.0, &mut present, &mut sacl, &mut defaulted) == 0
            {
                return Err(io::Error::last_os_error());
            }
            let error = SetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                LABEL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                sacl,
            );
            if error != 0 {
                return Err(io::Error::other(format!(
                    "Could not label administrator update directory: {}",
                    io::Error::from_raw_os_error(error as i32)
                )));
            }
        }
    }
    let actual = security_descriptor(&file)?;
    if !has_policy(&actual) {
        return Err(denied(&format!(
            "Administrator update directory has an untrusted owner or permissions: {}",
            path.display()
        )));
    }
    Ok(file)
}

fn security_descriptor(file: &File) -> io::Result<Descriptor> {
    let mut actual = std::ptr::null_mut();
    // LABEL_SECURITY_INFORMATION needs READ_CONTROL, not access to the full auditing SACL.
    // SAFETY: the handle is live; the returned descriptor owns all queried SID/ACL pointers.
    let error = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION | LABEL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut actual,
        )
    };
    if error != 0 {
        if !actual.is_null() {
            unsafe {
                LocalFree(actual);
            }
        }
        return Err(io::Error::from_raw_os_error(error as i32));
    }
    Ok(Descriptor(actual))
}

fn has_access_policy(descriptor: &Descriptor) -> bool {
    // SAFETY: descriptors come from the kernel or the SDDL parser. GetAce validates each index.
    unsafe {
        let mut owner = std::ptr::null_mut();
        let mut defaulted = 0;
        let mut control = 0;
        let mut revision = 0;
        let mut present = 0;
        let mut dacl = std::ptr::null_mut();
        if GetSecurityDescriptorOwner(descriptor.0, &mut owner, &mut defaulted) == 0
            || owner.is_null()
            || (IsWellKnownSid(owner, WinBuiltinAdministratorsSid) == 0
                && IsWellKnownSid(owner, WinLocalSystemSid) == 0)
            || GetSecurityDescriptorControl(descriptor.0, &mut control, &mut revision) == 0
            || control & SE_DACL_PROTECTED == 0
            || GetSecurityDescriptorDacl(descriptor.0, &mut present, &mut dacl, &mut defaulted) == 0
            || present == 0
            || dacl.is_null()
            || IsValidAcl(dacl) == 0
            || (*dacl).AceCount != 2
        {
            return false;
        }
        let mut administrators = false;
        let mut system = false;
        for index in 0..2 {
            let Some(ace) = access_ace(dacl, index, ACCESS_ALLOWED_ACE_TYPE) else { return false };
            if (*ace).Mask != FILE_ALL_ACCESS {
                return false;
            }
            let sid = std::ptr::addr_of_mut!((*ace).SidStart).cast();
            administrators |= IsWellKnownSid(sid, WinBuiltinAdministratorsSid) != 0;
            system |= IsWellKnownSid(sid, WinLocalSystemSid) != 0;
        }
        administrators && system
    }
}

fn has_policy(descriptor: &Descriptor) -> bool {
    if !has_access_policy(descriptor) {
        return false;
    }
    // SAFETY: the kernel/SDDL descriptor owns the ACL inspected through GetAce.
    unsafe {
        let mut present = 0;
        let mut defaulted = 0;
        let mut sacl = std::ptr::null_mut();
        if GetSecurityDescriptorSacl(descriptor.0, &mut present, &mut sacl, &mut defaulted) == 0
            || present == 0
            || sacl.is_null()
            || IsValidAcl(sacl) == 0
            || (*sacl).AceCount != 1
        {
            return false;
        }
        let Some(label) = access_ace(sacl, 0, SYSTEM_MANDATORY_LABEL_ACE_TYPE) else {
            return false;
        };
        let mask = SYSTEM_MANDATORY_LABEL_NO_READ_UP | SYSTEM_MANDATORY_LABEL_NO_WRITE_UP;
        (*label).Mask & mask == mask
            && IsWellKnownSid(std::ptr::addr_of_mut!((*label).SidStart).cast(), WinHighLabelSid)
                != 0
    }
}

unsafe fn access_ace(acl: *const ACL, index: u32, kind: u32) -> Option<*mut ACCESS_ALLOWED_ACE> {
    let mut raw = std::ptr::null_mut();
    // SAFETY: caller holds a valid Windows ACL. Both accepted ACE layouts start with header/mask/SID.
    unsafe {
        if GetAce(acl, index, &mut raw) == 0 || raw.is_null() {
            return None;
        }
        let ace = raw.cast::<ACCESS_ALLOWED_ACE>();
        let header = &(*ace).Header;
        let inherited =
            if kind == SYSTEM_MANDATORY_LABEL_ACE_TYPE { INHERITED_ACE as u8 } else { 0 };
        (u32::from(header.AceType) == kind
            && header.AceFlags & !inherited == (OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE) as u8
            && usize::from(header.AceSize) >= std::mem::size_of::<ACCESS_ALLOWED_ACE>())
        .then_some(ace)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_directory_policy_rejects_user_grants_owners_and_lower_integrity() {
        assert!(has_policy(&Descriptor::parse(POLICY).unwrap()));
        for policy in [
            POLICY.replace("O:BA", "O:BU"),
            POLICY.replace("D:P", "D:"),
            POLICY.replace(";;;BA)", ";;;BU)"),
            POLICY.replace("NWNR", "NW"),
            POLICY.replace(";;;HI)", ";;;ME)"),
            POLICY.replace("OICI", ""),
        ] {
            assert!(!has_policy(&Descriptor::parse(&policy).unwrap()), "{policy}");
        }
    }

    #[test]
    fn directory_read_handles_pin_names_and_reject_untrusted_existing_storage() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("directory");
        std::fs::create_dir(&path).unwrap();
        let file = pin_directory(&path, false).unwrap();
        assert!(std::fs::rename(&path, root.path().join("moved")).is_err());
        assert!(secure_directory(&path, &Descriptor::parse(POLICY).unwrap()).is_err());
        drop(file);
        std::fs::rename(&path, root.path().join("moved")).unwrap();
    }

    #[test]
    fn protected_directory_creation_matches_the_actual_token_and_files_inherit_integrity() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("private");
        let policy = Descriptor::parse(POLICY).unwrap();
        let directory = secure_directory(&path, &policy);
        if !crate::platform::elevation::is_elevated().unwrap() {
            assert!(directory.is_err(), "ordinary tokens cannot own an administrator directory");
            assert!(!path.exists());
            return;
        }
        let _directory = directory.unwrap();
        let child = path.join("child");
        let _child = secure_directory(&child, &policy).unwrap();
        let payload = child.join("workspace.json");
        std::fs::write(&payload, b"{}").unwrap();
        let descriptor = security_descriptor(&File::open(payload).unwrap()).unwrap();
        // SAFETY: the kernel-produced descriptor owns the ACL and the ACE checked below.
        unsafe {
            let mut present = 0;
            let mut defaulted = 0;
            let mut sacl = std::ptr::null_mut();
            assert_ne!(
                GetSecurityDescriptorSacl(descriptor.0, &mut present, &mut sacl, &mut defaulted),
                0
            );
            assert_ne!(present, 0);
            assert!(!sacl.is_null());
            assert_eq!((*sacl).AceCount, 1);
            let mut raw = std::ptr::null_mut();
            assert_ne!(GetAce(sacl, 0, &mut raw), 0);
            let label = &*raw.cast::<SYSTEM_MANDATORY_LABEL_ACE>();
            assert_eq!(u32::from(label.Header.AceType), SYSTEM_MANDATORY_LABEL_ACE_TYPE);
            let required = SYSTEM_MANDATORY_LABEL_NO_READ_UP | SYSTEM_MANDATORY_LABEL_NO_WRITE_UP;
            assert_eq!(label.Mask & required, required);
            assert_ne!(
                IsWellKnownSid(
                    std::ptr::addr_of!(label.SidStart).cast_mut().cast(),
                    WinHighLabelSid
                ),
                0
            );
        }
    }
}
