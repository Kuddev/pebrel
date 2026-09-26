//! Optional, process-scoped Windows App Runtime package graph lease.
//!
//! Resolve the Windows 11 API dynamically so older Windows can still load Pebrel.
//! No bootstrap DLL, installation, filesystem probing or global package mutation.

use std::ffi::c_void;
use std::ptr::{null, null_mut};

use windows_core::{Error, HRESULT, Result};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::System::Memory::{GetProcessHeap, HeapFree};

type Create = unsafe extern "system" fn(
    *const c_void,
    *const u16,
    u64,
    i32,
    i32,
    *const u16,
    u32,
    *mut *mut u16,
) -> i32;
type Add = unsafe extern "system" fn(*const u16, i32, u32, *mut *mut c_void, *mut *mut u16) -> i32;
type Remove = unsafe extern "system" fn(*mut c_void) -> i32;
type Delete = unsafe extern "system" fn(*const u16) -> i32;

pub(super) struct Runtime {
    context: *mut c_void,
    id: *mut u16,
    remove: Remove,
    delete: Delete,
}

impl Runtime {
    pub(super) fn load() -> Result<Self> {
        Self::load_family(windows_core::w!("Microsoft.WindowsAppRuntime.1.8_8wekyb3d8bbwe"))
    }

    fn load_family(family: windows_core::PCWSTR) -> Result<Self> {
        // SAFETY: kernelbase is an OS module already loaded for this process. Each
        // export has the signature in the Windows SDK's appmodel.h (22621+).
        unsafe {
            let module = GetModuleHandleW(windows_core::w!("kernelbase.dll").as_ptr());
            let proc = |name: &'static std::ffi::CStr| {
                GetProcAddress(module, name.as_ptr().cast()).ok_or_else(Error::from_win32)
            };
            if module.is_null() {
                return Err(Error::from_win32());
            }
            let create: Create = std::mem::transmute(proc(c"TryCreatePackageDependency")?);
            let add: Add = std::mem::transmute(proc(c"AddPackageDependency")?);
            let remove: Remove = std::mem::transmute(proc(c"RemovePackageDependency")?);
            let delete: Delete = std::mem::transmute(proc(c"DeletePackageDependency")?);
            let mut lease = Self { context: null_mut(), id: null_mut(), remove, delete };
            // Pin the minimum runtime actually validated, allowing newer patches
            // in the same 1.8 family. Process lifetime; default architecture.
            HRESULT(create(
                null(),
                family.as_ptr(),
                (8000_u64 << 48) | (946_u64 << 32) | (1701_u64 << 16),
                0,
                0,
                null(),
                0,
                &mut lease.id,
            ))
            .ok()?;
            let mut full_name = null_mut();
            let result = HRESULT(add(lease.id, 0, 0, &mut lease.context, &mut full_name));
            if !full_name.is_null() {
                if let Ok(name) = windows_core::PCWSTR(full_name).to_string() {
                    log::debug!(target: "nebula", "Acrylic runtime: {name}");
                }
                HeapFree(GetProcessHeap(), 0, full_name.cast());
            }
            result.ok()?;
            Ok(lease)
        }
    }
}

#[cfg(test)]
#[test]
fn absent_runtime_is_a_recoverable_error() {
    assert!(
        Runtime::load_family(windows_core::w!("Pebrel.MissingAcrylicRuntime_8wekyb3d8bbwe"))
            .is_err()
    );
}

impl Drop for Runtime {
    fn drop(&mut self) {
        // Release owned SDK objects before removing their activation context.
        // Removing the graph entry does not unload already loaded runtime DLLs.
        unsafe {
            if !self.context.is_null() {
                let _ = (self.remove)(self.context);
            }
            if !self.id.is_null() {
                let _ = (self.delete)(self.id);
                HeapFree(GetProcessHeap(), 0, self.id.cast());
            }
        }
    }
}
