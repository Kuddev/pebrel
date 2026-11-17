//! Short-lived, per-user package deployment invoked by the Inno installer.
#![cfg(windows)]

use std::os::windows::ffi::OsStrExt;
use std::{env, fs, path::Path};
use windows::{
    core::{Error, Interface, Result, HSTRING, PCWSTR, PWSTR},
    ApplicationModel::Package,
    Data::Xml::Dom::{XmlDocument, XmlElement},
    Foundation::Uri,
    Management::Deployment::{
        AddPackageOptions, DeploymentResult, PackageManager, RegisterPackageOptions,
    },
    Win32::{
        Foundation::E_FAIL,
        System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED},
        UI::Shell::UrlCreateFromPathW,
    },
};

fn failure(message: impl AsRef<str>) -> Error {
    Error::new(E_FAIL, message.as_ref())
}

fn file_uri(path: &Path) -> Result<Uri> {
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut buffer = vec![0u16; 32768];
    let mut length = buffer.len() as u32;
    unsafe {
        UrlCreateFromPathW(PCWSTR(path.as_ptr()), PWSTR(buffer.as_mut_ptr()), &mut length, 0)?
    };
    Uri::CreateUri(&HSTRING::from_wide(&buffer[..length as usize])?)
}

fn identity(manifest: &str) -> Result<(HSTRING, HSTRING)> {
    let document = XmlDocument::new()?;
    document.LoadXml(&HSTRING::from(manifest.trim_start_matches('\u{feff}')))?;
    let nodes = document.GetElementsByTagName(&HSTRING::from("Identity"))?;
    if nodes.Length()? != 1 {
        return Err(failure("The context-menu manifest must contain one package identity"));
    }
    let element: XmlElement = nodes.Item(0)?.cast()?;
    let name = element.GetAttribute(&HSTRING::from("Name"))?;
    let publisher = element.GetAttribute(&HSTRING::from("Publisher"))?;
    if name.is_empty() || publisher.is_empty() {
        return Err(failure("The context-menu package identity is incomplete"));
    }
    Ok((name, publisher))
}

fn packages(manager: &PackageManager, name: &HSTRING, publisher: &HSTRING) -> Result<Vec<Package>> {
    let iterator = manager
        .FindPackagesByUserSecurityIdNamePublisher(&HSTRING::new(), name, publisher)?
        .First()?;
    let mut found = Vec::new();
    while iterator.HasCurrent()? {
        found.push(iterator.Current()?);
        iterator.MoveNext()?;
    }
    Ok(found)
}

fn owns(package: &Package, installation: &Path) -> Result<bool> {
    // InstalledLocation is WindowsApps for signed sparse packages. The external
    // location is the actual application directory for both deployment modes.
    let registered = package.EffectiveExternalLocation()?.Path()?;
    let expected = fs::canonicalize(installation).map_err(|e| failure(e.to_string()))?;
    let actual = fs::canonicalize(Path::new(&registered.to_os_string()))
        .map_err(|e| failure(e.to_string()))?;
    Ok(expected.as_os_str().eq_ignore_ascii_case(actual.as_os_str()))
}

fn completed(result: DeploymentResult) -> Result<()> {
    let code = result.ExtendedErrorCode()?;
    if code.is_err() {
        return Err(Error::new(code, result.ErrorText()?.to_string_lossy()));
    }
    Ok(())
}

fn deploy(installation: &Path, unregister: bool) -> Result<()> {
    let root = installation.join("context-menu");
    let manifest_path = root.join("AppxManifest.xml");
    let manifest = fs::read_to_string(&manifest_path).map_err(|e| failure(e.to_string()))?;
    let (name, publisher) = identity(&manifest)?;
    let manager = PackageManager::new()?;
    for package in packages(&manager, &name, &publisher)? {
        if !owns(&package, installation)? {
            if unregister {
                // Uninstalling an old copy must not unregister a newer owner.
                continue;
            }
            return Err(failure("The context menu is registered to another installation"));
        }
        if unregister {
            completed(manager.RemovePackageAsync(&package.Id()?.FullName()?)?.get()?)?;
        }
    }
    if !unregister {
        for path in [
            installation.join("pebrel.exe"),
            root.join("pebrel_context_menu.dll"),
            root.join("assets/logo.png"),
        ] {
            if !path.is_file() {
                return Err(failure(format!("Missing context-menu payload: {}", path.display())));
            }
        }
        let signed = root.join("Pebrel.ContextMenu.msix");
        if signed.is_file() {
            // Windows verifies the package signature; never import certificates
            // or silently retry rejected signed packages in development mode.
            let options = AddPackageOptions::new()?;
            options.SetExternalLocationUri(&file_uri(installation)?)?;
            completed(manager.AddPackageByUriAsync(&file_uri(&signed)?, &options)?.get()?)?;
        } else {
            let options = RegisterPackageOptions::new()?;
            options.SetExternalLocationUri(&file_uri(installation)?)?;
            options.SetDeveloperMode(true)?;
            completed(
                manager.RegisterPackageByUriAsync(&file_uri(&manifest_path)?, &options)?.get()?,
            )?;
        }
    }
    let mut registered = false;
    for package in packages(&manager, &name, &publisher)? {
        if owns(&package, installation)? {
            if !package.Status()?.VerifyIsOK()? {
                return Err(failure("The registered context-menu package is not healthy"));
            }
            registered = true;
        }
    }
    if registered == unregister {
        return Err(failure("Context-menu deployment verification failed"));
    }
    Ok(())
}

fn run() -> Result<()> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() != 1 || (args[0] != "--register" && args[0] != "--unregister") {
        return Err(failure("Usage: pebrel-context-menu-setup --register | --unregister"));
    }
    let executable = env::current_exe().map_err(|e| failure(e.to_string()))?;
    let root = executable
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| failure("Helper must be installed in <installation>/context-menu"))?;
    let result = deploy(root, args[0] == "--unregister");
    let message = match &result {
        Ok(()) => format!("{}: OK\n", args[0].to_string_lossy()),
        Err(error) => format!("{}: {error}\n", args[0].to_string_lossy()),
    };
    // Preserve deployment diagnostics for silent installers too.
    fs::write(root.join("context-menu/setup.log"), message).map_err(|e| failure(e.to_string()))?;
    result
}

fn main() {
    let result = unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.and_then(|()| {
        let result = run();
        unsafe { RoUninitialize() };
        result
    });
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_requires_complete_unambiguous_manifest() {
        unsafe { RoInitialize(RO_INIT_MULTITHREADED).unwrap() };
        let (name, publisher) = identity(include_str!("../package/AppxManifest.xml")).unwrap();
        assert_eq!(name, "Pebrel.ContextMenu");
        assert_eq!(publisher, "CN=PebrelLocal");
        assert!(
            identity(&format!("\u{feff}{}", include_str!("../package/AppxManifest.xml"))).is_ok()
        );
        assert!(identity("<Package><Identity Name='X'/></Package>").is_err());
        assert!(identity("<Package><Identity/><Identity/></Package>").is_err());
        unsafe { RoUninitialize() };
    }

    #[test]
    fn file_uri_preserves_spaces_and_unicode() {
        unsafe { RoInitialize(RO_INIT_MULTITHREADED).unwrap() };
        let uri = file_uri(Path::new(r"C:\目录 #1\AppxManifest.xml")).unwrap();
        assert_eq!(uri.SchemeName().unwrap(), "file");
        assert!(uri.AbsoluteUri().unwrap().to_string_lossy().contains("%23"));
        drop(uri);
        unsafe { RoUninitialize() };
    }
}
