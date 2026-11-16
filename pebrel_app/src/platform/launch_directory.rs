//! Keep guest paths out of host filesystem validation and process creation.

use std::path::{Path, PathBuf};

use pebrel_terminal::tty::{Options, Shell};

pub(crate) fn is_wsl(shell: Option<&Shell>) -> bool {
    shell.is_some_and(|shell| is_wsl_program(shell.program()))
}

pub(crate) fn is_wsl_program(program: &str) -> bool {
    cfg!(windows) && crate::shell_detect::is_wsl_launcher(program)
}

pub(crate) fn guest_path(path: &Path) -> Option<&str> {
    let path = path.to_str()?;
    (path.starts_with('/') && !path.starts_with("//") && !path.chars().any(char::is_control))
        .then_some(path)
}

/// The resident selects the shell; a Windows caller cannot classify `/path`
/// before that selection. Preserve both interpretations until it does.
pub(crate) fn cli_directory(path: &Path) -> std::io::Result<(PathBuf, Option<PathBuf>)> {
    let host = std::path::absolute(path)?;
    if cfg!(windows) && guest_path(path).is_some() {
        Ok((path.to_owned(), Some(host)))
    } else {
        Ok((host, None))
    }
}

/// Explicit guest paths are validated by the selected guest when it starts.
/// Host paths keep the existing directory/existence and absolute-path checks.
pub(crate) fn validate(path: PathBuf, wsl: bool) -> Option<PathBuf> {
    if wsl && let Some(guest) = guest_path(&path) {
        return crate::shell_detect::wsl_accepts_startup_arg(guest).then_some(path);
    }
    path.is_dir().then(|| std::path::absolute(path).ok()).flatten()
}

pub(crate) fn wsl_argument(path: &Path) -> Option<String> {
    if let Some(guest) = guest_path(path) {
        return Some(guest.to_owned());
    }
    std::path::absolute(path).ok()?.into_os_string().into_string().ok()
}

/// Apply the directory policy only at the final Windows PTY boundary. Earlier
/// copy/restore decisions can first encode their verified guest cwd in --cd.
pub(crate) fn prepare_wsl(options: &mut Options, configured: Option<&str>) {
    #[cfg(windows)]
    let literal = options.escape_args;
    #[cfg(not(windows))]
    let literal = false;
    prepare_wsl_arguments(options, configured, literal);
}

fn prepare_wsl_arguments(options: &mut Options, configured: Option<&str>, literal: bool) {
    let Some(shell) = options.shell.as_ref() else { return };
    let Some(launch) = crate::shell_detect::wsl_launch(shell.program(), shell.args()) else {
        return;
    };
    let guest = options.working_directory.as_deref().and_then(guest_path);
    let directory = if launch.chooses_directory {
        None
    } else if let Some(guest) = guest {
        Some(guest.to_owned())
    } else if launch.command.is_some() {
        None
    } else {
        configured
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .and_then(|path| validate(PathBuf::from(path), true))
            .and_then(|path| path.into_os_string().into_string().ok())
            .or_else(|| Some("~".to_owned()))
    };
    if let Some(directory) = directory {
        let args = if literal {
            crate::shell_detect::wsl_literal_args_in_directory(
                shell.program(),
                shell.args(),
                &directory,
            )
        } else {
            crate::shell_detect::wsl_args_in_directory(shell.program(), shell.args(), &directory)
        };
        if let Some(args) = args {
            options.shell = Some(Shell::new(shell.program().to_owned(), args));
        }
    }
    if guest.is_some() {
        options.working_directory = None;
    }
}

#[cfg(test)]
pub(crate) fn test_posix_echo_alias(directory: &Path) -> Option<PathBuf> {
    #[cfg(unix)]
    {
        let path = directory.join("wsl");
        std::os::unix::fs::symlink("/bin/echo", &path).unwrap();
        Some(path)
    }
    #[cfg(not(unix))]
    {
        let _ = directory;
        None
    }
}

#[cfg(test)]
pub(crate) fn test_non_utf8_directory(root: &Path) -> Option<PathBuf> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        Some(root.join(std::ffi::OsString::from_vec(b"project-\xff".to_vec())))
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        Some(root.join(std::ffi::OsString::from_wide(&[b'p' as u16, 0xd800])))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = root;
        None
    }
}

