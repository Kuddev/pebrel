//! Native argv quoting for automatic Agent recovery.

use crate::display::side_panel::{PathQuote, drop_text_for_paths};

pub(crate) fn quote_args(
    args: &[String],
    shell: PathQuote,
    program: Option<&str>,
) -> Option<String> {
    #[cfg(windows)]
    let legacy_args = if matches!(shell, PathQuote::PowerShell)
        && program.is_some_and(|program| {
            let name = program.rsplit(['/', '\\']).next().unwrap_or(program);
            name.eq_ignore_ascii_case("powershell") || name.eq_ignore_ascii_case("powershell.exe")
        }) {
        Some(
            args.iter()
                .map(|arg| {
                    let quoted =
                        crate::platform::elevation::quoted_argument(std::ffi::OsStr::new(arg))
                            .ok()?;
                    String::from_utf16(&quoted).ok()
                })
                .collect::<Option<Vec<_>>>()?,
        )
    } else {
        None
    };
    #[cfg(windows)]
    let args = legacy_args.as_deref().unwrap_or(args);
    #[cfg(not(windows))]
    let _ = program;
    let quoted = if matches!(shell, PathQuote::CommandPrompt) {
        let mut quoted = String::new();
        // CMD passes quotes to the CLI's argv parser. A quoted argument ending in
        // a backslash needs doubled trailing slashes before its closing quote.
        for arg in args {
            let mut item = if arg.is_empty() {
                "\"\" ".to_owned()
            } else {
                drop_text_for_paths(std::slice::from_ref(arg), shell)?
            };
            if item.starts_with('"') {
                let trailing = arg.chars().rev().take_while(|ch| *ch == '\\').count();
                item.insert_str(item.len() - 2, &"\\".repeat(trailing));
            }
            quoted.push_str(&item);
        }
        quoted
    } else {
        let mut quoted = String::new();
        for arg in args {
            if arg.is_empty() {
                quoted.push_str("'' ");
            } else {
                quoted.push_str(&drop_text_for_paths(std::slice::from_ref(arg), shell)?);
            }
        }
        quoted
    };
    if quoted.len() > 64 * 1024 {
        return None;
    }
    Some(quoted)
}

#[cfg(all(test, windows))]
mod native_tests {
    use super::*;
    use crate::agent_resume::append;

    #[test]
    fn windows_powershell_and_pwsh_deliver_the_exact_native_argument_array() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("argv.rs");
        let executable = root.path().join("argv probe.exe");
        std::fs::write(
            &source,
            r#"fn main() { print!("{:?}", std::env::args().skip(1).collect::<Vec<_>>()); }"#,
        )
        .unwrap();
        assert!(
            std::process::Command::new("rustc")
                .arg(&source)
                .arg("-o")
                .arg(&executable)
                .status()
                .unwrap()
                .success()
        );
        let args = [
            "",
            "--config",
            "model_provider=\"custom\"",
            "a\"b",
            "a b\\",
            "x\\\"y",
            "it's & $(Write-Output nope)",
            "",
        ];
        let value = serde_json::to_string(&args).unwrap();
        let quoted_exe = drop_text_for_paths(
            &[executable.to_string_lossy().into_owned()],
            PathQuote::PowerShell,
        )
        .unwrap();
        for shell in ["powershell.exe", "pwsh.exe"] {
            let line = append(
                format!("& {}", quoted_exe.trim_end()),
                &value,
                PathQuote::PowerShell,
                Some(shell),
            )
            .unwrap();
            let mut command = std::process::Command::new(shell);
            command.args(["-NoProfile", "-NonInteractive", "-Command", &line]);
            let output = crate::platform::process_output::read_cancellable(
                command,
                std::time::Duration::from_secs(15),
                8192,
                &|| false,
            )
            .unwrap();
            let actual = String::from_utf8(output).unwrap();
            assert_eq!(actual.trim(), format!("{args:?}"), "{shell}: {line}");
        }
    }
}
