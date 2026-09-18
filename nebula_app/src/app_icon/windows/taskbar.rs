//! Windows taskbar identity uses shell properties in addition to WM_SETICON.
//! Keep the selected icon as an immutable, content-addressed ICO so Explorer
//! does not reuse the default executable icon from its path-based cache.

use nebula_settings::AppIconName;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use windows::Win32::Foundation::{HWND, PROPERTYKEY};
use windows::Win32::System::Com::StructuredStorage::{
    PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0,
};
use windows::Win32::System::Variant::VT_LPWSTR;
use windows::Win32::UI::Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow};
use windows::Win32::UI::Shell::SHStrDupW;
use windows::core::{GUID, HSTRING};

mod shortcuts;
pub(crate) use shortcuts::refresh_pinned;

const APP_USER_MODEL: GUID = GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3);
const RELAUNCH_COMMAND: PROPERTYKEY = PROPERTYKEY { fmtid: APP_USER_MODEL, pid: 2 };
const RELAUNCH_ICON: PROPERTYKEY = PROPERTYKEY { fmtid: APP_USER_MODEL, pid: 3 };
const RELAUNCH_NAME: PROPERTYKEY = PROPERTYKEY { fmtid: APP_USER_MODEL, pid: 4 };
const APP_ID: PROPERTYKEY = PROPERTYKEY { fmtid: APP_USER_MODEL, pid: 5 };

fn ico(variant: AppIconName) -> std::io::Result<Vec<u8>> {
    let frames = crate::app_icon::FRAME_SIZES
        .into_iter()
        .map(|size| {
            crate::app_icon::png(variant, size)
                .map(|png| (size, png))
                .ok_or_else(|| std::io::Error::other("Could not render application icon"))
        })
        .collect::<std::io::Result<Vec<_>>>()?;
    let mut bytes = vec![0, 0, 1, 0];
    bytes.extend_from_slice(&(frames.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * frames.len() as u32;
    for (size, png) in &frames {
        bytes.extend_from_slice(&[*size as u8, *size as u8, 0, 0]);
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&32u16.to_le_bytes());
        bytes.extend_from_slice(&(png.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for (_, png) in frames {
        bytes.extend_from_slice(&png);
    }
    Ok(bytes)
}

fn materialize(directory: &Path, variant: AppIconName) -> std::io::Result<PathBuf> {
    let bytes = ico(variant)?;
    let digest =
        Sha256::digest(&bytes).iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    let path = directory.join("app-icons").join(format!(
        "{}-{}.ico",
        variant.settings_value(),
        &digest[..16]
    ));
    if std::fs::read(&path).ok().as_deref() != Some(bytes.as_slice()) {
        crate::atomic_file::write(&path, &bytes)?;
    }
    Ok(path)
}

fn set_string(store: &IPropertyStore, key: &PROPERTYKEY, value: &str) -> windows::core::Result<()> {
    // SHStrDupW transfers a COM allocation to the owning PROPVARIANT. Its
    // binding calls PropVariantClear on drop; never give it a Rust Vec pointer.
    let owned = unsafe { SHStrDupW(&HSTRING::from(value))? };
    let property = PROPVARIANT {
        Anonymous: PROPVARIANT_0 {
            Anonymous: std::mem::ManuallyDrop::new(PROPVARIANT_0_0 {
                vt: VT_LPWSTR,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: PROPVARIANT_0_0_0 { pwszVal: owned },
            }),
        },
    };
    unsafe { store.SetValue(key, &property) }
}

fn apply(hwnd: HWND, icon: &Path, executable: &Path) -> windows::core::Result<()> {
    let store: IPropertyStore = unsafe { SHGetPropertyStoreForWindow(hwnd)? };
    // Explorer requires all three relaunch properties, even when only the icon
    // differs. Keep the product launch mode and executable path intact.
    set_string(&store, &RELAUNCH_COMMAND, &format!("\"{}\" --gpui", executable.display()))?;
    set_string(&store, &RELAUNCH_NAME, crate::brand::NAME)?;
    set_string(&store, &RELAUNCH_ICON, &format!("{},0", icon.display()))?;
    set_string(&store, &APP_ID, crate::brand::WINDOWS_APP_ID)?;
    unsafe { store.Commit() }
}

pub(super) fn set_window(hwnd: windows_sys::Win32::Foundation::HWND, variant: AppIconName) {
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        static FILES: std::sync::OnceLock<
            std::sync::Mutex<std::collections::HashMap<(PathBuf, AppIconName), PathBuf>>,
        > = std::sync::OnceLock::new();
        let directory = crate::display::nebula_data_dir();
        let mut files =
            FILES.get_or_init(Default::default).lock().unwrap_or_else(|error| error.into_inner());
        let key = (directory.clone(), variant);
        let icon = match files.get(&key).filter(|path| path.is_file()) {
            Some(path) => path.clone(),
            None => {
                let path = materialize(&directory, variant)?;
                files.insert(key, path.clone());
                path
            },
        };
        drop(files);
        apply(HWND(hwnd), &icon, &std::env::current_exe()?)?;
        Ok(())
    })();
    if let Err(error) = result {
        log::warn!("Could not update taskbar icon: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taskbar_icon_paths_are_stable_per_palette_and_contain_every_frame() {
        let directory = tempfile::tempdir().unwrap();
        let first = materialize(directory.path(), AppIconName::Titanium).unwrap();
        assert_eq!(first, materialize(directory.path(), AppIconName::Titanium).unwrap());
        assert_ne!(first, materialize(directory.path(), AppIconName::GraphiteViolet).unwrap());
        let bytes = std::fs::read(first).unwrap();
        for size in crate::app_icon::FRAME_SIZES {
            let png = crate::app_icon::tests::ico_png(&bytes, size);
            assert_eq!(
                image::load_from_memory(png).unwrap().to_rgba8(),
                crate::app_icon::rgba(AppIconName::Titanium, size).unwrap()
            );
        }
    }
}

#[cfg(test)]
pub(in crate::app_icon) fn icon_location(
    hwnd: windows_sys::Win32::Foundation::HWND,
) -> windows::core::Result<String> {
    let store: IPropertyStore = unsafe { SHGetPropertyStoreForWindow(HWND(hwnd))? };
    let property = unsafe { store.GetValue(&RELAUNCH_ICON)? };
    Ok(windows::core::BSTR::try_from(&property)?.to_string())
}
