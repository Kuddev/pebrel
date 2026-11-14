//! Narrow Windows App SDK 1.8 ABI, absent from the OS-only `windows` crate.
//! Interface identities and slot order come from Microsoft.UI.winmd 1.8.
//! Use local activation factories: no SDK COM pointer may outlive our runtime lease.

use std::ffi::c_void;

use windows::UI::Color;
use windows::UI::Composition::CompositionTarget;
use windows::Win32::System::WinRT::{RoActivateInstance, RoGetActivationFactory};
use windows_core::{HRESULT, HSTRING, IInspectable_Vtbl, Interface, Result};

windows_core::imp::define_interface!(Acrylic, AcrylicVtbl, 0x7c20a6af_8eb3_5f08_bdfc_6d35e35dfe45);
#[repr(C)]
pub struct AcrylicVtbl {
    base: IInspectable_Vtbl,
    get_fallback: unsafe extern "system" fn(*mut c_void, *mut Color) -> HRESULT,
    set_fallback: unsafe extern "system" fn(*mut c_void, Color) -> HRESULT,
    get_luminosity: unsafe extern "system" fn(*mut c_void, *mut f32) -> HRESULT,
    set_luminosity: unsafe extern "system" fn(*mut c_void, f32) -> HRESULT,
    get_tint_color: unsafe extern "system" fn(*mut c_void, *mut Color) -> HRESULT,
    set_tint_color: unsafe extern "system" fn(*mut c_void, Color) -> HRESULT,
    get_tint: unsafe extern "system" fn(*mut c_void, *mut f32) -> HRESULT,
    set_tint: unsafe extern "system" fn(*mut c_void, f32) -> HRESULT,
}

windows_core::imp::define_interface!(Statics, StaticsVtbl, 0xa9e8f790_79ef_5416_9b67_6bcfe867c8b7);
#[repr(C)]
pub struct StaticsVtbl {
    base: IInspectable_Vtbl,
    is_supported: unsafe extern "system" fn(*mut c_void, *mut bool) -> HRESULT,
}

windows_core::imp::define_interface!(
    Configuration,
    ConfigurationVtbl,
    0xebcce1b9_0e0c_5431_ab0e_00f3f0669962
);
#[repr(C)]
pub struct ConfigurationVtbl {
    base: IInspectable_Vtbl,
    get_high_contrast_color: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
    set_high_contrast_color: unsafe extern "system" fn(*mut c_void, *mut c_void) -> HRESULT,
    get_high_contrast: unsafe extern "system" fn(*mut c_void, *mut bool) -> HRESULT,
    set_high_contrast: unsafe extern "system" fn(*mut c_void, bool) -> HRESULT,
    get_input_active: unsafe extern "system" fn(*mut c_void, *mut bool) -> HRESULT,
    set_input_active: unsafe extern "system" fn(*mut c_void, bool) -> HRESULT,
    get_theme: unsafe extern "system" fn(*mut c_void, *mut i32) -> HRESULT,
    set_theme: unsafe extern "system" fn(*mut c_void, i32) -> HRESULT,
}

windows_core::imp::define_interface!(
    Controller,
    ControllerVtbl,
    0x5632d76c_0b74_5b52_aa33_80262068aeb2
);
#[repr(C)]
pub struct ControllerVtbl {
    base: IInspectable_Vtbl,
    set_window_target:
        unsafe extern "system" fn(*mut c_void, WindowId, *mut c_void, *mut bool) -> HRESULT,
    set_core_target:
        unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void, *mut bool) -> HRESULT,
}

#[repr(C)]
struct WindowId {
    value: u64,
}

windows_core::imp::define_interface!(
    WithTargets,
    WithTargetsVtbl,
    0x9c56fe7c_98eb_5f89_ad97_dad57fc30c8c
);
#[repr(C)]
pub struct WithTargetsVtbl {
    base: IInspectable_Vtbl,
    get_state: unsafe extern "system" fn(*mut c_void, *mut i32) -> HRESULT,
    add_target: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut bool) -> HRESULT,
    remove_all_targets: unsafe extern "system" fn(*mut c_void) -> HRESULT,
    remove_target: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut bool) -> HRESULT,
    set_configuration: unsafe extern "system" fn(*mut c_void, *mut c_void) -> HRESULT,
    add_state_changed: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut i64) -> HRESULT,
    remove_state_changed: unsafe extern "system" fn(*mut c_void, i64) -> HRESULT,
}

const CLASS: &str = "Microsoft.UI.Composition.SystemBackdrops.DesktopAcrylicController";

impl Acrylic {
    pub(super) fn new() -> Result<Self> {
        unsafe {
            let factory: Statics = RoGetActivationFactory(&HSTRING::from(CLASS))?;
            let mut supported = false;
            (factory.vtable().is_supported)(factory.as_raw(), &mut supported).ok()?;
            if !supported {
                return Err(windows_core::Error::new(
                    HRESULT(0x80004001_u32 as i32),
                    "Acrylic unsupported",
                ));
            }
            RoActivateInstance(&HSTRING::from(CLASS))?.cast()
        }
    }

    pub(super) fn attach(&self, hwnd: isize, target: &CompositionTarget) -> Result<()> {
        let controller: Controller = self.cast()?;
        let mut attached = false;
        unsafe {
            (controller.vtable().set_window_target)(
                controller.as_raw(),
                WindowId { value: hwnd as u64 },
                target.as_raw(),
                &mut attached,
            )
            .ok()?;
        }
        if !attached {
            return Err(windows_core::Error::new(
                HRESULT(0x80004005_u32 as i32),
                "Acrylic target rejected",
            ));
        }
        Ok(())
    }

    pub(super) fn configure(&self) -> Result<Configuration> {
        unsafe {
            let config: Configuration = RoActivateInstance(&HSTRING::from(
                "Microsoft.UI.Composition.SystemBackdrops.SystemBackdropConfiguration",
            ))?
            .cast()?;
            (config.vtable().set_input_active)(config.as_raw(), true).ok()?;
            let targets: WithTargets = self.cast()?;
            // SetTarget installs its own focus policy. Override AFTER attaching,
            // otherwise deactivation reinstates opaque gray. Keep OS contrast and
            // transparency policies; only the focus-derived fallback is disabled.
            (targets.vtable().set_configuration)(targets.as_raw(), config.as_raw()).ok()?;
            (self.vtable().set_tint_color)(self.as_raw(), Color { A: 255, R: 46, G: 52, B: 64 })
                .ok()?;
            (self.vtable().set_tint)(self.as_raw(), 0.0).ok()?;
            (self.vtable().set_luminosity)(self.as_raw(), 0.0).ok()?;
            Ok(config)
        }
    }

    pub(super) fn close(&self) {
        match self.cast::<windows::Foundation::IClosable>().and_then(|object| object.Close()) {
            Ok(()) => {},
            Err(error) => {
                log::warn!(target: "nebula", "closing Acrylic controller failed: {error}")
            },
        }
    }
}
