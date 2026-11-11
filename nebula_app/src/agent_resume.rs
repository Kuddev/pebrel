//! Safe argument parsing and shell quoting for Agent session resume and fork rules.

use crate::display::side_panel::PathQuote;

const MAX_ARGS: usize = 32;

pub(crate) fn parse(value: &str) -> Option<Vec<String>> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Some(Vec::new());
    }
    if trimmed.chars().any(char::is_control) {
        return None;
    }
    let args = if trimmed.starts_with('[') {
        serde_json::from_str::<Vec<String>>(trimmed).ok()?
    } else {
        tokenize_cli_suffix(trimmed)?
    };
    if args.len() > MAX_ARGS || args.iter().any(|arg| arg.chars().any(char::is_control)) {
        return None;
    }
    Some(args)
}

fn tokenize_cli_suffix(input: &str) -> Option<Vec<String>> {
    let mut args = Vec::new();
    let mut chars = input.chars().peekable();
    while let Some(&ch) = chars.peek() {
        if ch.is_ascii_whitespace() {
            chars.next();
            continue;
        }
        let mut current = String::new();
        while let Some(&ch) = chars.peek() {
            if ch.is_ascii_whitespace() {
                break;
            }
            chars.next();
            match ch {
                '\'' => {
                    let mut closed = false;
                    for inner in chars.by_ref() {
                        if inner == '\'' {
                            closed = true;
                            break;
                        }
                        current.push(inner);
                    }
                    if !closed {
                        return None;
                    }
                },
                '"' => {
                    let mut closed = false;
                    while let Some(inner) = chars.next() {
                        if inner == '"' {
                            closed = true;
                            break;
                        }
                        if inner == '\\'
                            && let Some(&escaped @ ('"' | '\\')) = chars.peek()
                        {
                            chars.next();
                            current.push(escaped);
                            continue;
                        }
                        current.push(inner);
                    }
                    if !closed {
                        return None;
                    }
                },
                ';' | '|' | '&' | '`' | '$' | '<' | '>' | '(' | ')' => return None,
                other => current.push(other),
            }
        }
        args.push(current);
    }
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
    fn natural_suffix_and_json_array_append_quoted_arguments_across_shells() {
        assert_eq!(
            append("codex resume 'abc'".to_owned(), "--yolo", PathQuote::Posix, Some("/bin/zsh"),),
            Some("codex resume 'abc' --yolo".to_owned())
        );
        assert_eq!(
            append(
                "agy --conversation 'abc'".to_owned(),
                "--dangerously-skip-permissions --model \"gemini-2.5-pro\"",
                PathQuote::Posix,
                Some("/bin/zsh"),
            ),
            Some(
                "agy --conversation 'abc' --dangerously-skip-permissions --model gemini-2.5-pro"
                    .to_owned()
            )
        );
        assert_eq!(
            append(
                "claude --resume 'abc'".to_owned(),
                r#"["--dangerously-skip-permissions","it's literal; rm -rf /"]"#,
                PathQuote::Posix,
                Some("/bin/zsh"),
            ),
            Some(
                "claude --resume 'abc' --dangerously-skip-permissions 'it'\\''s literal; rm -rf /'"
                    .to_owned()
            )
        );
        assert_eq!(
            append(
                "codex resume 'abc'".to_owned(),
                r#"["--profile","it's & $(calc)"]"#,
                PathQuote::PowerShell,
                Some("pwsh"),
            ),
            Some("codex resume 'abc' --profile 'it''s & $(calc)'".to_owned())
        );
        assert_eq!(
            append(
                "codex resume \"abc\"".to_owned(),
                r#"["--profile","two words","a b\\"]"#,
                PathQuote::CommandPrompt,
                Some("cmd.exe"),
            ),
            Some("codex resume \"abc\" --profile \"two words\" \"a b\\\\\"".to_owned())
        );
        assert_eq!(
            append("codex resume 'abc'".to_owned(), r#"[""]"#, PathQuote::Posix, Some("/bin/sh")),
            Some("codex resume 'abc' ''".to_owned())
        );
    }

    #[test]
    fn shell_injection_and_malformed_inputs_are_rejected() {
        for invalid in [
            "--yolo; echo pwn",
            "--yolo && echo pwn",
            "--yolo | cat",
            "--yolo $(whoami)",
            "--yolo `whoami`",
            "--unclosed 'quote",
            "--unclosed \"quote",
            "[invalid",
            "[\"--ok\", 1]",
            "[\"--bad\\nline\"]",
        ] {
            assert_eq!(
                append("claude --resume 'abc'".to_owned(), invalid, PathQuote::Posix, None),
                None,
                "{invalid}"
            );
        }
        assert_eq!(
            append(
                "claude --resume \"abc\"".to_owned(),
                r#"["%PATH%"]"#,
                PathQuote::CommandPrompt,
                Some("cmd.exe"),
            ),
            None
        );
    }
}
