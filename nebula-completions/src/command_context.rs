//! Literal command context and byte ranges, independent of terminal rendering.

use crate::{Span, Suggestion};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellSyntax {
    Posix,
    PowerShell,
    Cmd,
    /// Unknown shells only share plain, unquoted arguments.
    Literal,
}

impl ShellSyntax {
    pub fn for_program(program: &str) -> Self {
        let name = program.rsplit(['/', '\\']).next().unwrap_or(program).to_ascii_lowercase();
        match name.trim_end_matches(".exe") {
            "bash" | "zsh" | "sh" | "dash" | "fish" => Self::Posix,
            "pwsh" | "powershell" => Self::PowerShell,
            "cmd" => Self::Cmd,
            _ => Self::Literal,
        }
    }
}

#[derive(Debug)]
struct Word {
    value: String,
    span: Span,
    quote: Option<char>,
    closed: bool,
}

/// A proven branch argument, not a guess based on the final whitespace token.
#[derive(Debug)]
pub struct GitSwitchContext {
    pub directories: Vec<String>,
    target: Word,
    syntax: ShellSyntax,
    pub include_busy: bool,
}

impl GitSwitchContext {
    pub fn parse(line: &str, cursor: usize, syntax: ShellSyntax) -> Option<Self> {
        // 当前终端只证明行尾的输入真值；中线编辑不能把右侧文本悄悄覆盖。
        if cursor != line.len() || line.len() > 4096 {
            return None;
        }
        let command = line.trim_start_matches([' ', '\t']);
        if !["git ", "git\t", "git.exe ", "git.exe\t"]
            .iter()
            .any(|prefix| command.starts_with(prefix))
        {
            return None;
        }
        let mut words = words(line, syntax)?;
        let target = words.pop()?;
        let mut args = words.iter();
        if !matches!(args.next()?.value.as_str(), "git" | "git.exe") {
            return None;
        }
        let mut directories = Vec::new();
        loop {
            match args.next()?.value.as_str() {
                "-C" => directories.push(args.next()?.value.clone()),
                "switch" => break,
                _ => return None,
            }
        }
        let mut include_busy = false;
        let mut options = true;
        while let Some(arg) = args.next() {
            match arg.value.as_str() {
                "--" if options => options = false,
                "-c" | "-C" | "--create" | "--force-create" if options => {
                    args.next()?; // 新分支名由用户指定；其后的 start-point 才接受现有分支。
                    include_busy = true;
                },
                "--conflict" if options => {
                    if !matches!(args.next()?.value.as_str(), "merge" | "diff3" | "zdiff3") {
                        return None;
                    }
                },
                "--ignore-other-worktrees" if options => include_busy = true,
                "-d" | "--detach" if options => include_busy = true,
                "-q"
                | "--quiet"
                | "-m"
                | "--merge"
                | "-f"
                | "--force"
                | "--discard-changes"
                | "--guess"
                | "--no-guess"
                | "--no-track"
                | "--progress"
                | "--no-progress"
                | "--overwrite-ignore"
                | "--no-overwrite-ignore"
                    if options => {},
                _ => return None,
            }
        }
        if target.value.starts_with('-') && options {
            return None;
        }
        Some(Self { directories, target, syntax, include_busy })
    }

    pub fn candidate(&self, branch: &str) -> Option<Suggestion> {
        if !branch.starts_with(&self.target.value) || branch.starts_with('-') {
            return None;
        }
        let quote = self.target.quote;
        let safe =
            branch.chars().all(|c| c.is_alphanumeric() || matches!(c, '/' | '.' | '_' | '-'));
        let value = match (quote, self.syntax, safe) {
            (Some('\''), ShellSyntax::Posix, _) => format!("'{}'", branch.replace('\'', "'\\''")),
            (Some('\''), ShellSyntax::PowerShell, _) => format!("'{}'", branch.replace('\'', "''")),
            (Some('"'), ShellSyntax::Posix, _) => format!(
                "\"{}\"",
                branch
                    .replace('\\', "\\\\")
                    .replace('$', "\\$")
                    .replace('`', "\\`")
                    .replace('"', "\\\"")
            ),
            (Some('"'), ShellSyntax::PowerShell, _) => {
                format!("\"{}\"", branch.replace('`', "``").replace('$', "`$").replace('"', "`\""))
            },
            (Some('"'), ShellSyntax::Cmd, true) => format!("\"{branch}\""),
            (None, _, true) => branch.to_owned(),
            (None, ShellSyntax::Posix, false) => format!("'{}'", branch.replace('\'', "'\\''")),
            (None, ShellSyntax::PowerShell, false) => format!("'{}'", branch.replace('\'', "''")),
            // CMD 的百分号/延迟展开依赖运行中的选项，不能伪造通用转义。
            _ => return None,
        };
        Some(Suggestion {
            value,
            display_override: Some(branch.to_owned()),
            span: self.target.span,
            append_whitespace: false,
            ..Default::default()
        })
    }
}

