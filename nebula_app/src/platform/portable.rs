//! macOS startup-only storage selection. No preferences or workers may open first.

use std::path::{Path, PathBuf};
use std::{env, fs, io};

use super::Launch;
use crate::i18n::{Message, UiLanguage};

const MARKER: &str = ".pebrel-portable";

pub(super) fn prepare(gui_launch: bool, explicit_config: bool) -> io::Result<Launch> {
    // Explicit launch configuration remains authoritative, including test isolation.
    if explicit_config
        || [
            "PEBREL_CONFIG_DIR",
            "NEBULA_CONFIG_DIR",
            "PEBREL_CONFIG_FILE",
            "NEBULA_CONFIG_FILE",
            "PEBREL_GPUI_CONFIG",
            "NEBULA_GPUI_CONFIG",
        ]
        .into_iter()
        .any(|name| env::var_os(name).is_some_and(|value| !value.is_empty()))
    {
        // Helpers inherit the override. Do not run legacy migration inside an
        // already portable store (or mix in another Mac's files).
        let data = nebula_settings::settings_dir();
        if data.join(MARKER).is_file() {
            return activate(&data);
        }
        return Ok(Launch::Installed);
    }
    let executable = env::current_exe()?.canonicalize()?;
    let Some(data) = data_directory(&executable, super::super::dirs::home_dir().as_deref()) else {
        return Ok(Launch::Installed);
    };
    let marked = data.join(MARKER).try_exists()?;
    if !marked {
        // CLI commands never prompt. Once enabled, they discover the same adjacent data.
        if !gui_launch {
            return Ok(Launch::Installed);
        }
        let language = language();
        let portable = language.text(Message::StartupPortableEnable);
        let normal = language.text(Message::StartupPortableNormal);
        let quit = language.text(Message::StartupPortableQuit);
        let result = rfd::MessageDialog::new()
            .set_title(language.text(Message::StartupPortableTitle))
            .set_description(language.format(
                Message::StartupPortableDescription,
                &[("path", &data.display().to_string())],
            ))
            .set_buttons(rfd::MessageButtons::YesNoCancelCustom(
                portable.to_owned(),
                normal.to_owned(),
                quit.to_owned(),
            ))
            .show();
        match selected_launch(result, language) {
            Launch::Portable => {},
            other => return Ok(other),
        }
    }
    if executable.components().any(|part| part.as_os_str() == "AppTranslocation") {
        return Err(io::Error::other(language().text(Message::StartupPortableTranslocated)));
    }
    activate(&data)
}

fn selected_launch(result: rfd::MessageDialogResult, language: UiLanguage) -> Launch {
    match result {
        rfd::MessageDialogResult::Custom(choice)
            if choice == language.text(Message::StartupPortableEnable) =>
        {
            Launch::Portable
        },
        rfd::MessageDialogResult::Custom(choice)
            if choice == language.text(Message::StartupPortableNormal) =>
        {
            Launch::Installed
        },
        _ => Launch::Quit,
    }
}

fn activate(data: &Path) -> io::Result<Launch> {
    initialize_directory(data).map_err(|error| {
        io::Error::other(language().format(
            Message::StartupPortableWriteFailed,
            &[("path", &data.display().to_string()), ("error", &error.to_string())],
        ))
    })?;
    // Startup is still on the main thread, before application workers or PTYs.
    // Both aliases cover legacy readers and are inherited by local CLI helpers.
    unsafe {
        env::set_var("PEBREL_CONFIG_DIR", data);
        env::set_var("NEBULA_CONFIG_DIR", data);
        env::set_var("TMPDIR", data.join("tmp"));
    }
    // A portable store must not import machine-local legacy data on another Mac.
    Ok(Launch::Portable)
}

fn data_directory(executable: &Path, home: Option<&Path>) -> Option<PathBuf> {
    // Only a real bundle layout prompts; cargo binaries and command-line installs don't.
    let macos = executable.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    if macos.file_name()? != "MacOS"
        || contents.file_name()? != "Contents"
        || !bundle.extension()?.eq_ignore_ascii_case("app")
        || bundle.starts_with("/Applications")
        || bundle.starts_with("/System/Applications")
        || home.is_some_and(|home| bundle.starts_with(home.join("Applications")))
    {
        return None;
    }
    Some(bundle.parent()?.join("Pebrel Data"))
}

