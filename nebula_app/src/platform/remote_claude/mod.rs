//! 远程 Claude Code 的 Windows 本机实现。
//!
//! 让 Claude Code 跑在远程 Linux 上、而文件与命令回到本机的那一半事实全部
//! 只在本机成立：回环 sshd（[`sshd`]）、一次性密钥（[`keys`]）、PowerShell
//! 与注册表/ACL 事实（[`local`]）、两阶段编排（[`session`]）。远端侧只用
//! 文本：路径映射（[`mirror`]）、追加系统提示词（[`prompt`]）、引导脚本
//! （[`script`]）、主机候选（[`hosts`]）。
//!
//! 跨平台的入口与设计与理由见
//! `architecture/notes/nebula_app/remote_claude/2026-10-08-remote-claude-over-ssh.md`。

mod hosts;
mod keys;
mod local;
mod mirror;
mod prompt;
mod script;
mod session;
mod sshd;

pub(crate) use session::run;