#[cfg(test)]
pub(crate) fn test_ambiguous_cli_directory(host: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let host = host.to_str().unwrap().trim_start_matches(r"\\?\");
        assert_eq!(host.as_bytes()[1], b':', "fixture needs a native drive path");
        PathBuf::from(host[2..].replace('\\', "/"))
    }
    #[cfg(not(windows))]
    host.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_argv_keeps_the_guest_directory_unquoted_until_native_encoding() {
        let mut launch = options(
            &["-d", "Team Linux", "--exec", "tool", "two words", ""],
            Some("/guest directory"),
        );
        prepare_wsl_arguments(&mut launch, None, true);
        assert_eq!(
            launch.shell.unwrap().args(),
            &["--cd", "/guest directory", "-d", "Team Linux", "--exec", "tool", "two words", ""]
        );
        assert!(launch.working_directory.is_none());
    }

    #[test]
    fn cli_paths_keep_guest_and_native_interpretations_until_shell_resolution() {
        let guest = Path::new("/home/pebrel-fixture/project space");
        let (cwd, host) = cli_directory(guest).unwrap();
        if cfg!(windows) {
            assert_eq!(cwd, guest);
            assert_eq!(host, Some(std::path::absolute(guest).unwrap()));
        } else {
            assert_eq!(cwd, std::path::absolute(guest).unwrap());
            assert!(host.is_none());
        }
        let relative = Path::new("project space");
        let (cwd, host) = cli_directory(relative).unwrap();
        assert_eq!(cwd, std::env::current_dir().unwrap().join(relative));
        assert!(host.is_none());
        assert_eq!(is_wsl_program("wsl.exe"), cfg!(windows));
        assert!(!is_wsl_program("native-profile.exe"));
    }

    fn options(args: &[&str], cwd: Option<&str>) -> Options {
        Options {
            shell: Some(Shell::new(
                "wsl.exe".into(),
                args.iter().map(|arg| (*arg).into()).collect(),
            )),
            working_directory: cwd.map(PathBuf::from),
            ..Default::default()
        }
    }

    #[test]
    fn inherited_host_cwd_uses_guest_home_without_changing_distribution_or_user() {
        for cwd in [r"C:\Users\example", r"D:\Apps\Pebrel"] {
            let mut launch = options(&["-d", "Team Linux", "--user", "guest"], Some(cwd));
            prepare_wsl(&mut launch, None);
            assert_eq!(
                launch.shell.unwrap().args(),
                &["--cd", "~", "-d", "Team Linux", "--user", "guest"]
            );
            assert_eq!(launch.working_directory, Some(PathBuf::from(cwd)));
        }
    }

    #[test]
    fn explicit_and_configured_guest_paths_do_not_require_a_host_directory() {
        let guest = "/home/pebrel-nonexistent-fixture/project space";
        assert_eq!(validate(PathBuf::from(guest), true), Some(PathBuf::from(guest)));
        assert_eq!(validate(PathBuf::from(guest), false), None);
        for explicit in [false, true] {
            let mut launch = options(&["-d", "Ubuntu"], explicit.then_some(guest));
            prepare_wsl(&mut launch, Some(if explicit { "/different" } else { guest }));
            assert_eq!(
                launch.shell.unwrap().args(),
                &["--cd", &format!("\"{guest}\""), "-d", "Ubuntu"]
            );
            assert!(launch.working_directory.is_none());
        }
    }

    #[test]
    fn selected_directories_and_direct_commands_are_not_overridden_by_defaults() {
        for args in [
            vec!["--cd", "/selected", "-d", "Ubuntu"],
            vec!["~", "-d", "Ubuntu"],
            vec!["--cd=/selected", "--user=guest"],
            vec!["-d", "Ubuntu", "--exec", "printf", "--cd", "/command-argument"],
            vec!["--", "pwd"],
        ] {
            let mut launch = options(&args, Some(r"C:\inherited"));
            prepare_wsl(&mut launch, Some("/configured"));
            assert_eq!(launch.shell.unwrap().args(), args);
        }
    }

    #[test]
    fn configured_host_directory_is_explicit_and_missing_setting_falls_back_home() {
        let directory = tempfile::tempdir().unwrap();
        let configured = directory.path().to_str().unwrap();
        let mut launch = options(&[], None);
        prepare_wsl(&mut launch, Some(configured));
        assert_eq!(launch.shell.unwrap().args(), &["--cd", configured]);
        let mut launch = options(&[], None);
        prepare_wsl(&mut launch, Some(r"Z:\missing-pebrel-fixture"));
        assert_eq!(launch.shell.unwrap().args(), &["--cd", "~"]);
    }
}