fn initialize_directory(data: &Path) -> io::Result<()> {
    // Reject links that would leave the folder behind when the app is moved.
    for directory in [data.to_owned(), data.join("tmp")] {
        match fs::symlink_metadata(&directory) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(io::Error::other(
                    language().text(Message::StartupPortableLinkedDirectory),
                ));
            },
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {},
        }
        fs::create_dir_all(directory)?;
    }
    // Atomic writes test writability on every launch, including an existing store.
    let probe = data.join("tmp").join(format!(".startup-{}", std::process::id()));
    crate::atomic_file::write(&probe, b"")?;
    fs::remove_file(probe)?;
    crate::atomic_file::write(&data.join(MARKER), b"1\n")
}

fn language() -> UiLanguage {
    UiLanguage::for_locale(crate::i18n::system_locale().as_deref())
}

pub(super) fn report_error(error: &dyn std::fmt::Display) {
    rfd::MessageDialog::new()
        .set_title(language().text(Message::StartupPortableErrorTitle))
        .set_description(error.to_string())
        .set_level(rfd::MessageLevel::Error)
        .show();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_location_distinguishes_installed_portable_and_development_launches() {
        let home = Some(Path::new("/Users/test"));
        for path in [
            "/Applications/Pebrel.app/Contents/MacOS/pebrel",
            "/Applications/Tools/Pebrel.app/Contents/MacOS/pebrel",
            "/Users/test/Applications/Pebrel.app/Contents/MacOS/pebrel",
            "/System/Applications/Pebrel.app/Contents/MacOS/pebrel",
            "/work/target/debug/pebrel",
            "/work/Pebrel.app/pebrel",
        ] {
            assert_eq!(data_directory(Path::new(path), home), None, "{path}");
        }
        for folder in ["/Volumes/PSSD/我的工具", "/Applications Backup", "/Users/test/Downloads"]
        {
            let executable = Path::new(folder).join("Pebrel.app/Contents/MacOS/pebrel");
            assert_eq!(
                data_directory(&executable, home),
                Some(Path::new(folder).join("Pebrel Data"))
            );
        }
    }

    #[test]
    fn existing_data_survives_relaunch_and_folder_move() {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("original/Pebrel Data");
        initialize_directory(&data).unwrap();
        fs::write(data.join("pebrel_settings.txt"), "theme=dark\n").unwrap();
        initialize_directory(&data).unwrap();
        fs::rename(data.parent().unwrap(), temp.path().join("moved")).unwrap();
        let moved = temp.path().join("moved/Pebrel Data");
        initialize_directory(&moved).unwrap();
        assert_eq!(fs::read_to_string(moved.join("pebrel_settings.txt")).unwrap(), "theme=dark\n");
        assert!(moved.join(MARKER).is_file());
        assert!(moved.join("tmp").is_dir());
    }

    #[test]
    fn unavailable_or_redirected_storage_never_marks_portable_success() {
        let temp = tempfile::tempdir().unwrap();
        let blocked = temp.path().join("blocked");
        fs::write(&blocked, b"user file").unwrap();
        assert!(initialize_directory(&blocked).is_err());
        assert_eq!(fs::read(&blocked).unwrap(), b"user file");
        let linked = temp.path().join("linked");
        std::os::unix::fs::symlink(temp.path(), &linked).unwrap();
        assert!(initialize_directory(&linked).is_err());
        assert!(!temp.path().join(MARKER).exists());
    }

    #[test]
    fn read_only_storage_fails_without_replacing_user_data() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("Pebrel Data");
        initialize_directory(&data).unwrap();
        fs::write(data.join("pebrel_settings.txt"), "theme=dark\n").unwrap();
        fs::set_permissions(&data, fs::Permissions::from_mode(0o555)).unwrap();
        let result = initialize_directory(&data);
        fs::set_permissions(&data, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(result.is_err());
        assert_eq!(fs::read_to_string(data.join("pebrel_settings.txt")).unwrap(), "theme=dark\n");
    }

    #[test]
    fn only_explicit_portable_choice_enables_portable_storage() {
        for &language in UiLanguage::ALL {
            for (message, expected) in [
                (Message::StartupPortableEnable, Launch::Portable),
                (Message::StartupPortableNormal, Launch::Installed),
                (Message::StartupPortableQuit, Launch::Quit),
            ] {
                assert_eq!(
                    selected_launch(
                        rfd::MessageDialogResult::Custom(language.text(message).into()),
                        language
                    ),
                    expected
                );
            }
            assert_eq!(selected_launch(rfd::MessageDialogResult::Cancel, language), Launch::Quit);
        }
    }
}
