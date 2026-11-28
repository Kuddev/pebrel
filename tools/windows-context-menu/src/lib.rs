//! Windows 11 Explorer command, adapted from the user's PebrelContextMenu helper.
#![cfg(windows)]

use std::cell::RefCell;
use std::ffi::{c_void, OsString};
use std::os::windows::{
    ffi::{OsStrExt, OsStringExt},
    process::CommandExt,
};
use std::path::{Path, PathBuf};
use std::process::Command;

use windows::core::{implement, Error, IUnknown, Interface, GUID, HRESULT, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    BOOL, CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_FAIL, E_INVALIDARG, E_NOTIMPL,
    E_POINTER, HMODULE, S_FALSE,
};
use windows::Win32::Globalization::GetUserDefaultUILanguage;
use windows::Win32::System::Com::{
    CoTaskMemFree, IBindCtx, IClassFactory, IClassFactory_Impl, IServiceProvider,
};
use windows::Win32::System::LibraryLoader::{
    GetModuleFileNameW, GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
    GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
};
use windows::Win32::System::Ole::{IObjectWithSite, IObjectWithSite_Impl};
use windows::Win32::UI::Shell::{
    IEnumExplorerCommand, IExplorerCommand, IExplorerCommand_Impl, IFolderView, IPersistFolder2,
    IShellItem, IShellItemArray, SHCreateItemFromIDList, SHStrDupW, ECF_DEFAULT, ECS_ENABLED,
    SIGDN_FILESYSPATH,
};

include!(concat!(env!("OUT_DIR"), "/titles.rs"));
const CLSID_PEBREL_COMMAND: GUID = GUID::from_u128(0x209e5492_2c19_45c0_ae7e_a4beffd09073);
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

enum Message {
    OpenInPebrel,
}
impl Message {
    fn text(self, language: u16) -> PCWSTR {
        PCWSTR(
            if language & 0x3ff == 0x04 { OPEN_IN_PEBREL_ZH } else { OPEN_IN_PEBREL_EN }.as_ptr(),
        )
    }
}

fn executable_from_module(module: &Path) -> windows::core::Result<PathBuf> {
    // Payload: <installation>/context-menu/pebrel_context_menu.dll.
    let root = module.parent().and_then(Path::parent).ok_or_else(|| Error::from(E_FAIL))?;
    Ok(root.join("pebrel.exe"))
}

fn executable() -> windows::core::Result<PathBuf> {
    let mut module = HMODULE::default();
    let mut path = vec![0u16; 32_768];
    unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            PCWSTR(DllGetClassObject as *const () as *const u16),
            &mut module,
        )?;
        let length = GetModuleFileNameW(module, &mut path) as usize;
        if length == 0 || length >= path.len() {
            return Err(Error::from_win32());
        }
        executable_from_module(Path::new(&OsString::from_wide(&path[..length])))
    }
}

fn folder_path(item: &IShellItem) -> windows::core::Result<PathBuf> {
    unsafe {
        let raw = item.GetDisplayName(SIGDN_FILESYSPATH)?;
        // Shell strings use CoTaskMemAlloc. Preserve UTF-16 paths losslessly.
        let path = PathBuf::from(OsString::from_wide(raw.as_wide()));
        CoTaskMemFree(Some(raw.0.cast()));
        Ok(path)
    }
}

fn launch(executable: &Path, directory: &Path) -> windows::core::Result<()> {
    if !directory.is_dir() {
        return Err(E_INVALIDARG.into());
    }
    Command::new(executable)
        .arg("--gpui")
        .arg("--working-directory")
        .arg(directory)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map(|_| ())
        .map_err(|error| Error::from(HRESULT::from_win32(error.raw_os_error().unwrap_or(1) as u32)))
}

#[implement(IExplorerCommand, IObjectWithSite)]
#[derive(Default)]
struct PebrelCommand {
    site: RefCell<Option<IUnknown>>,
}

impl IExplorerCommand_Impl for PebrelCommand_Impl {
    fn GetTitle(&self, _items: Option<&IShellItemArray>) -> windows::core::Result<PWSTR> {
        unsafe { SHStrDupW(Message::OpenInPebrel.text(GetUserDefaultUILanguage())) }
    }

    fn GetIcon(&self, _items: Option<&IShellItemArray>) -> windows::core::Result<PWSTR> {
        let mut icon: Vec<u16> = executable()?.as_os_str().encode_wide().collect();
        icon.extend(",0".encode_utf16());
        icon.push(0);
        unsafe { SHStrDupW(PCWSTR(icon.as_ptr())) }
    }

    fn GetToolTip(&self, _items: Option<&IShellItemArray>) -> windows::core::Result<PWSTR> {
        Err(E_NOTIMPL.into())
    }

