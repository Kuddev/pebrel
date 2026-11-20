//! A pinned taskbar button reads its shortcut icon, even after WM_SETICON.
//! Only update links targeting this exact executable; preserve all other link
//! metadata and leave links to other installations or applications untouched.

use super::*;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoUninitialize, IPersistFile, STGM_READWRITE,
};
use windows::Win32::UI::Shell::{
    IShellLinkW, SHCNE_UPDATEITEM, SHCNF_PATHW, SHChangeNotify, SLGP_RAWPATH, ShellLink,
};
use windows::core::Interface;

fn same_executable(target: &Path, executable: &Path) -> bool {
    match (std::fs::canonicalize(target), std::fs::canonicalize(executable)) {
        (Ok(target), Ok(executable)) => {
            target.to_string_lossy().eq_ignore_ascii_case(&executable.to_string_lossy())
        },
        _ => false,
    }
}

fn update_link(path: &Path, executable: &Path, icon: &Path) -> windows::core::Result<bool> {
    let link: IShellLinkW = unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)? };
    let file: IPersistFile = link.cast()?;
    let filename = HSTRING::from(path.as_os_str());
    unsafe { file.Load(&filename, STGM_READWRITE)? };
    let mut target = vec![0u16; 32768];
    unsafe { link.GetPath(&mut target, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32)? };
    let length = target.iter().position(|unit| *unit == 0).unwrap_or(target.len());
    let target = PathBuf::from(String::from_utf16_lossy(&target[..length]));
    if !same_executable(&target, executable) {
        return Ok(false);
    }
    unsafe {
        link.SetIconLocation(&HSTRING::from(icon.as_os_str()), 0)?;
        file.Save(&filename, true)?;
        SHChangeNotify(SHCNE_UPDATEITEM, SHCNF_PATHW, Some(filename.as_ptr().cast()), None);
    }
    Ok(true)
}

pub(crate) fn refresh_pinned(variant: AppIconName) {
    // An isolated preview/test instance must not rewrite the installed app's pins.
    if ["PEBREL_CONFIG_DIR", "NEBULA_CONFIG_DIR"]
        .into_iter()
        .any(|key| std::env::var_os(key).is_some_and(|value| !value.is_empty()))
    {
        return;
    }
    let Some(app_data) = std::env::var_os("APPDATA") else { return };
    let directory = PathBuf::from(app_data)
        .join("Microsoft/Internet Explorer/Quick Launch/User Pinned/TaskBar");
    std::thread::spawn(move || {
        static WRITER: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _writer = WRITER.lock().unwrap_or_else(|error| error.into_inner());
        // Serializing the workers and rechecking selection makes rapid changes
        // converge on the newest icon, including when an older write was in flight.
        if crate::app_icon::selected() != variant {
            return;
        }
        let Ok(executable) = std::env::current_exe() else { return };
        let Ok(icon) = materialize(&crate::display::nebula_data_dir(), variant) else { return };
        let Ok(entries) = std::fs::read_dir(directory) else { return };
        if unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_err() {
            return;
        }
        for entry in entries.flatten() {
            if crate::app_icon::selected() != variant {
                break;
            }
            let path = entry.path();
            if path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("lnk")) {
                if let Err(error) = update_link(&path, &executable, &icon) {
                    log::debug!("Could not refresh pinned application icon: {error}");
                }
            }
        }
        unsafe { CoUninitialize() };
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn updating_owned_pin_preserves_arguments_and_ignores_other_targets() {
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.unwrap();
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("pebrel.exe");
        let other = directory.path().join("other.exe");
        std::fs::write(&executable, []).unwrap();
        std::fs::write(&other, []).unwrap();
        let icon = directory.path().join("selected.ico");
        for (index, target) in [&executable, &other].into_iter().enumerate() {
            let path = directory.path().join(format!("pin-{index}.lnk"));
            let link: IShellLinkW =
                unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }.unwrap();
            unsafe {
                link.SetPath(&HSTRING::from(target.as_os_str())).unwrap();
                link.SetArguments(windows::core::w!("--gpui --working-directory C:\\project"))
                    .unwrap();
                link.SetIconLocation(windows::core::w!("original.ico"), 2).unwrap();
                link.cast::<IPersistFile>()
                    .unwrap()
                    .Save(&HSTRING::from(path.as_os_str()), true)
                    .unwrap();
            }
            let before = std::fs::read(&path).unwrap();
            assert_eq!(update_link(&path, &executable, &icon).unwrap(), index == 0);
            if index == 1 {
                assert_eq!(std::fs::read(&path).unwrap(), before);
                continue;
            }
            unsafe {
                link.cast::<IPersistFile>()
                    .unwrap()
                    .Load(&HSTRING::from(path.as_os_str()), STGM_READWRITE)
            }
            .unwrap();
            let mut arguments = [0u16; 256];
            unsafe { link.GetArguments(&mut arguments) }.unwrap();
            let length = arguments.iter().position(|unit| *unit == 0).unwrap();
            assert_eq!(
                String::from_utf16_lossy(&arguments[..length]),
                "--gpui --working-directory C:\\project"
            );
            let mut location = [0u16; 1024];
            let mut frame = -1;
            unsafe { link.GetIconLocation(&mut location, &mut frame) }.unwrap();
            let length = location.iter().position(|unit| *unit == 0).unwrap();
            assert_eq!(PathBuf::from(String::from_utf16_lossy(&location[..length])), icon);
            assert_eq!(frame, 0);
        }
        unsafe { CoUninitialize() };
    }
}
