use nebula_terminal::tty;

pub fn prepare(options: &mut tty::Options) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        prepare_unix(options)
    }
    #[cfg(not(unix))]
    {
        let _ = options;
        Ok(())
    }
}

#[cfg(unix)]
fn prepare_unix(options: &mut tty::Options) -> std::io::Result<()> {
    use std::path::Path;

    let program = match &options.shell {
        Some(shell) => shell.program().to_owned(),
        None => tty::default_shell_program()?,
    };
    let args = options.shell.as_ref().map(|shell| shell.args()).unwrap_or_default();
    let name = Path::new(&program).file_name().and_then(|name| name.to_str()).unwrap_or_default();
    if !supports(name, args) {
        return Ok(());
    }
    let root = super::dirs::data_dir().join("shell-integration");
    match name {
        "zsh" => {
            let directory = root.join("zsh");
            write_zsh_files(&directory)?;
            if let Some(original) = std::env::var_os("ZDOTDIR") {
                options.env.insert(
                    "NEBULA_ORIGINAL_ZDOTDIR".into(),
                    original.to_string_lossy().into_owned(),
                );
                options.env.insert("NEBULA_ZDOTDIR_WAS_SET".into(), "1".into());
            } else {
                options.env.insert("NEBULA_ZDOTDIR_WAS_SET".into(), "0".into());
            }
            let directory = directory.to_string_lossy().into_owned();
            options.env.insert("NEBULA_ZSH_INTEGRATION".into(), directory.clone());
            options.env.insert("ZDOTDIR".into(), directory);
        },
        "bash" => {
            std::fs::create_dir_all(&root)?;
            let init = root.join("bashrc");
            let content =
                format!("{}\n{}", include_str!("../../res/shell/bashrc"), tty::connection_shell());
            crate::atomic_file::write(&init, content.as_bytes())?;
            options.shell = Some(tty::Shell::new(
                program,
                vec!["--rcfile".into(), init.to_string_lossy().into_owned(), "-i".into()],
            ));
        },
        _ => {},
    }
    Ok(())
}

const ZSH_FILES: [(&str, &str); 3] = [
    (".zshenv", include_str!("../../res/shell/zshenv")),
    (".zprofile", include_str!("../../res/shell/zprofile")),
    (".zshrc", include_str!("../../res/shell/zshrc")),
];

/// The zsh bootstrap shared by local zsh and WSL guests: restore the user's
/// `ZDOTDIR`, source their own startup files, then install the precmd reports.
fn write_zsh_files(directory: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(directory)?;
    for (name, content) in ZSH_FILES {
        let content = if name == ".zshrc" {
            format!("{content}\n{}", tty::connection_shell())
        } else {
            content.to_owned()
        };
        // A guest zsh treats CR as part of each command; checkout bytes must not leak in.
        let content = content.replace("\r\n", "\n");
        crate::atomic_file::write(&directory.join(name), content.as_bytes())?;
    }
    Ok(())
}