    fn GetCanonicalName(&self) -> windows::core::Result<GUID> {
        Ok(CLSID_PEBREL_COMMAND)
    }

    fn GetState(
        &self,
        _items: Option<&IShellItemArray>,
        _slow: BOOL,
    ) -> windows::core::Result<u32> {
        Ok(ECS_ENABLED.0 as u32)
    }

    fn Invoke(
        &self,
        items: Option<&IShellItemArray>,
        _bind: Option<&IBindCtx>,
    ) -> windows::core::Result<()> {
        let executable = executable()?;
        unsafe {
            if let Some(items) = items {
                let count = items.GetCount()?;
                if count > 0 {
                    for index in 0..count {
                        launch(&executable, &folder_path(&items.GetItemAt(index)?)?)?;
                    }
                    return Ok(());
                }
            }
            // A background menu has no selection. Ask the host's folder view
            // for its folder instead of silently reporting a successful no-op.
            let site = self.site.borrow();
            let service: IServiceProvider =
                site.as_ref().ok_or_else(|| Error::from(E_FAIL))?.cast()?;
            let view: IFolderView = service.QueryService(&IFolderView::IID)?;
            let folder: IPersistFolder2 = view.GetFolder()?;
            let id_list = folder.GetCurFolder()?;
            let item: windows::core::Result<IShellItem> = SHCreateItemFromIDList(id_list);
            CoTaskMemFree(Some(id_list.cast()));
            launch(&executable, &folder_path(&item?)?)
        }
    }

    fn GetFlags(&self) -> windows::core::Result<u32> {
        Ok(ECF_DEFAULT.0 as u32)
    }
    fn EnumSubCommands(&self) -> windows::core::Result<IEnumExplorerCommand> {
        Err(E_NOTIMPL.into())
    }
}

impl IObjectWithSite_Impl for PebrelCommand_Impl {
    fn SetSite(&self, site: Option<&IUnknown>) -> windows::core::Result<()> {
        *self.site.borrow_mut() = site.cloned();
        Ok(())
    }
    fn GetSite(&self, riid: *const GUID, result: *mut *mut c_void) -> windows::core::Result<()> {
        if riid.is_null() || result.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe {
            *result = std::ptr::null_mut();
        }
        let site = self.site.borrow();
        unsafe { site.as_ref().ok_or_else(|| Error::from(E_FAIL))?.query(riid, result).ok() }
    }
}

#[implement(IClassFactory)]
struct Factory;
impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(
        &self,
        outer: Option<&IUnknown>,
        riid: *const GUID,
        result: *mut *mut c_void,
    ) -> windows::core::Result<()> {
        if riid.is_null() || result.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe {
            *result = std::ptr::null_mut();
        }
        if outer.is_some() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        let command: IExplorerCommand = PebrelCommand::default().into();
        unsafe { command.query(riid, result).ok() }
    }
    fn LockServer(&self, _lock: BOOL) -> windows::core::Result<()> {
        Ok(())
    }
}

#[no_mangle]
unsafe extern "system" fn DllGetClassObject(
    class: *const GUID,
    riid: *const GUID,
    result: *mut *mut c_void,
) -> HRESULT {
    if class.is_null() || riid.is_null() || result.is_null() {
        return E_POINTER;
    }
    *result = std::ptr::null_mut();
    if *class != CLSID_PEBREL_COMMAND {
        return CLASS_E_CLASSNOTAVAILABLE;
    }
    let factory: IClassFactory = Factory.into();
    factory.query(riid, result)
}

#[no_mangle]
extern "system" fn DllCanUnloadNow() -> HRESULT {
    // The manifest hosts this DLL in a COM surrogate, not the Explorer process.
    S_FALSE
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installation_path_is_relative_and_unicode_safe() {
        let module = Path::new(r"D:\工具 路径\Pebrel\context-menu\pebrel_context_menu.dll");
        assert_eq!(
            executable_from_module(module).unwrap(),
            Path::new(r"D:\工具 路径\Pebrel\pebrel.exe")
        );
    }
    #[test]
    fn missing_directory_and_executable_report_failure() {
        assert!(launch(Path::new("missing-pebrel.exe"), Path::new("missing-directory")).is_err());
        assert!(launch(Path::new("missing-pebrel.exe"), &std::env::temp_dir()).is_err());
    }
    #[test]
    fn class_factory_rejects_invalid_pointers_and_unknown_classes() {
        let mut result = std::ptr::null_mut();
        unsafe {
            assert_eq!(
                DllGetClassObject(std::ptr::null(), &IClassFactory::IID, &mut result),
                E_POINTER
            );
            assert_eq!(
                DllGetClassObject(&GUID::zeroed(), &IClassFactory::IID, &mut result),
                CLASS_E_CLASSNOTAVAILABLE
            );
        }
    }
}
