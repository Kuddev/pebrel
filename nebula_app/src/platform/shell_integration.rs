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
        let content = tty::shell_line_endings(&content);
        crate::atomic_file::write(&directory.join(name), content.as_bytes())?;
    }
    Ok(())
}

/// Host directory a WSL guest zsh uses as `ZDOTDIR` (translated through `WSLENV`
/// `/p`), apart from the local-zsh one so the two never rewrite each other.
/// Only computes the path; [`wsl_zsh_directory`] writes it.
pub(crate) fn wsl_zsh_path() -> std::path::PathBuf {
    super::dirs::data_dir().join("shell-integration").join("wsl-zsh")
}

/// [`wsl_zsh_path`] with the bootstrap written. Only the guest probe worker
/// calls it (it writes and fsyncs); whether a guest user can read it is the
/// probe's answer (`super::wsl_guest_shell`). The files are written once per
/// process and afterwards only when missing, never while a starting guest zsh
/// may be reading them.
pub(crate) fn wsl_zsh_directory() -> Option<std::path::PathBuf> {
    static WRITTEN: std::sync::Mutex<bool> = std::sync::Mutex::new(false);
    if !cfg!(windows) {
        return None;
    }
    let directory = wsl_zsh_path();
    let mut written = WRITTEN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if !*written || !ZSH_FILES.iter().all(|(name, _)| directory.join(name).is_file()) {
        if let Err(error) = write_zsh_files(&directory) {
            log::warn!("Could not prepare WSL zsh integration: {error}");
            return None;
        }
        *written = true;
    }
    Some(directory)
}

#[cfg(unix)]
fn supports(name: &str, args: &[String]) -> bool {
    match name {
        "zsh" => args.iter().all(|arg| matches!(arg.as_str(), "-l" | "--login" | "-i")),
        "bash" => !cfg!(target_os = "macos") && args.is_empty(),
        _ => false,
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