fn words(line: &str, syntax: ShellSyntax) -> Option<Vec<Word>> {
    let mut result = Vec::new();
    let mut chars = line.char_indices().peekable();
    while let Some((start, first)) = chars.next() {
        if matches!(first, ' ' | '\t') {
            continue;
        }
        let quote = match first {
            '"' if syntax != ShellSyntax::Literal => Some(first),
            '\'' if matches!(syntax, ShellSyntax::Posix | ShellSyntax::PowerShell) => Some(first),
            _ => None,
        };
        let mut word = Word {
            value: String::new(),
            span: Span::new(start, line.len()),
            quote,
            closed: quote.is_none(),
        };
        let mut current = if quote.is_some() { chars.next() } else { Some((start, first)) };
        while let Some((offset, ch)) = current {
            if quote == Some(ch) {
                if syntax == ShellSyntax::PowerShell
                    && chars.peek().is_some_and(|(_, next)| *next == ch)
                {
                    chars.next();
                    word.value.push(ch);
                } else {
                    word.closed = true;
                    word.span.end = offset + ch.len_utf8();
                    if chars.peek().is_some_and(|(_, next)| !matches!(next, ' ' | '\t')) {
                        return None;
                    }
                    break;
                }
            } else if quote.is_none() && matches!(ch, ' ' | '\t') {
                word.span.end = offset;
                break;
            } else {
                let escape = match syntax {
                    ShellSyntax::Posix => ch == '\\' && quote != Some('\''),
                    ShellSyntax::PowerShell => ch == '`' && quote != Some('\''),
                    _ => false,
                };
                if escape {
                    let (_, escaped) = chars.next()?;
                    if escaped.is_control() {
                        return None;
                    }
                    if syntax == ShellSyntax::Posix
                        && quote == Some('"')
                        && !matches!(escaped, '$' | '`' | '"' | '\\')
                    {
                        word.value.push(ch);
                    }
                    word.value.push(escaped);
                } else {
                    // 不执行展开、子命令或复合语句；已引用的字面量按 shell 方言判断。
                    let expansion = match syntax {
                        ShellSyntax::Cmd => matches!(ch, '%' | '!' | '^'),
                        ShellSyntax::Posix | ShellSyntax::PowerShell => {
                            quote != Some('\'') && matches!(ch, '$' | '`')
                        },
                        ShellSyntax::Literal => {
                            !ch.is_alphanumeric() && !matches!(ch, '/' | '.' | '_' | '-')
                        },
                    };
                    if ch.is_control()
                        || expansion
                        || quote.is_none()
                            && matches!(
                                ch,
                                ';' | '|'
                                    | '&'
                                    | '<'
                                    | '>'
                                    | '('
                                    | ')'
                                    | '{'
                                    | '}'
                                    | '['
                                    | ']'
                                    | '*'
                                    | '?'
                                    | '#'
                                    | '~'
                                    | '\''
                                    | '"'
                            )
                    {
                        return None;
                    }
                    word.value.push(ch);
                }
            }
            current = chars.next();
        }
        if !word.closed && chars.peek().is_some() {
            return None;
        }
        result.push(word);
        if result.len() > 64 {
            return None;
        }
    }
    if line.ends_with([' ', '\t']) && result.last().is_none_or(|word| word.closed) {
        result.push(Word {
            value: String::new(),
            span: Span::new(line.len(), line.len()),
            quote: None,
            closed: true,
        });
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_position_preserves_directories_options_and_utf8_ranges() {
        for syntax in [ShellSyntax::Posix, ShellSyntax::PowerShell, ShellSyntax::Cmd] {
            for line in [
                "git switch ",
                "git switch --quiet fe",
                "git switch -c new fe",
                "git switch -- fe",
                "git -C \"中文 repo\" switch \"fe\"",
            ] {
                let context = GitSwitchContext::parse(line, line.len(), syntax).unwrap();
                let candidate = context.candidate("feature/中文").unwrap();
                assert!(line.is_char_boundary(candidate.span.start));
                assert_eq!(candidate.span.end, line.len());
                assert!(candidate.value.contains("feature/中文"));
                if line.contains("-C") {
                    assert_eq!(context.directories, ["中文 repo"]);
                }
            }
        }
    }

    #[test]
    fn does_not_treat_other_arguments_or_shell_expressions_as_branches() {
        for line in [
            "git switch",
            "git switch -c ",
            "git switch --orphan new",
            "git switch --conflict ",
            "git switch main ",
            "git switch --track ",
            "echo git switch fe",
            "git -c alias.switch=x switch fe",
            "git switch $(echo fe)",
            "git switch fe; pwd",
            "git switch fe | cat",
        ] {
            assert!(
                GitSwitchContext::parse(line, line.len(), ShellSyntax::Posix).is_none(),
                "{line}"
            );
        }
        assert!(GitSwitchContext::parse("git switch feat", 13, ShellSyntax::Posix).is_none());
    }

    #[test]
    fn incomplete_quotes_and_shell_escaping_produce_literal_arguments() {
        let line = "git switch 'fe";
        for (syntax, expected) in
            [(ShellSyntax::Posix, "'feat'\\''$x'"), (ShellSyntax::PowerShell, "'feat''$x'")]
        {
            let context = GitSwitchContext::parse(line, line.len(), syntax).unwrap();
            assert_eq!(context.candidate("feat'$x").unwrap().value, expected);
        }
        let line = "git -C repo\\ name switch fe";
        assert_eq!(
            GitSwitchContext::parse(line, line.len(), ShellSyntax::Posix).unwrap().directories,
            ["repo name"]
        );
        let line = "git -C \"D:\\repo name\" switch fe";
        assert_eq!(
            GitSwitchContext::parse(line, line.len(), ShellSyntax::PowerShell).unwrap().directories,
            ["D:\\repo name"]
        );
    }
}
