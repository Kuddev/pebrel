//! 远程 Claude Code over SSH（设计与理由见
//! `architecture/notes/nebula_app/remote_claude/2026-10-08-remote-claude-over-ssh.md`）。
//!
//! 让 Claude Code 跑在远程 Linux 服务器上，而文件读写与命令执行回到本机项目目录。
//!
//! - [`mirror`]：R1 本机路径 → 远端镜像目录的固定映射与项目状态键。
//! - [`prompt`]：R4 每次启动重算的追加系统提示词。
//! - [`script`]：服务器端的预检/下发与会话引导脚本（文本生成）。
//! - [`hosts`]：R7 服务器候选列表，与侧栏共用排序/隐藏规则。
//! - 本机侧（仅 Windows）：[`local`] 环境事实、[`keys`] 一次性密钥、
//!   [`sshd`] 回环 SSH 服务端实例、[`session`] 两阶段编排。
//!
//! 入口是 `pebrel claude --ssh <别名>`（[`run`]）。

mod hosts;
mod mirror;
mod prompt;
mod script;

#[cfg(windows)]
mod keys;
#[cfg(windows)]
mod local;
#[cfg(windows)]
mod session;
#[cfg(windows)]
mod sshd;

#[cfg(windows)]
pub(crate) use session::run;
// 会话包装用的同一份编码实现（见 `gpui_shell::workspace::remote_claude_wrapper`）。
pub(crate) use script::utf16le_base64;