/// Host directory that a WSL guest zsh uses as `ZDOTDIR` (translated through
/// `WSLENV` `/p`), see [`crate::shell_detect::wsl_cwd_report_env`]. Kept apart
/// from the local-zsh directory so the two integrations never rewrite each other.
///
/// `None` off Windows and when the data directory is not on a local drive
/// letter: a UNC or redirected path is not automounted in the guest, and a
/// `ZDOTDIR` the guest cannot read would also skip the user's own startup files.
/// Whether a given guest user can actually read it, and whether the launch starts
/// zsh at all, is the guest's answer (`super::wsl_guest_shell`), not a host guess.
/// Only the guest probe worker calls this (see `super::wsl_guest_shell`): it
/// writes and fsyncs, so it must never run on the UI thread. The spawn path uses
/// [`wsl_zsh_directory_ready`]. The files are written once per process; later
/// calls never replace a file that a starting guest zsh may be reading. A failed
/// write (a sharing violation from a guest still reading through 9P, an
/// antivirus hold) warns once and is retried only after five minutes. A
/// bootstrap file deleted while the process runs is rewritten by the next probe.
pub(crate) fn wsl_zsh_directory() -> Option<std::path::PathBuf> {
    #[cfg(windows)]
    {
        use std::sync::PoisonError;
        use std::time::Instant;

        let mut state = WSL_BOOTSTRAP.lock().unwrap_or_else(PoisonError::into_inner);
        let (directory, complete) = match &*state {
            Some(Bootstrap::Ineligible) => return None,
            Some(Bootstrap::Ready(directory)) => (
                directory.clone(),
                ZSH_FILES.iter().all(|(name, _)| directory.join(name).is_file()),
            ),
            Some(Bootstrap::Failed(_, at)) if at.elapsed() < BOOTSTRAP_RETRY_AFTER => return None,
            Some(Bootstrap::Failed(directory, _)) => (directory.clone(), false),
            None => {
                let directory = super::dirs::data_dir().join("shell-integration").join("wsl-zsh");
                if !is_local_drive_path(&directory) {
                    *state = Some(Bootstrap::Ineligible);
                    return None;
                }
                (directory, false)
            },
        };
        if complete {
            return Some(directory);
        }
        match write_zsh_files(&directory) {
            Ok(()) => {
                *state = Some(Bootstrap::Ready(directory.clone()));
                Some(directory)
            },
            Err(error) => {
                log::warn!("Could not prepare WSL zsh integration: {error}");
                *state = Some(Bootstrap::Failed(directory, Instant::now()));
                None
            },
        }
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// The WSL bootstrap directory if it is already written and complete, for the
/// spawn path on the UI thread: it never writes and never waits for a writer
/// (a busy lock answers `None`; that pane simply starts without zsh reports).
pub(crate) fn wsl_zsh_directory_ready() -> Option<std::path::PathBuf> {
    #[cfg(windows)]
    {
        let state = WSL_BOOTSTRAP.try_lock().ok()?;
        match &*state {
            Some(Bootstrap::Ready(directory))
                if ZSH_FILES.iter().all(|(name, _)| directory.join(name).is_file()) =>
            {
                Some(directory.clone())
            },
            _ => None,
        }
    }
    #[cfg(not(windows))]
    {
        None
    }
}

#[cfg(windows)]
const BOOTSTRAP_RETRY_AFTER: std::time::Duration = std::time::Duration::from_secs(300);

#[cfg(windows)]
enum Bootstrap {
    Ineligible,
    Ready(std::path::PathBuf),
    Failed(std::path::PathBuf, std::time::Instant),
}

#[cfg(windows)]
static WSL_BOOTSTRAP: std::sync::Mutex<Option<Bootstrap>> = std::sync::Mutex::new(None);

/// `D:\…` or `\\?\D:\…` on a fixed disk. UNC shares, mapped network drives and
/// relative paths are not automounted into the guest.
#[cfg(windows)]
fn is_local_drive_path(path: &std::path::Path) -> bool {
    use std::path::{Component, Prefix};
    // `DRIVE_FIXED` lives in a `windows` feature this crate does not enable.
    const DRIVE_FIXED: u32 = 3;
    let letter = match path.components().next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => letter,
            _ => return false,
        },
        _ => return false,
    };
    let root: Vec<u16> = format!("{}:\\", char::from(letter)).encode_utf16().chain([0]).collect();
    // SAFETY: `root` is a NUL-terminated UTF-16 string that outlives the call.
    let kind = unsafe {
        windows::Win32::Storage::FileSystem::GetDriveTypeW(windows::core::PCWSTR(root.as_ptr()))
    };
    kind == DRIVE_FIXED
}

#[cfg(unix)]
fn supports(name: &str, args: &[String]) -> bool {
    match name {
        "zsh" => args.iter().all(|arg| matches!(arg.as_str(), "-l" | "--login" | "-i")),
        "bash" => !cfg!(target_os = "macos") && args.is_empty(),
        _ => false,
    }
}

