//! 远程 Claude Code over SSH：跨平台的那一小块。
//!
//! 功能本体（回环 sshd、一次性密钥、两阶段编排、远端脚本）是 Windows 本机
//! 实现，归 `crate::platform::remote_claude`；这里只留两平台都会用到的编码
//! 工具——`pebrel claude` 由 CLI 的能力门控隐藏，但会话包装器（
//! `gpui_shell::workspace::remote_claude_wrapper`）在任何平台都要能编译。
//!
//! 设计与理由见
//! `architecture/notes/nebula_app/remote_claude/2026-10-08-remote-claude-over-ssh.md`。

/// 一段脚本 → PowerShell `-EncodedCommand` 接受的 UTF-16LE Base64。
///
/// 只有这一种形式同时绕开 cmd.exe 与 PowerShell 两套引号规则；两代
/// Windows PowerShell 都认。
pub(crate) fn utf16le_base64(script: &str) -> Result<String, String> {
    use base64::Engine as _;

    let mut bytes = Vec::with_capacity(script.len() * 2);
    for unit in script.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16le_base64_matches_powershell_encoding() {
        // "PS" 的 UTF-16LE 字节是 50 00 53 00。
        assert_eq!(utf16le_base64("PS").unwrap(), "UABTAA==");
    }
}
