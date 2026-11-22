//! Windows PTY command-line encoding and owned-shell refresh policy.

use super::{powershell_launch, resolved_default_shell};
use crate::tty::Options;

// Modified per stdlib implementation.
// https://github.com/rust-lang/rust/blob/6707bf0f59485cf054ac1095725df43220e4be20/library/std/src/sys/args/windows.rs#L174
fn push_escaped_arg(cmd: &mut String, arg: &str) {
    let arg_bytes = arg.as_bytes();
    let quote = arg_bytes.iter().any(|c| *c == b' ' || *c == b'\t') || arg_bytes.is_empty();
    if quote {
        cmd.push('"');
    }

    let mut backslashes: usize = 0;
    for x in arg.chars() {
        if x == '\\' {
            backslashes += 1;
        } else {
            if x == '"' {
                // Add n+1 backslashes to total 2n+1 before internal '"'.
                cmd.extend((0..=backslashes).map(|_| '\\'));
            }
            backslashes = 0;
        }
        cmd.push(x);
    }

    if quote {
        // Add n backslashes to total 2n before ending '"'.
        cmd.extend((0..backslashes).map(|_| '\\'));
        cmd.push('"');
    }
}

pub(super) fn cmdline(config: &Options) -> String {
    let using_default_shell = config.shell.is_none();
    let default_shell = using_default_shell.then(resolved_default_shell);
    let refreshed =
        config.shell.as_ref().filter(|_| !config.escape_args).and_then(powershell_launch::refresh);
    let shell = refreshed.as_ref().or(config.shell.as_ref()).or(default_shell.as_ref()).unwrap();

    let mut cmd = String::new();
    push_escaped_arg(&mut cmd, &shell.program);

    for arg in &shell.args {
        cmd.push(' ');
        if config.escape_args || using_default_shell {
            push_escaped_arg(&mut cmd, arg);
        } else {
            cmd.push_str(arg)
        }
    }
    cmd
}

#[cfg(test)]
mod tests {
    use super::super::powershell_integration_args;
    use super::*;
    use crate::tty::Shell;

    #[test]
    fn literal_argv_does_not_rewrite_a_managed_bootstrap_path() {
        let args = powershell_integration_args(
            vec!["-NoLogo".into()],
            &std::env::temp_dir().join("pebrel_prompt.ps1"),
        );
        let shell = Shell::new("powershell.exe".into(), args);
        assert!(super::powershell_launch::refresh(&shell).is_some());
        let mut expected = String::new();
        push_escaped_arg(&mut expected, shell.program());
        for arg in shell.args() {
            expected.push(' ');
            push_escaped_arg(&mut expected, arg);
        }
        let options = Options { shell: Some(shell), escape_args: true, ..Default::default() };
        assert_eq!(cmdline(&options), expected);
    }

    #[test]
    fn test_escape() {
        let test_set = vec![
            // Basic cases - no escaping needed
            ("abc", "abc"),
            // Cases requiring quotes (space/tab)
            ("", "\"\""),
            (" ", "\" \""),
            ("ab c", "\"ab c\""),
            ("ab\tc", "\"ab\tc\""),
            // Cases with backslashes only (no spaces, no quotes) - no quotes added
            ("ab\\c", "ab\\c"),
            // Cases with quotes only (no spaces) - quotes escaped but no outer quotes
            ("ab\"c", "ab\\\"c"),
            ("\"", "\\\""),
            ("a\"b\"c", "a\\\"b\\\"c"),
            // Cases requiring both quotes and escaping (contains spaces)
            ("ab \"c", "\"ab \\\"c\""),
            ("a \"b\" c", "\"a \\\"b\\\" c\""),
            // Complex real-world cases
            ("C:\\Program Files\\", "\"C:\\Program Files\\\\\""),
            ("C:\\Program Files\\a.txt", "\"C:\\Program Files\\a.txt\""),
            (
                r#"sh -c "cd /home/user; ARG='abc' \""'${SHELL:-sh}" -i -c '"'echo hello'""#,
                r#""sh -c \"cd /home/user; ARG='abc' \\\"\"'${SHELL:-sh}\" -i -c '\"'echo hello'\"""#,
            ),
        ];

        for (input, expected) in test_set {
            let mut escaped_arg = String::new();
            push_escaped_arg(&mut escaped_arg, input);
            assert_eq!(escaped_arg, expected, "Failed for input: {}", input);
        }
    }

    #[test]
    fn test_cmdline() {
        let mut options = Options {
            shell: Some(Shell {
                program: "echo".to_string(),
                args: vec!["hello world".to_string()],
            }),
            working_directory: None,
            drain_on_exit: true,
            env: Default::default(),
            env_is_complete: false,
            escape_args: false,
        };
        assert_eq!(cmdline(&options), "echo hello world");

        options.escape_args = true;
        assert_eq!(cmdline(&options), "echo \"hello world\"");
    }
}