#[cfg(test)]
mod zsh_file_tests {
    #[test]
    fn zsh_bootstrap_is_posix_text_and_chains_the_user_rc() {
        let directory = tempfile::tempdir().expect("temporary directory");
        // The second write covers replacing an existing bootstrap.
        for _ in 0..2 {
            super::write_zsh_files(directory.path()).expect("write zsh bootstrap");
        }
        for name in [".zshenv", ".zprofile", ".zshrc"] {
            let content = std::fs::read_to_string(directory.path().join(name)).expect(name);
            assert!(!content.contains('\r'), "{name} must not carry CR into the guest");
            assert!(
                content.contains(&format!("${{ZDOTDIR-$HOME}}/{name}")),
                "{name} chains user file"
            );
        }
        let env = std::fs::read_to_string(directory.path().join(".zshenv")).unwrap();
        assert!(
            env.contains("-o rcs && -o interactive"),
            "`zsh -c` must not export the bootstrap ZDOTDIR to its children"
        );
        let rc = std::fs::read_to_string(directory.path().join(".zshrc")).unwrap();
        assert!(rc.contains("]7;file://"), "zsh integration reports OSC 7 cwd");
    }

    #[cfg(windows)]
    #[test]
    fn wsl_bootstrap_needs_a_guest_visible_drive() {
        let system = std::env::var_os("SystemRoot").expect("SystemRoot");
        assert!(super::is_local_drive_path(std::path::Path::new(&system)));
        assert!(!super::is_local_drive_path(std::path::Path::new(r"\\server\share\Pebrel")));
        assert!(!super::is_local_drive_path(std::path::Path::new(r"relative\Pebrel")));
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn custom_commands_and_rcfiles_are_never_rewritten() {
        for name in ["bash", "zsh", "fish"] {
            assert!(!supports(name, &["-c".into(), "echo test".into()]));
            assert!(!supports(name, &["--norc".into()]));
            assert!(!supports(name, &["--rcfile".into(), "custom".into()]));
        }
        assert!(supports("zsh", &["-l".into()]));
        assert_eq!(supports("bash", &[]), !cfg!(target_os = "macos"));
    }

    #[test]
    fn zsh_integration_enables_colors_for_macos_bsd_ls() {
        let zshrc = include_str!("../../res/shell/zshrc");
        assert!(zshrc.contains("${CLICOLOR=1}"));
        assert!(zshrc.contains("${LSCOLORS=GxFxCxDxBxegedabagaced}"));
        assert!(zshrc.contains("export CLICOLOR LSCOLORS"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_zsh_color_defaults_respect_user_policy() {
        let directory = tempfile::tempdir().unwrap();
        let integration = directory.path().join("integration.zsh");
        std::fs::write(&integration, include_str!("../../res/shell/zshrc")).unwrap();
        for (rc, expected) in [
            ("", "1:GxFxCxDxBxegedabagaced:unset"),
            ("CLICOLOR=0; LSCOLORS=custom; alias ls='ls -lah'", "0:custom:ls -lah"),
            ("CLICOLOR=''; LSCOLORS=''", "::unset"),
            ("NO_COLOR=1", "unset:unset:unset"),
            ("TERM=dumb", "unset:unset:unset"),
        ] {
            std::fs::write(directory.path().join(".zshrc"), rc).unwrap();
            let output = std::process::Command::new("/bin/zsh")
                .args([
                    "-d",
                    "-f",
                    "-c",
                    "source \"$1\"; print -r -- \"${CLICOLOR-unset}:${LSCOLORS-unset}:${aliases[ls]-unset}\"",
                    "pebrel-color-test",
                ])
                .arg(&integration)
                .env("NEBULA_ZDOTDIR_WAS_SET", "1")
                .env("NEBULA_ORIGINAL_ZDOTDIR", directory.path())
                .env("ZDOTDIR", directory.path())
                .env("TERM", "xterm-256color")
                .env_remove("CLICOLOR")
                .env_remove("CLICOLOR_FORCE")
                .env_remove("LSCOLORS")
                .env_remove("NO_COLOR")
                .output()
                .unwrap();
            assert!(output.status.success(), "{rc}: {:?}", output.stderr);
            assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), expected, "{rc}");
        }
    }
}
