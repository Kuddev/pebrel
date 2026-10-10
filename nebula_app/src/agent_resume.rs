//! Literal argument arrays for automatic recovery, quoted for the pane's shell.

use crate::display::side_panel::PathQuote;

pub(crate) fn parse(value: &str) -> Option<Vec<String>> {
    if value.trim().is_empty() {
        return Some(Vec::new());
    }
    let args: Vec<String> = serde_json::from_str(value).ok()?;
    if args.is_empty() {
        return Some(args);
    }
    if args.len() > 32 || args.iter().any(|arg| arg.chars().any(char::is_control)) {
        return None;
    }
    crate::platform::agent_resume::quote_args(&args, PathQuote::Posix, None)?;
    Some(args)
}

pub(crate) fn append(
    command: String,
    value: &str,
    shell: PathQuote,
    program: Option<&str>,
) -> Option<String> {
    let args = parse(value)?;
    if args.is_empty() {
        return Some(command);
    }
    let quoted = crate::platform::agent_resume::quote_args(&args, shell, program)?;
    Some(format!("{command} {}", quoted.trim_end()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_arguments_cannot_become_shell_expressions() {
        let value = r#"["--config", "name=a b;$(touch nope)&|", "it's"]"#;
        assert_eq!(
            append("codex resume id".into(), value, PathQuote::Posix, None).unwrap(),
            "codex resume id --config 'name=a b;$(touch nope)&|' 'it'\\''s'"
        );
        assert_eq!(
            append("codex resume id".into(), value, PathQuote::PowerShell, None).unwrap(),
            "codex resume id --config 'name=a b;$(touch nope)&|' 'it''s'"
        );
        assert_eq!(
            append("codex resume id".into(), r#"["a b\\"]"#, PathQuote::CommandPrompt, None)
                .unwrap(),
            "codex resume id \"a b\\\\\""
        );
        for value in [r#"["%PATH%"]"#, r#"["!VAR!"]"#, r#"["a\"b"]"#] {
            assert!(
                append("codex resume id".into(), value, PathQuote::CommandPrompt, None).is_none()
            );
        }
        assert_eq!(parse(r#"[""]"#), Some(vec![String::new()]));
        assert_eq!(
            append("codex resume id".into(), r#"[""]"#, PathQuote::Posix, None),
            Some("codex resume id ''".into())
        );
        for value in ["--yolo", r#"[1]"#, r#"[null]"#, r#"["\n"]"#] {
            assert!(parse(value).is_none());
        }
    }
}
