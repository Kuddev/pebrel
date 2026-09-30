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
    home: bool,
}

/// Literal arguments and the final editable word; never evaluates shell code.
#[derive(Debug)]
pub struct CommandContext {
    pub(crate) arguments: Vec<String>,
    home_arguments: Vec<usize>,
    target: Word,
    syntax: ShellSyntax,
}

impl CommandContext {
    pub fn parse(line: &str, cursor: usize, syntax: ShellSyntax) -> Option<Self> {
        Self::parse_words(line, cursor, syntax, false)
    }

    pub(crate) fn parse_with_home(line: &str, cursor: usize, syntax: ShellSyntax) -> Option<Self> {
        Self::parse_words(line, cursor, syntax, true)
    }

    pub(crate) fn argument_expands_home(&self, index: usize) -> bool {
        self.home_arguments.contains(&index)
    }

    fn parse_words(
        line: &str,
        cursor: usize,
        syntax: ShellSyntax,
        allow_home: bool,
    ) -> Option<Self> {
        // 终端目前只证明行尾输入，不能借补齐覆盖光标右侧的未知内容。
        if cursor != line.len() || line.len() > 4096 {
            return None;
        }
        let mut words = words(line, syntax)?;
        let target = words.pop()?;
        // 已完成参数若含展开，无法证明它指向哪个目录；目标词的 home 由路径来源处理。
        if words.iter().any(|word| !word.closed || word.home && !allow_home) {
            return None;
        }
        let home_arguments =
            words.iter().enumerate().filter_map(|(i, word)| word.home.then_some(i)).collect();
        Some(Self {
            arguments: words.into_iter().map(|word| word.value).collect(),
            home_arguments,
            target,
            syntax,
        })
    }

    pub fn prefix(&self) -> &str {
        &self.target.value
    }

    pub fn expands_home(&self) -> bool {
        self.target.home && (self.prefix() == "~" || self.prefix().starts_with("~/"))
    }

    /// Matching is separate from quoting so ranked/fuzzy results use the same edit contract.
    pub fn candidate(&self, value: &str) -> Option<Suggestion> {
        if value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
            return None;
        }
        let quote = self.target.quote;
        // PowerShell 将 -Fconfig.conf 拆成参数 -Fconfig 和 .conf；整个词需引用。
        // 普通选项仍保持裸写，否则 cmdlet 会把参数名当成位置参数。
        let parameter_split = self.syntax == ShellSyntax::PowerShell
            && value.starts_with('-')
            && value.contains(['.', ':']);
        let safe = !parameter_split
            && value.chars().all(|c| {
                c.is_alphanumeric()
                    || matches!(c, '/' | '.' | '_' | '-' | ':' | '=')
                    || c == '@'
                        && self.syntax != ShellSyntax::Literal
                        && (self.syntax != ShellSyntax::PowerShell || !value.starts_with('@'))
                    || c == ',' && matches!(self.syntax, ShellSyntax::Posix | ShellSyntax::Cmd)
                    || c == '\\'
                        && matches!(self.syntax, ShellSyntax::PowerShell | ShellSyntax::Cmd)
            });
        let cmd_quoted_safe = self.syntax == ShellSyntax::Cmd
            && value.chars().all(|c| {
                c.is_alphanumeric()
                    || matches!(c, '/' | '\\' | '.' | '_' | '-' | ':' | '=' | ' ' | '@' | ',')
            });
        let quoted = match (quote, self.syntax, safe) {
            (Some('\''), ShellSyntax::Posix, _) => format!("'{}'", value.replace('\'', "'\\''")),
            (Some('\''), ShellSyntax::PowerShell, _) => format!("'{}'", value.replace('\'', "''")),
            (Some('"'), ShellSyntax::Posix, _) => format!(
                "\"{}\"",
                value
                    .replace('\\', "\\\\")
                    .replace('$', "\\$")
                    .replace('`', "\\`")
                    .replace('"', "\\\"")
            ),
            (Some('"'), ShellSyntax::PowerShell, _) => {
                format!("\"{}\"", value.replace('`', "``").replace('$', "`$").replace('"', "`\""))
            },
            (_, ShellSyntax::Cmd, _) if cmd_quoted_safe && (quote.is_some() || !safe) => {
                // CMD 内建命令和外部 argv 对尾部反斜杠的规则不同；路径来源先统一为 /。
                if value.ends_with('\\') {
                    return None;
                }
                format!("\"{value}\"")
            },
            (None, _, true) => value.to_owned(),
            (None, ShellSyntax::Posix, false) => format!("'{}'", value.replace('\'', "'\\''")),
            (None, ShellSyntax::PowerShell, false) => format!("'{}'", value.replace('\'', "''")),
            // CMD 的百分号/延迟展开依赖运行中的选项，不能伪造通用转义。
            _ => return None,
        };
        Some(Suggestion {
            value: quoted,
            display_override: Some(value.to_owned()),
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
        let mut quote = match first {
            '"' if syntax != ShellSyntax::Literal => Some(first),
            '\'' if matches!(syntax, ShellSyntax::Posix | ShellSyntax::PowerShell) => Some(first),
            _ => None,
        };
        let mut word = Word {
            value: String::new(),
            span: Span::new(start, line.len()),
            quote,
            closed: quote.is_none(),
            home: first == '~' && matches!(syntax, ShellSyntax::Posix | ShellSyntax::PowerShell),
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
                    quote = None;
                    word.closed = true;
                }
            } else if quote.is_none() && matches!(ch, ' ' | '\t') {
                word.span.end = offset;
                break;
            } else if quote.is_none()
                && (ch == '"' && syntax != ShellSyntax::Literal
                    || ch == '\'' && matches!(syntax, ShellSyntax::Posix | ShellSyntax::PowerShell))
            {
                quote = Some(ch);
                word.closed = false;
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
                    // PowerShell 的 `n/`t 等是控制字符，不是被转义的字母。
                    if syntax == ShellSyntax::PowerShell && escaped.is_ascii_alphanumeric() {
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
                            !ch.is_alphanumeric()
                                && !matches!(ch, '/' | '.' | '_' | '-' | ':' | '=')
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
            home: false,
        });
    }
    Some(result)
}
