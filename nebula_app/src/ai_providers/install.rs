//! CLI 安装复用真实 PTY 生命周期：官方安装器的进度、依赖提示和取消都留在专用标签。
use crate::ai_agents::AgentKind;
use crate::session::LaunchSession;

pub fn launch(agent: AgentKind) -> Option<(LaunchSession, String)> {
    let windows = crate::platform::Platform::current() == crate::platform::Platform::Windows;
    let script = script(agent, windows)?;
    let (program, args, command) = if windows {
        // 不拼接用户输入；安装正文以 PowerShell 自身的字符串规则传给子会话。
        use base64::Engine as _;
        let encoded = base64::engine::general_purpose::STANDARD.encode(script.as_bytes());
        (
            "powershell.exe",
            vec!["-NoLogo".into(), "-NoProfile".into()],
            format!(
                "& ([scriptblock]::Create([Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{encoded}'))))"
            ),
        )
    } else {
        let escaped = script.replace('\\', "\\\\").replace('\'', "\\'").replace('\n', "\\n");
        ("/bin/bash", vec!["-l".into()], format!("bash -c $'{escaped}'"))
    };
    Some((
        LaunchSession::Shell {
            name: format!("Install {}", agent.display_name()),
            program: program.into(),
            args,
        },
        command,
    ))
}

fn script(agent: AgentKind, windows: bool) -> Option<String> {
    let (unix, win) = match agent {
        AgentKind::Codex => {
            ("https://chatgpt.com/codex/install.sh", "https://chatgpt.com/codex/install.ps1")
        },
        AgentKind::Claude => ("https://claude.ai/install.sh", "https://claude.ai/install.ps1"),
        AgentKind::Pi => ("https://pi.dev/install.sh", "https://pi.dev/install.ps1"),
        AgentKind::Hermes => (
            "https://hermes-agent.nousresearch.com/install.sh",
            "https://hermes-agent.nousresearch.com/install.ps1",
        ),
        AgentKind::Kimi => (
            "https://code.kimi.com/kimi-code/install.sh",
            "https://code.kimi.com/kimi-code/install.ps1",
        ),
        AgentKind::OpenCode if !windows => ("https://opencode.ai/v2/install", ""),
        AgentKind::Grok if !windows => {
            ("https://raw.githubusercontent.com/superagent-ai/grok-cli/main/install.sh", "")
        },
        AgentKind::Gemini | AgentKind::OpenCode | AgentKind::Grok => ("", ""),
        _ => return None,
    };
    if windows {
        let body = match agent {
            AgentKind::Gemini | AgentKind::OpenCode => {
                let package =
                    if agent == AgentKind::Gemini { "@google/gemini-cli" } else { "@opencode/cli" };
                format!(
                    "{}\n& npm.cmd install --global --prefix (Join-Path $env:APPDATA 'npm') {package}\nif ($LASTEXITCODE -ne 0) {{ throw 'npm installation failed' }}\nAdd-InstallPath (Join-Path $env:APPDATA 'npm')",
                    include_str!("install/node.ps1")
                )
            },
            AgentKind::Grok => include_str!("install/grok.ps1").into(),
            _ => format!("Invoke-Expression (Invoke-RestMethod -Uri '{win}')"),
        };
        Some(format!(
            "$ErrorActionPreference = 'Stop'\n[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)\ntry {{\n{body}\n}} catch {{ Write-Host $_ -ForegroundColor Red; throw }}"
        ))
    } else if agent == AgentKind::Gemini {
        Some(format!(
            "set -eu\n{}\nnpm install --global --prefix \"$HOME/.local\" @google/gemini-cli\nprintf '\\nGemini installed. Open a new terminal to use gemini.\\n'",
            include_str!("install/node.sh")
        ))
    } else {
        // 先完整下载再执行，网络截断不会留下一个被部分执行的安装脚本。
        Some(format!(
            "set -eu\ninstaller=$(mktemp)\ntrap 'rm -f \"$installer\"' EXIT\ncurl --fail --location --proto '=https' --tlsv1.2 '{unix}' -o \"$installer\"\nbash \"$installer\""
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_plans_cover_visible_agents_and_never_inherit_remote_shells() {
        for agent in [
            AgentKind::Claude,
            AgentKind::Codex,
            AgentKind::Gemini,
            AgentKind::OpenCode,
            AgentKind::Grok,
            AgentKind::Pi,
            AgentKind::Hermes,
            AgentKind::Kimi,
        ] {
            assert!(script(agent, true).is_some());
            assert!(script(agent, false).is_some());
            let (LaunchSession::Shell { program, args, .. }, command) = launch(agent).unwrap()
            else {
                panic!("native shell expected")
            };
            assert_eq!(
                program,
                if crate::platform::Platform::current() == crate::platform::Platform::Windows {
                    "powershell.exe"
                } else {
                    "/bin/bash"
                }
            );
            assert!(
                !args.iter().any(|arg| arg.contains("install")),
                "restoring a tab must not reinstall"
            );
            assert!(!command.is_empty());
        }
    }
}
