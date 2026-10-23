//! 普通命令的显示匹配；命令仍由原 shell 执行，输出仍完整进入 VT 网格。
use nebula_terminal::inline_content::CommandView;
use pebrel_completions::command_context::{CommandContext, ShellSyntax};

pub(crate) fn classify(shell: &str, line: &str) -> Option<CommandView> {
    let shell = shell.rsplit('|').next()?.split(':').next()?;
    let syntax = ShellSyntax::for_program(shell);
    let words = CommandContext::literal_arguments(line, syntax)?;
    let mut words = words.as_slice();
    while words.first().is_some_and(|word| matches!(word.as_str(), "command" | "builtin")) {
        words = &words[1..];
    }
    let program = words.first()?.rsplit(['/', '\\']).next()?.to_ascii_lowercase();
    let program = program.strip_suffix(".exe").unwrap_or(&program);
    let mime = match program {
        "ls" | "ll" | "la" if syntax != ShellSyntax::PowerShell => {
            // 仅改变已完成输出的呈现；递归/自定义格式由原终端显示，避免猜测字段。
            if words[1..].iter().any(|arg| {
                arg == "--recursive"
                    || arg.starts_with('-') && !arg.starts_with("--") && arg.contains('R')
            }) {
                return None;
            }
            "text/x-terminal-fold"
        },
        "ls" | "ll" | "dir" | "gci" | "get-childitem" | "nebula-list"
            if syntax == ShellSyntax::PowerShell =>
        {
            "text/x-terminal-fold"
        },
        "cat" | "get-content" | "gc" | "type"
            if program == "cat" || syntax == ShellSyntax::PowerShell =>
        {
            let mut arguments = words[1..].iter();
            let mut path = None;
            while let Some(argument) = arguments.next() {
                match argument.to_ascii_lowercase().as_str() {
                    "--" => {},
                    "-raw" | "-path" | "-literalpath" if syntax == ShellSyntax::PowerShell => {},
                    "-encoding" if syntax == ShellSyntax::PowerShell => {
                        arguments.next()?;
                    },
                    _ if argument.starts_with('-') || path.is_some() => return None,
                    _ => path = Some(argument),
                }
            }
            match path?.rsplit('.').next()?.to_ascii_lowercase().as_str() {
                "md" | "markdown" | "mdown" => "text/markdown",
                "json" | "jsonl" | "ndjson" => "application/json",
                _ => return None,
            }
        },
        "git" => {
            let mut index = 1;
            while matches!(words.get(index).map(String::as_str), Some("-C" | "-c")) {
                words.get(index + 1)?;
                index += 2;
            }
            if words.get(index).is_some_and(|word| word == "--no-pager") {
                index += 1;
            }
            match words.get(index)?.as_str() {
                "diff" | "show" => "text/x-diff",
                "status" | "log" | "branch" | "tag" | "stash" | "reflog" | "shortlog" => {
                    "text/x-git-output"
                },
                "clone" | "fetch" | "pull" | "push" => "text/x-git-progress",
                _ => return None,
            }
        },
        _ => return None,
    };
    Some(CommandView { mime: mime.into(), title: line.chars().take(120).collect() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_commands_match_without_evaluating_shell_code() {
        for line in ["cat long.md", "Get-Content -Raw '中文 文件.md'", "cat ./a.md -Encoding utf8"]
        {
            assert_eq!(classify("pwsh:1:token", line).unwrap().mime, "text/markdown");
        }
        assert_eq!(classify("bash:1", "cat 'data.json'").unwrap().mime, "application/json");
        assert_eq!(classify("pwsh:1", "git -C 'a b' diff").unwrap().mime, "text/x-diff");
        for command in ["ls", "ls -la", "ls --color=auto '中文 目录'", "ll"] {
            assert_eq!(classify("bash:1", command).unwrap().mime, "text/x-terminal-fold");
        }
        for command in ["ls", "Get-ChildItem", "Nebula-List .", "gci -Force"] {
            assert_eq!(classify("pwsh:1", command).unwrap().mime, "text/x-terminal-fold");
        }
        for line in [
            "cat x.md | more",
            "cat x.md > y",
            "cat x.md; echo hi",
            "cat $p",
            "cat *.md",
            "cat -n x.md",
            "cat x.md y.md",
            "cat 'x.md",
            "echo '# Hi'",
            "ls | more",
            "ls > files.txt",
        ] {
            assert!(classify("pwsh:1", line).is_none(), "{line}");
        }
    }
}
