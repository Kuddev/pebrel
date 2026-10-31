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

    /// Hermetic git fixture: a fresh repo whose HEAD sits on a distinctive
    /// branch, so `git rev-parse --abbrev-ref` is deterministic.
    fn init_git_repo(root: &std::path::Path) {
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .arg("-C")
                .arg(root)
                .args(args)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init", "--quiet"]);
        git(&["symbolic-ref", "HEAD", "refs/heads/nebula-test-branch"]);
        git(&[
            "-c",
            "user.email=nebula@example.com",
            "-c",
            "user.name=Nebula",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--allow-empty",
            "--quiet",
            "-m",
            "init",
        ]);
    }

    fn git_available() -> bool {
        std::process::Command::new("git")
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    /// The `NEBULA|cwd|branch` title is the only branch source for the sidebar
    /// group chip; outside a repository the branch field must be empty (not
    /// garbage), so the chip clears when leaving a repo.
    #[test]
    fn zsh_precmd_reports_git_branch_via_nebula_title() {
        if !git_available() {
            return;
        }
        let home = tempfile::tempdir().unwrap();
        let repo = home.path().join("repo");
        std::fs::create_dir(&repo).unwrap();
        init_git_repo(&repo);
        let plain = home.path().join("plain");
        std::fs::create_dir(&plain).unwrap();
        let script = home.path().join("integration.zsh");
        std::fs::write(&script, include_str!("../../res/shell/zshrc")).unwrap();

        let run = |dir: &std::path::Path| {
            std::process::Command::new("/bin/zsh")
                .args([
                    "-d",
                    "-f",
                    "-c",
                    // Two precmds in one shell pin the change-edge contract: the
                    // `pebrel_git_root` UserVar must ride only the first one.
                    r#"source "$2"; cd "$1" && _nebula_precmd && _nebula_precmd"#,
                    "nebula-title-test",
                ])
                .arg(dir)
                .arg(&script)
                .env("NEBULA_ZDOTDIR_WAS_SET", "1")
                .env("NEBULA_ORIGINAL_ZDOTDIR", home.path())
                .env("ZDOTDIR", home.path())
                .env("TERM", "xterm-256color")
                .output()
                .unwrap()
        };

        let repo_output = run(&repo);
        assert!(repo_output.status.success(), "{:?}", repo_output.stderr);
        let stdout = String::from_utf8(repo_output.stdout).unwrap();
        assert!(
            stdout.contains(&format!("\x1b]2;NEBULA|{}|nebula-test-branch\x07", repo.display())),
            "repo title missing: {stdout:?}"
        );
        assert_eq!(
            stdout.matches("SetUserVar=pebrel_git_root=").count(),
            1,
            "the root var rides change edges only: {stdout:?}"
        );
        use base64::Engine as _;
        let physical_root = base64::engine::general_purpose::STANDARD
            .encode(repo.canonicalize().unwrap().display().to_string());
        assert!(
            stdout.contains(&format!("\x1b]1337;SetUserVar=pebrel_git_root={physical_root}\x07")),
            "repo root missing: {stdout:?}"
        );

        let plain_output = run(&plain);
        assert!(plain_output.status.success(), "{:?}", plain_output.stderr);
        let stdout = String::from_utf8(plain_output.stdout).unwrap();
        assert!(
            stdout.contains(&format!("\x1b]2;NEBULA|{}|\x07", plain.display())),
            "non-repo branch must be empty: {stdout:?}"
        );
        assert!(
            !stdout.contains("SetUserVar=pebrel_git_root="),
            "no root to report outside a repository: {stdout:?}"
        );
    }

    /// Same protocol contract for the Linux local bash integration.
    #[test]
    fn bash_precmd_reports_git_branch_via_nebula_title() {
        if !git_available() {
            return;
        }
        let home = tempfile::tempdir().unwrap();
        let repo = home.path().join("repo");
        std::fs::create_dir(&repo).unwrap();
        init_git_repo(&repo);
        let script = home.path().join("integration.bash");
        std::fs::write(&script, include_str!("../../res/shell/bashrc")).unwrap();

        let output = std::process::Command::new("/bin/bash")
            .args([
                "-c",
                // PROMPT_COMMAND normally primes `_nebula_last_status` via the
                // capture hook before `_nebula_precmd` runs; do it by hand here.
                // Two precmds pin the change-edge contract for the root var.
                r#"source "$2"; cd "$1"; _nebula_last_status=0; _nebula_precmd; _nebula_precmd"#,
                "nebula-title-test",
            ])
            .arg(&repo)
            .arg(&script)
            .env("HOME", home.path())
            .env("TERM", "xterm-256color")
            .output()
            .unwrap();
        assert!(output.status.success(), "{:?}", output.stderr);
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(
            stdout.contains(&format!("\x1b]2;NEBULA|{}|nebula-test-branch\x07", repo.display())),
            "repo title missing: {stdout:?}"
        );
        assert_eq!(
            stdout.matches("SetUserVar=pebrel_git_root=").count(),
            1,
            "the root var rides change edges only: {stdout:?}"
        );
        use base64::Engine as _;
        let physical_root = base64::engine::general_purpose::STANDARD
            .encode(repo.canonicalize().unwrap().display().to_string());
        assert!(
            stdout.contains(&format!("\x1b]1337;SetUserVar=pebrel_git_root={physical_root}\x07")),
            "repo root missing: {stdout:?}"
        );
    }

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
