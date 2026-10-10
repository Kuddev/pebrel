//! 远端 Claude Code 的追加系统提示词。
//!
//! Claude Code 的内置文件工具（Read/Edit/Write/Glob/Grep）作用在它所在的 Linux，
//! 而项目在 Windows。本项目不替换、不禁用这些工具，只靠提示词把项目操作引向
//! SSH 回连；所以提示词必须说清"项目不在这里"和"不可以悄悄回退到 Linux"。
//!
//! 每次启动（包括 `--resume`/`--continue`）都由本机重新生成并通过
//! `--append-system-prompt` 传入，不使用任何旧快照。内容只取决于项目与稳定的配置
//! 路径，不含端口、连接 ID 之类每次都变的值：同一项目逐字节一致，模型的提示词
//! 缓存在重连后仍能命中。

/// Windows OpenSSH 用什么解释远端传来的命令行。
///
/// 由注册表 `HKLM\SOFTWARE\OpenSSH\DefaultShell` 决定，它对本机所有 sshd 实例
/// （包括这里的临时实例）生效；未设置时是 `cmd.exe /c`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CommandShell {
    Cmd,
    PowerShell,
}

pub(super) struct PromptInput<'a> {
    /// 本机项目目录（普通 Windows 形式，不带 `\\?\`）。
    pub(super) project: &'a str,
    /// 远端稳定的 SSH 配置路径，主机别名固定为 `windows`。
    pub(super) ssh_config: &'a str,
    /// 远端镜像目录，即 Claude Code 的 cwd。
    pub(super) mirror: &'a str,
    pub(super) shell: CommandShell,
    /// 回连后用于脚本的 PowerShell 可执行文件。
    pub(super) powershell: &'a str,
}

pub(super) fn system_prompt(input: &PromptInput<'_>) -> String {
    let PromptInput { project, ssh_config, mirror, shell, powershell } = *input;
    let ssh = format!("ssh -F {} windows", sh_quote(ssh_config));
    let project_ps = project.replace('\'', "''");
    let invoke = match shell {
        CommandShell::Cmd => format!(
            "Windows OpenSSH runs each remote command line with cmd.exe /c. Invoke PowerShell \
             directly as \"{powershell}\" -NoLogo -NoProfile -NonInteractive -EncodedCommand \
             <base64>; do not prefix it with the PowerShell & operator."
        ),
        CommandShell::PowerShell => format!(
            "Windows OpenSSH runs each remote command line with PowerShell. Invoke a separate \
             PowerShell as & '{}' -NoLogo -NoProfile -NonInteractive -EncodedCommand <base64>.",
            powershell.replace('\'', "''")
        ),
    };
    [
        "The user and this project are on the user's Windows computer, NOT on this Linux host."
            .to_owned(),
        format!("Windows project directory: {project}"),
        format!(
            "This Linux working directory ({mirror}) is only a session anchor. It contains no \
             project files; never create, read or edit project files there."
        ),
        format!(
            "Run every project read, search, edit, build, test and git command on Windows \
             through your Bash tool with standard OpenSSH: {ssh} '<remote command>'."
        ),
        "That SSH configuration pins a loopback tunnel, a temporary identity and the host key. \
         Do not print, copy or read the private key into the conversation."
            .to_owned(),
        invoke,
        "For robust quoting, encode PowerShell scripts as UTF-16LE Base64 on Linux, for example \
         printf '%s' \"$script\" | iconv -f UTF-8 -t UTF-16LE | base64 -w0, and pass the result to \
         -EncodedCommand."
            .to_owned(),
        format!(
            "Begin project scripts with Set-Location -LiteralPath '{project_ps}' and \
             [Console]::OutputEncoding = [Text.UTF8Encoding]::new($false); read and write files \
             as UTF-8 and keep their existing line endings."
        ),
        format!(
            "Your Read, Write, Edit, Glob, Grep and NotebookEdit tools act on this Linux host, so \
             do not use them on project paths. To edit a file you may copy it with scp -F {} \
             windows:'<absolute Windows path>' to a scratch file under /tmp, edit the copy, and \
             copy it back the same way; the Windows file stays the source of truth.",
            sh_quote(ssh_config)
        ),
        "If Windows is unreachable, stop and tell the user. Never silently fall back to this \
         Linux host. After an interruption report results as unknown and do not automatically \
         retry side effects."
            .to_owned(),
        "Each SSH command is its own session and several may run concurrently. Windows processes \
         started by a command end when that SSH session ends. Keep a long-running Windows \
         process such as a dev server or watcher in a background Bash task and stop that task to \
         end it."
            .to_owned(),
        "A remote command that waits indefinitely holds the turn until its Bash timeout. Size the \
         timeout to the work and run anything open-ended as a background task."
            .to_owned(),
    ]
    .join("\n")
}

/// POSIX sh 单引号转义：结果可直接拼进 sh 命令行。
pub(super) fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prompt(shell: CommandShell, powershell: &str) -> String {
        system_prompt(&PromptInput {
            project: r"C:\work\proj",
            ssh_config: "/home/cc/.pebrel-remote/projects/aabbccdd/ssh_config",
            mirror: "/home/cc/pebrel-remote/c/work/proj",
            shell,
            powershell,
        })
    }

    /// R4：同一项目每次启动的提示词必须逐字节一致，才可能命中模型的提示词缓存。
    #[test]
    fn prompt_is_byte_identical_for_the_same_project() {
        assert_eq!(
            prompt(CommandShell::Cmd, r"C:\pwsh.exe"),
            prompt(CommandShell::Cmd, r"C:\pwsh.exe")
        );
    }

    #[test]
    fn prompt_pins_project_paths_and_connection_rule() {
        let text = prompt(CommandShell::Cmd, r"C:\pwsh.exe");
        assert!(text.contains(r"Windows project directory: C:\work\proj"));
        assert!(text.contains("/home/cc/.pebrel-remote/projects/aabbccdd/ssh_config"));
        assert!(text.contains("never create, read or edit project files there"));
        assert!(text.contains("Never silently fall back"));
        assert!(text.contains(r#""C:\pwsh.exe" -NoLogo"#));
    }

    #[test]
    fn powershell_default_shell_quote_form_differs_from_cmd() {
        let text = prompt(CommandShell::PowerShell, r"C:\Program Files\PowerShell\7\pwsh.exe");
        assert!(text.contains(r"& 'C:\Program Files\PowerShell\7\pwsh.exe' -NoLogo"));
    }

    #[test]
    fn sh_quote_escapes_single_quotes() {
        assert_eq!(sh_quote("plain"), "'plain'");
        assert_eq!(sh_quote("it's"), r"'it'\''s'");
    }
}
