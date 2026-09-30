//! Native host and drive observations used when resolving local file links.
//! URI parsing and the decision to open a link remain in the file-link layer.

pub(crate) fn completion_case_sensitive() -> bool {
    !cfg!(windows)
}

pub(crate) fn completion_spelling(
    path: &str,
    syntax: nebula_completions::command_context::ShellSyntax,
) -> String {
    use nebula_completions::command_context::ShellSyntax;
    // PowerShell 5.1 给原生程序重建 argv 时，也会误解带空格目录尾部的反斜杠。
    if cfg!(windows)
        && (matches!(syntax, ShellSyntax::Posix | ShellSyntax::Cmd)
            || syntax == ShellSyntax::PowerShell && path.chars().any(char::is_whitespace))
    {
        path.replace('\\', "/")
    } else {
        path.to_owned()
    }
}

/// Match the machine names supplied by the native terminal environment.
pub(crate) fn matches_hostname(host: &str) -> bool {
    #[cfg(windows)]
    if std::env::var("COMPUTERNAME").is_ok_and(|name| name.eq_ignore_ascii_case(host)) {
        return true;
    }
    #[cfg(unix)]
    {
        for variable in ["HOSTNAME", "HOST"] {
            if std::env::var(variable).is_ok_and(|name| name.eq_ignore_ascii_case(host)) {
                return true;
            }
        }
        if system_hostname().is_some_and(|name| name.eq_ignore_ascii_case(host)) {
            return true;
        }
    }
    false
}

/// Whether a Windows drive is mounted; Unix paths never imply a Windows drive.
pub(crate) fn drive_exists(letter: char) -> bool {
    #[cfg(windows)]
    {
        letter.is_ascii_alphabetic() && std::path::Path::new(&format!("{letter}:\\")).exists()
    }
    #[cfg(not(windows))]
    {
        let _ = letter;
        false
    }
}

#[cfg(unix)]
fn system_hostname() -> Option<String> {
    let mut buffer = [0u8; 256];
    // SAFETY: the writable buffer has the exact length passed to gethostname.
    // Decoding below also rejects an unterminated or invalid UTF-8 response.
    if unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len()) } != 0 {
        return None;
    }
    hostname_from_buffer(&buffer).map(str::to_owned)
}

#[cfg(any(unix, test))]
fn hostname_from_buffer(buffer: &[u8]) -> Option<&str> {
    let end = buffer.iter().position(|&byte| byte == 0)?;
    let name = std::str::from_utf8(&buffer[..end]).ok()?;
    (!name.is_empty()).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::hostname_from_buffer;

    #[cfg(windows)]
    #[test]
    fn completion_directory_quotes_survive_native_cmd_and_powershell_arguments() {
        use nebula_completions::command_context::{CommandContext, ShellSyntax};
        use std::os::windows::process::CommandExt;
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo name");
        std::fs::create_dir(&repo).unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q"])
                .current_dir(&repo)
                .status()
                .unwrap()
                .success()
        );
        for (shell, syntax) in
            [("cmd.exe", ShellSyntax::Cmd), ("powershell.exe", ShellSyntax::PowerShell)]
        {
            let input = CommandContext::parse("git -C re", 9, syntax).unwrap();
            let path = super::completion_spelling("repo name\\", syntax);
            let candidate = input.candidate(&path).unwrap();
            let line = format!("git -C {} rev-parse --is-inside-work-tree", candidate.value);
            let mut command = std::process::Command::new(shell);
            command.current_dir(root.path());
            if syntax == ShellSyntax::Cmd {
                command.args(["/d", "/c"]).raw_arg(&line);
            } else {
                command.args(["-NoProfile", "-NonInteractive", "-Command", &line]);
            }
            let output = crate::platform::process_output::read_cancellable(
                command,
                std::time::Duration::from_secs(15),
                8192,
                &|| false,
            )
            .unwrap();
            assert_eq!(std::str::from_utf8(&output).unwrap().trim(), "true", "{shell}: {line}");
        }
    }

    #[test]
    fn hostname_decoding_stops_at_the_native_terminator() {
        assert_eq!(hostname_from_buffer(b"workstation\0unused"), Some("workstation"));
    }

    #[test]
    fn hostname_decoding_rejects_incomplete_or_invalid_native_data() {
        assert_eq!(hostname_from_buffer(b"unterminated"), None);
        assert_eq!(hostname_from_buffer(b"\xff\0"), None);
        assert_eq!(hostname_from_buffer(b"\0"), None);
        assert_eq!(hostname_from_buffer(b""), None);
    }
}
