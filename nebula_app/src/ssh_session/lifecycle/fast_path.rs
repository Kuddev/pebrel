//! 分支快路径：集成 shell 的免 python bootstrap 与按连接池键的 root 缓存。
//!
//! 上游的 `integration::prepare`（python snapshot/apply + `remote_shell.py`
//! exec）保持字节不动，作为本路径失败后的委托安全网（`open_shell_channel`）；
//! 本文件持有快路径全部分支差异——上游合并干净后再退役 python dispatch
//! （见 architecture/notes/nebula_app/ssh_session/2026-10-22-fastpath-shell-dispatch.md）。
//!
//! 动机（用户 10-22 两次现场，同一根因）：远端 provider 配置被用户改过
//! （`~/.claude/settings.json` 的 hooks 被同步成异机路径）时，`plan` 按
//! 「edited … preserved」正确拒绝改写——但这会拖垮**整条 shell 通道**：上游
//! `prepare` 拿不到 bootstrap，pane 静默回退普通登录 shell，OSC 7/
//! `NEBULA|`/`pebrel_git_root` 全哑，侧栏 SSH 分组冻结在初始 cwd。本路径把
//! 「shell 通道」与「provider hook 记账」解耦：cwd/分组/分支只依赖 rcfile
//! 资产在场，plan/apply 的保护性失败只降级 hook 安装，不再降级 shell。
//!
//! 快路径约束：缓存条目只在**安装确认**（snapshot 可解析且 rcfile 在场）
//! 后写入——speculative exec 永不
//! 抢占未安装过集成的主机（rcfile 缺失时分派脚本会退成裸登录 shell，比
//! 走上游安装路径更差）。Windows 远端因此天然隔离：`Snapshot::plan` 的
//! POSIX 绝对路径门禁会先拒绝安装，缓存永远没有条目。

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use nebula_terminal::event::WindowSize;
use russh_sftp::client::SftpSession;
use russh_sftp::client::error::Error as SftpClientError;
use russh_sftp::protocol::{FileAttributes, OpenFlags, StatusCode};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

use super::super::{AcquiredSession, SessionError, SharedSession};
use crate::ai_hook::remote::quote;

/// 免 python 的集成 shell 分派命令：`exec sh -c '<POSIX 脚本>'`。脚本按
/// `$SHELL` 分派——bash 以 `--rcfile <root>/bashrc -i` 启动（回放登录启动，
/// `PEBREL_REMOTE_LOGIN=1`），zsh 折 `ZDOTDIR` 到集成根后 `zsh -il`，其余
/// 退普通登录 shell；环境契约（token 双名、pane 标记、hook 可执行路径）
/// 与 `res/hooks/remote_shell.py` 逐项一致（测试互为镜像）。rcfile 缺失
/// （未安装/用户 `--remove`/缓存 root 陈旧）同样退登录 shell——通道不死，
/// 后台校验负责补装。脚本走 argv，不经 stdin/PTY，无回显污染；root 与
/// token 都用单引号包裹（loopback 测试按 `'` 切 32 位 hex 提取 token）。
pub(super) fn shell_dispatch(root: &str, token: &str) -> String {
    let hook = format!("{root}/pebrel-hook");
    let script = format!(
        concat!(
            "root={root}\n",
            "export PEBREL_REMOTE_HOOK_TOKEN={token} NEBULA_REMOTE_HOOK_TOKEN={token}\n",
            "export PEBREL_PANE_REMOTE=1 NEBULA_PANE_REMOTE=1\n",
            "hook={hook}\n",
            "export PEBREL_HOOK_EXE=\"$hook\" NEBULA_HOOK_EXE=\"$hook\"\n",
            "shell=${{SHELL:-/bin/sh}}\n",
            "case \"${{shell##*/}}\" in\n",
            "bash)\n",
            "  [ -r \"$root/bashrc\" ] && {{\n",
            "    export PEBREL_REMOTE_LOGIN=1\n",
            "    exec \"$shell\" --rcfile \"$root/bashrc\" -i\n",
            "  }}\n",
            "  ;;\n",
            "zsh)\n",
            "  [ -r \"$root/.zshrc\" ] && {{\n",
            "    if [ -n \"${{ZDOTDIR+set}}\" ]; then was=1; else was=0; fi\n",
            "    export NEBULA_ZDOTDIR_WAS_SET=\"$was\" NEBULA_ORIGINAL_ZDOTDIR=\"${{ZDOTDIR-}}\"\n",
            "    export NEBULA_ZSH_INTEGRATION=\"$root\" ZDOTDIR=\"$root\"\n",
            "    exec \"$shell\" -il\n",
            "  }}\n",
            "  ;;\n",
            "esac\n",
            "exec \"$shell\" -l\n",
        ),
        root = quote(root),
        token = quote(token),
        hook = quote(&hook),
    );
    format!("exec sh -c {}", quote(&script))
}

/// 连接池键 → 已确认安装的集成根。进程内缓存：条目只在安装成功
/// （snapshot+apply 通过）后写入，重启后首连重付一次安装探测。
fn roots() -> &'static Mutex<HashMap<String, String>> {
    static ROOTS: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    ROOTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cached_root(key: &str) -> Option<String> {
    roots().lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(key).cloned()
}

fn remember_root(key: &str, root: &str) {
    roots()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(key.to_owned(), root.to_owned());
}

#[cfg(test)]
pub(super) fn cache_clear_for_tests() {
    roots().lock().unwrap_or_else(std::sync::PoisonError::into_inner).clear();
}

/// loopback 流程测试用：预填缓存，模拟「本进程早前连接已确认安装」。
#[cfg(test)]
pub(super) fn cache_seed_for_tests(key: &str, root: &str) {
    remember_root(key, root);
}

/// 远端 python 适配器入口（与 `integration::PYTHON` 同一约定；该常量是
/// integration 模块私有，这里独立持有一份，两处必须同步改）。
const PYTHON: &str = "python3 -";
/// 关键路径安装预算：与上游 `integration::BUDGET` 同刻度；snapshot 在实测
/// 主机上 ~100ms（codex 是原生二进制），预算余量充足。
const INSTALL_BUDGET: Duration = Duration::from_secs(6);
/// 后台校验预算：不在关键路径上，宽给慢主机；provider 记账失败后的补装
/// 重试也由它承担（每条新 TCP 连接一次）。
const VERIFY_BUDGET: Duration = Duration::from_secs(30);

/// 快路径开 shell 入口（`open_shell_with_budget` 的唯一分派点）。
///
/// ① 缓存命中 → 直接 exec 免 python 分派；② SFTP 直装（canonicalize 发现
/// root，bashrc 缺席/过期才写资产；provider 记账失败不拖垮 shell）→ 分派；
/// ③ SFTP 不可用 → python snapshot+apply 兜底 → 分派；④ 仍失败 → 委托上游
/// `open_shell_channel`（字节不动的 prepare + python bootstrap +
/// request_shell 回退）。后台校验只在**新** TCP 连接或委托发生时跑，
/// 复用连接的后续 pane 零探测流量。
pub(super) async fn open_shell_channel_fast(
    acquired: &AcquiredSession,
    size: WindowSize,
    remote_cwd: Option<&str>,
    target: Option<&super::super::ResumeTarget>,
) -> Result<(super::ShellChannel, String), SessionError> {
    let hook_token = super::super::remote_hook_token()?;
    if let Some(root) = cached_root(&acquired.key) {
        match dispatch_shell(&acquired.session, size, &root, &hook_token, remote_cwd).await {
            Ok(result) => {
                maybe_verify(acquired, false);
                return Ok(result);
            },
            Err(error) => log::warn!(
                target: "nebula",
                "SSH fast-path dispatch rejected for {} (root {root}): {error}; reinstalling",
                acquired.key
            ),
        }
    }
    // ② SFTP 直装：python 退出关键路径（无 python3 主机也能集成）。
    if let Some(root) = install_via_sftp(acquired, INSTALL_BUDGET).await {
        remember_root(&acquired.key, &root);
        match dispatch_shell(&acquired.session, size, &root, &hook_token, remote_cwd).await {
            Ok(result) => {
                maybe_verify(acquired, false);
                return Ok(result);
            },
            Err(error) => log::warn!(
                target: "nebula",
                "SSH dispatch rejected after SFTP install (root {root}): {error}; trying python",
            ),
        }
    }
    // ③ python 安装兜底：SFTP 不可用/失败的主机仍能经 python 装（provider
    // hooks 与资产升级也由它和后台 verify 负责）。
    if let Some(root) = install(&acquired.session, INSTALL_BUDGET).await {
        remember_root(&acquired.key, &root);
        match dispatch_shell(&acquired.session, size, &root, &hook_token, remote_cwd).await {
            Ok(result) => {
                maybe_verify(acquired, false);
                return Ok(result);
            },
            Err(error) => log::warn!(
                target: "nebula",
                "SSH dispatch rejected after install (root {root}): {error}; delegating",
            ),
        }
    }
    log::warn!(
        target: "nebula",
        "SSH fast-path shell dispatch unavailable for {}; delegating to the upstream prepare path",
        acquired.key
    );
    let result = super::open_shell_channel(&acquired.session, size, remote_cwd, target).await;
    maybe_verify(acquired, true);
    result
}

/// 安装探测（尽力而为）：snapshot → plan → 有差异则 apply。
///
/// plan/apply 的「edited … preserved」类失败是保护用户改动的**正确行为**，
/// 不让它拖垮 shell 通道：只要 snapshot 显示 rcfile 资产已在场（此前装过），
/// 照样返回 root 供 dispatch；provider hook 记账留给后台校验持续重试。
/// snapshot 本身失败、响应不可解析或主机显式禁用（`plan` Ok(None)）才返回
/// `None`（交回委托路径）。与 `integration::prepare` 同一形状（那里只回
/// 命令串拿不出 root，故在此组装同一批 `pub(crate)` 件）。
async fn install(session: &SharedSession, budget: Duration) -> Option<String> {
    let channel = match session.channel_open_session().await {
        Ok(channel) => channel,
        Err(error) => {
            log::warn!(target: "nebula", "SSH integration snapshot channel failed: {error}");
            return None;
        },
    };
    let raw = match super::super::exec::capture(
        channel,
        PYTHON,
        &crate::ai_hook::remote::request(&serde_json::json!({"action":"snapshot"})),
        budget,
        "fast-path snapshot",
    )
    .await
    {
        Ok(raw) => raw,
        Err(error) => {
            log::warn!(target: "nebula", "SSH integration snapshot failed: {error}");
            return None;
        },
    };
    let value = match crate::ai_hook::remote::response(&raw) {
        Ok(value) => value,
        Err(error) => {
            log::warn!(target: "nebula", "SSH integration snapshot response unusable: {error}");
            return None;
        },
    };
    let snapshot: crate::ai_hook::remote::Snapshot = match serde_json::from_value(value.clone()) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            log::warn!(target: "nebula", "SSH integration snapshot undecodable: {error}");
            return None;
        },
    };
    let root = snapshot.root.clone();
    match snapshot.plan(crate::ai_hook::remote::Action::Automatic) {
        // 用户 `--remove` 过：尊重 opt-out，不给 dispatch 机会。
        Ok(None) => None,
        Ok(Some(files)) if files.is_empty() => Some(root),
        Ok(Some(files)) => match apply(session, &files, budget).await {
            Ok(()) => Some(root),
            Err(error) => {
                log::warn!(target: "nebula", "SSH integration apply failed (root {root}): {error}");
                rcfile_installed(&value).then_some(root)
            },
        },
        Err(error) => {
            log::warn!(
                target: "nebula",
                "SSH integration plan refused (root {root}): {error}; \
                 dispatching with the installed rcfile only"
            );
            rcfile_installed(&value).then_some(root)
        },
    }
}

/// snapshot 原始响应里是否已有任一 rcfile 资产（bash/zsh 二选一即可）。
/// 用原始 `Value` 而非 `Snapshot`：其 `files` 字段私有，这里只需存在性。
fn rcfile_installed(value: &serde_json::Value) -> bool {
    ["bashrc", ".zshrc"].iter().any(|name| value["files"][name]["content"].is_string())
}

// ── SFTP 直装（第二阶段）：python 退出 shell 集成的安装与运行 ──────────────

/// SFTP 直装的 shell 资产清单（名称, 内容, 权限位）。内容与 python 适配器
/// （`remote.rs install()`）构造的逐字节一致——镜像单测锁定两处不漂移；
/// 唯一允许的差异是 `pebrel-hook` 的解释器路径：SFTP 版用 PATH 的
/// `python3`（无 python 主机 hook 本就不可用，shell 集成不受影响），python
/// 主机由后台 verify 以 `sys.executable` 绝对路径版覆写归一。
fn shell_assets(root: &str) -> Vec<(&'static str, String, u32)> {
    let connection = nebula_terminal::tty::connection_shell();
    vec![
        (".zshenv", include_str!("../../../res/shell/zshenv").to_owned(), 0o644),
        (".zprofile", include_str!("../../../res/shell/zprofile").to_owned(), 0o644),
        (".zshrc", format!("{}\n{}", include_str!("../../../res/shell/zshrc"), connection), 0o644),
        ("bashrc", format!("{}\n{}", include_str!("../../../res/shell/bashrc"), connection), 0o644),
        ("bridge.py", include_str!("../../../res/hooks/remote_bridge.py").to_owned(), 0o644),
        (
            "pebrel-hook",
            format!("#!/bin/sh\nexec python3 {} \"$@\"\n", quote(&format!("{root}/bridge.py"))),
            0o755,
        ),
    ]
}

/// 集成根派生自 SFTP realpath（`.` = 登录 home）；尾斜杠归一。XDG_DATA_HOME
/// 覆盖的主机由后台 python verify 的 root 修正缓存（SFTP 读不到登录环境）。
fn sftp_root(home: &str) -> String {
    let home = home.trim_end_matches('/');
    format!("{home}/.local/share/pebrel/ai")
}

/// manifest 合并：只动 assets 段，groups/notify/enabled_feature 原样保留
/// （`remove()` 仍能经 manifest.assets 删到这些文件，python plan 不会把
/// SFTP 写入的内容误判「edited preserved」）。缺席/损坏的旧文件回落为最小
/// 合法结构——`version` 必须在场，python 端 `Manifest.version` 反序列化
/// 无默认值。与后台 python apply 的 read-modify-write 竞争窗口极小、内容
/// 幂等，下一连接 verify 自愈。
fn merge_manifest_assets(existing: &str, updates: &[(&'static str, String)]) -> String {
    let mut value: serde_json::Value = if existing.trim().is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_str(existing).unwrap_or_else(|_| serde_json::json!({}))
    };
    let version = value.get("version").cloned().unwrap_or_else(|| serde_json::json!(1));
    if let Some(object) = value.as_object_mut() {
        let assets = object
            .entry("assets")
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()
            .expect("assets manifest section is an object");
        for (name, digest) in updates {
            assets.insert((*name).to_owned(), serde_json::json!(digest));
        }
        object.insert("version".to_owned(), version);
    }
    let mut text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_owned());
    text.push('\n');
    text
}

/// 在**已认证的池化会话**上开 SFTP 子系统（镜像 `ssh_session::open_sftp` 的
/// 通道参数，但不按目的地重解析——跳板/代理 profile 下重解析可能路由到
/// 另一条连接）。单包/在途上限的理由见 `ssh_sftp::limits`。
async fn open_sftp_on(session: &SharedSession) -> Option<SftpSession> {
    let mut channel = session.channel_open_session().await.ok()?;
    channel.request_subsystem(true, "sftp").await.ok()?;
    // 先等 subsystem 应答再握手：被拒绝（无 sftp 的服务器）即刻失败，不会
    // 把 SFTP INIT 字节吐进通道（那会污染 pane 数据流），也不必等外层预算。
    match channel.wait().await {
        Some(russh::ChannelMsg::Success) => SftpSession::new_with_config(
            channel.into_stream(),
            crate::ssh_sftp::limits::session_config(),
        )
        .await
        .ok(),
        _ => None,
    }
}

/// 读远端文件；缺席（NoSuchFile）与任何失败都按「无内容」处理——调用方
/// （digest 校验/manifest 合并）把读不到视作需要重写，自愈安全。
async fn read_optional(sftp: &SftpSession, path: &str) -> Option<Vec<u8>> {
    let mut file = match sftp.open(path.to_owned()).await {
        Ok(file) => file,
        Err(error) => {
            if !matches!(&error, SftpClientError::Status(status)
                if status.status_code == StatusCode::NoSuchFile)
            {
                log::warn!(target: "nebula", "SFTP read failed for {path}: {error}");
            }
            return None;
        },
    };
    let mut bytes = Vec::new();
    match file.read_to_end(&mut bytes).await {
        Ok(_) => Some(bytes),
        Err(error) => {
            log::warn!(target: "nebula", "SFTP read incomplete for {path}: {error}");
            None
        },
    }
}

/// temp 写入 → 设权限 → rename 落位（与上传管线同一覆盖式 rename 语义）。
/// 崩在 rename 前只留一个 `.nebula-new` 残件，下次连接的 digest 校验自愈。
async fn write_asset_atomic(
    sftp: &SftpSession,
    target: &str,
    content: &[u8],
    mode: u32,
) -> Result<(), String> {
    let temp = format!("{target}.nebula-new");
    let write = async {
        let mut file = sftp
            .open_with_flags(
                temp.clone(),
                OpenFlags::CREATE | OpenFlags::WRITE | OpenFlags::TRUNCATE,
            )
            .await
            .map_err(|error| error.to_string())?;
        file.write_all(content).await.map_err(|error| error.to_string())?;
        let mut attributes = FileAttributes::empty();
        attributes.permissions = Some(mode);
        sftp.set_metadata(temp.clone(), attributes).await.map_err(|error| error.to_string())?;
        sftp.rename(temp.clone(), target.to_owned()).await.map_err(|error| error.to_string())?;
        Ok::<_, String>(())
    };
    match write.await {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = sftp.remove_file(temp).await;
            Err(error)
        },
    }
}

/// SFTP 直装（第二阶段）：python 退出 shell 集成的关键路径。
///
/// ① `canonicalize(".")` 发现 home → root；② bashrc 在场且内容与期望一致 →
/// 直接返回（零写入）；③ 缺席/过期 → **POSIX 门槛**（`exec sh -c true` 一轮
/// 往返；Windows 等非 POSIX 远端到此为止，绝不留垃圾文件）→ 写全部资产 +
/// manifest 合并落盘。任何失败返回 `None`，交回 python 安装兜底。
async fn install_via_sftp(acquired: &AcquiredSession, budget: Duration) -> Option<String> {
    let install = async {
        let sftp = open_sftp_on(&acquired.session).await.ok_or("SFTP subsystem unavailable")?;
        let home = sftp
            .canonicalize(".".to_owned())
            .await
            .map_err(|error| format!("home discovery failed: {error}"))?;
        let root = sftp_root(&home);
        let assets = shell_assets(&root);
        let desired_bashrc = assets
            .iter()
            .find(|(name, _, _)| *name == "bashrc")
            .map(|(_, content, _)| content.as_bytes())
            .unwrap_or_default();
        if read_optional(&sftp, &format!("{root}/bashrc")).await.as_deref() == Some(desired_bashrc)
        {
            return Ok(root);
        }
        // POSIX 门槛：向非 POSIX 主机写文件只会留下永远用不上的残件。
        let gate = super::super::exec::capture(
            acquired.session.channel_open_session().await.map_err(|error| error.to_string())?,
            "exec sh -c true",
            b"",
            budget,
            "fast-path POSIX gate",
        )
        .await;
        if let Err(error) = gate {
            return Err(format!("remote is not a POSIX shell host: {error}"));
        }
        if let Err(create) = sftp.create_dir(root.clone()).await {
            let existing = sftp.metadata(root.clone()).await.ok();
            if !existing.is_some_and(|metadata| metadata.is_dir()) {
                return Err(format!("create integration root failed: {create}"));
            }
        }
        for (name, content, mode) in &assets {
            write_asset_atomic(&sftp, &format!("{root}/{name}"), content.as_bytes(), *mode).await?;
        }
        let manifest_path = format!("{root}/manifest.json");
        let existing = read_optional(&sftp, &manifest_path).await;
        let updates: Vec<(&'static str, String)> = assets
            .iter()
            .map(|(name, content, _)| (*name, crate::ai_hook::remote::digest(content)))
            .collect();
        let merged = merge_manifest_assets(
            existing.as_deref().and_then(|bytes| std::str::from_utf8(bytes).ok()).unwrap_or(""),
            &updates,
        );
        write_asset_atomic(&sftp, &manifest_path, merged.as_bytes(), 0o644).await?;
        Ok(root)
    };
    // SFTP 子系统协商没有内建超时（被拒绝的 subsystem 会让 SFTP 握手永远等
    // 不到 INIT），整个安装被预算兜底——loopback 的拒绝路径正是这条线。
    match tokio::time::timeout(budget, install).await {
        Ok(Ok(root)) => Some(root),
        Ok(Err(error)) => {
            log::warn!(target: "nebula", "SFTP shell-asset install unavailable: {error}");
            None
        },
        Err(_) => {
            log::warn!(target: "nebula", "SFTP shell-asset install timed out");
            None
        },
    }
}

async fn apply(
    session: &SharedSession,
    files: &[crate::ai_hook::remote::Edit],
    budget: Duration,
) -> Result<(), SessionError> {
    let raw = super::super::exec::capture(
        session.channel_open_session().await?,
        PYTHON,
        &crate::ai_hook::remote::request(&serde_json::json!({"action":"apply", "files": files})),
        budget,
        "fast-path apply",
    )
    .await?;
    if crate::ai_hook::remote::response(&raw)?.get("applied").and_then(serde_json::Value::as_bool)
        != Some(true)
    {
        return Err("remote integration did not confirm installation".into());
    }
    Ok(())
}

/// 新通道 + PTY + 免 python 分派请求。三步各等一次 Success；任何一步被拒
/// 即整体失败（调用方决定重装还是委托）。
async fn dispatch_shell(
    session: &SharedSession,
    size: WindowSize,
    root: &str,
    hook_token: &str,
    remote_cwd: Option<&str>,
) -> Result<(super::ShellChannel, String), SessionError> {
    let mut channel = super::own_channel(session.channel_open_session().await?);
    channel
        .request_pty(
            true,
            "xterm-256color",
            u32::from(size.num_cols),
            u32::from(size.num_lines),
            u32::from(size.cell_width) * u32::from(size.num_cols),
            u32::from(size.cell_height) * u32::from(size.num_lines),
            &[],
        )
        .await?;
    super::wait_request_success(&mut channel, "fast-path shell PTY").await?;
    channel.exec(true, shell_dispatch(root, hook_token)).await?;
    super::wait_request_success(&mut channel, "fast-path shell").await?;
    if let Some(command) = super::super::initial_remote_cd_command(remote_cwd) {
        channel.data_bytes(command).await?;
    }
    Ok((channel, hook_token.to_owned()))
}

/// 后台校验：宽预算重跑一次安装（补装缺失文件/升级资产/刷新缓存，
/// 均在下次连接生效），失败只记 debug——下一次新连接自然会再试。
fn maybe_verify(acquired: &AcquiredSession, delegated: bool) {
    if acquired.reused && !delegated {
        return;
    }
    let session = acquired.session.clone();
    let key = acquired.key.clone();
    tokio::spawn(async move {
        // 失败路径的 warn 已在 install 内部落盘；这里只负责缓存与资产升级。
        if let Some(root) = install(&session, VERIFY_BUDGET).await {
            remember_root(&key, &root);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    /// 缓存条目只在安装确认成功后写入（write-after-install），读按连接池键。
    #[test]
    fn root_cache_round_trips_per_pool_key() {
        cache_clear_for_tests();
        assert_eq!(cached_root("a@host:22"), None);
        remember_root("a@host:22", "/root/.local/share/pebrel/ai");
        assert_eq!(cached_root("a@host:22").as_deref(), Some("/root/.local/share/pebrel/ai"));
        assert_eq!(cached_root("b@host:22"), None, "entries are per pool key");
        remember_root("a@host:22", "/new/root");
        assert_eq!(cached_root("a@host:22").as_deref(), Some("/new/root"), "latest root wins");
        cache_clear_for_tests();
        assert_eq!(cached_root("a@host:22"), None);
    }

    /// SFTP 直装的资产必须与 python 适配器（`remote.rs install()`）构造的
    /// 逐字节一致：同一份仓库源文件 + 同一段连接遥测脚本 + 同一 launcher
    /// 形态（python 路径除外——SFTP 版用 PATH 的 `python3`，python 主机由
    /// 后台 verify 以绝对路径版覆写归一）。防两处安装器漂移。
    #[test]
    fn sftp_assets_mirror_the_python_installer_byte_for_byte() {
        let root = "/root/.local/share/pebrel/ai";
        let assets = shell_assets(root);
        let names: Vec<_> = assets.iter().map(|(name, _, _)| *name).collect();
        assert_eq!(
            names,
            vec![".zshenv", ".zprofile", ".zshrc", "bashrc", "bridge.py", "pebrel-hook"]
        );

        let repo = |path: &str| {
            std::fs::read_to_string(format!("{}/{}", env!("CARGO_MANIFEST_DIR"), path)).unwrap()
        };
        let connection = nebula_terminal::tty::connection_shell();
        let expected: Vec<(&str, String)> = vec![
            (".zshenv", repo("res/shell/zshenv")),
            (".zprofile", repo("res/shell/zprofile")),
            (".zshrc", format!("{}\n{}", repo("res/shell/zshrc"), connection)),
            ("bashrc", format!("{}\n{}", repo("res/shell/bashrc"), connection)),
            ("bridge.py", repo("res/hooks/remote_bridge.py")),
            (
                "pebrel-hook",
                format!("#!/bin/sh\nexec python3 {} \"$@\"\n", quote(&format!("{root}/bridge.py"))),
            ),
        ];
        for ((name, content, _), (expected_name, expected_content)) in assets.iter().zip(&expected)
        {
            assert_eq!(name, expected_name);
            assert_eq!(content, expected_content, "{name} diverged from the python installer");
        }
        // pebrel-hook 必须可执行，其余是数据文件。
        assert_eq!(assets.iter().find(|(name, _, _)| *name == "pebrel-hook").unwrap().2, 0o755);
        assert!(assets.iter().all(|(_, _, mode)| *mode == 0o644 || *mode == 0o755));
    }

    /// 集成根派生自 SFTP realpath 的 home；尾斜杠归一。
    #[test]
    fn sftp_root_appends_the_integration_path() {
        assert_eq!(sftp_root("/root"), "/root/.local/share/pebrel/ai");
        assert_eq!(sftp_root("/root/"), "/root/.local/share/pebrel/ai");
        assert_eq!(sftp_root("/home/u/x"), "/home/u/x/.local/share/pebrel/ai");
    }

    /// manifest 合并只动 assets 段：groups/notify/无关 assets 原样保留，
    /// 本批资产 digest 覆盖/插入；缺席或损坏的旧文件回落为最小合法结构
    /// （version 必须在场——python 端 Manifest 反序列化无默认值）。
    #[test]
    fn manifest_merge_preserves_foreign_entries_and_updates_shell_digests() {
        let existing = r#"{
          "version": 1,
          "groups": {"claude": {"Stop": []}},
          "assets": {"bashrc": "stale", "unrelated": "keep"},
          "notify": {"original": null, "installed": ["/hook", "codex"]},
          "enabled_feature": true
        }"#;
        let merged = merge_manifest_assets(
            existing,
            &[("bashrc", "new".to_owned()), ("pebrel-hook", "fresh".to_owned())],
        );
        let value: serde_json::Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(value["version"], 1);
        assert_eq!(value["groups"]["claude"]["Stop"].as_array().unwrap().len(), 0);
        assert_eq!(value["assets"]["bashrc"], "new");
        assert_eq!(value["assets"]["pebrel-hook"], "fresh");
        assert_eq!(value["assets"]["unrelated"], "keep");
        assert_eq!(value["notify"]["installed"][0], "/hook");
        assert_eq!(value["enabled_feature"], true);

        let fresh = merge_manifest_assets("", &[("bashrc", "only".to_owned())]);
        let value: serde_json::Value = serde_json::from_str(&fresh).unwrap();
        assert_eq!(value["version"], 1, "version must exist for the python Manifest");
        assert_eq!(value["assets"]["bashrc"], "only");
    }

    /// 平台依赖的端到端分派测试（stub shell、/bin/sh、Unix 权限位）：每个
    /// 测试体首行按宿主运行时跳过（不占 platform cfg 预算）。
    mod platform_dispatch_tests {
        use super::super::*;
        use super::*; // tests mod 的 TOKEN 与（unix）辅助 // fast_path 顶层的 shell_dispatch 等

        fn run_command(command: &str, env: &[(&str, &str)]) -> String {
            use std::process::Command;

            let mut child = Command::new("/bin/sh")
                .arg("-c")
                .arg(command)
                .env_remove("ZDOTDIR")
                .envs(env.iter().map(|(k, v)| (k.to_owned(), v.to_owned())))
                .stdout(std::process::Stdio::piped())
                .spawn()
                .expect("spawn /bin/sh");
            // sh -c 不会读 stdin；关掉避免子进程继承测试终端。
            let _ = child.stdin.take();
            let output = child.wait_with_output().expect("wait /bin/sh");
            assert!(output.status.success(), "dispatch script failed: {command}");
            String::from_utf8_lossy(&output.stdout).into_owned()
        }
        fn stub_shell(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
            use std::os::unix::fs::PermissionsExt as _;

            let path = dir.join(name);
            std::fs::write(
                &path,
                "#!/bin/sh\nprintf 'STUB_ARGS:%s\\n' \"$*\"\nenv | LC_ALL=C sort\n",
            )
            .unwrap();
            let mut permissions = std::fs::metadata(&path).unwrap().permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&path, permissions).unwrap();
            path
        }
        fn temp_root() -> (tempfile::TempDir, String) {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().join("root");
            std::fs::create_dir_all(&root).unwrap();
            let root = root.to_string_lossy().into_owned();
            (dir, root)
        }
        /// 观察分派目标的替身 shell：打印参数与继承的环境，然后退出。放在
        /// 独立临时目录里按需命名为 bash/zsh/fish，覆盖三条分派。
        /// bash 分派：装上 rcfile 后以 `--rcfile <root>/bashrc -i` 启动，且
        /// 远端 shell 集成的全部环境契约都在场（token 双名、pane 标记、hook
        /// 可执行路径、远端登录回放标记）。
        #[test]
        fn dispatch_bash_branch_carries_the_full_environment_contract() {
            if !std::env::consts::FAMILY.eq("unix") {
                return; // Windows 无 /bin/sh 与 Unix 权限位，整组跳过
            }
            let (_dir, root) = temp_root();
            std::fs::write(std::path::Path::new(&root).join("bashrc"), "").unwrap();
            let stub = stub_shell(_dir.path(), "bash");
            let out =
                run_command(&shell_dispatch(&root, TOKEN), &[("SHELL", stub.to_str().unwrap())]);
            assert!(out.contains(&format!("STUB_ARGS:--rcfile {root}/bashrc -i\n")), "{out}");
            for line in [
                format!("PEBREL_REMOTE_HOOK_TOKEN={TOKEN}"),
                format!("NEBULA_REMOTE_HOOK_TOKEN={TOKEN}"),
                "PEBREL_PANE_REMOTE=1".to_owned(),
                "NEBULA_PANE_REMOTE=1".to_owned(),
                format!("PEBREL_HOOK_EXE={root}/pebrel-hook"),
                format!("NEBULA_HOOK_EXE={root}/pebrel-hook"),
                "PEBREL_REMOTE_LOGIN=1".to_owned(),
            ] {
                assert!(out.contains(&line), "missing {line} in:\n{out}");
            }
        }
        /// zsh 分派：ZDOTDIR 折向集成根并记录原值；set-but-empty 的原 ZDOTDIR
        /// 与 shell.py 同样读作「曾设置」（WAS_SET=1）。
        #[test]
        fn dispatch_zsh_branch_folds_zdotdir_like_remote_shell_py() {
            if !std::env::consts::FAMILY.eq("unix") {
                return; // Windows 无 /bin/sh 与 Unix 权限位，整组跳过
            }
            let (_dir, root) = temp_root();
            std::fs::write(std::path::Path::new(&root).join(".zshrc"), "").unwrap();
            let stub = stub_shell(_dir.path(), "zsh");
            let out =
                run_command(&shell_dispatch(&root, TOKEN), &[("SHELL", stub.to_str().unwrap())]);
            assert!(out.contains("STUB_ARGS:-il\n"), "{out}");
            for line in [
                format!("ZDOTDIR={root}"),
                format!("NEBULA_ZSH_INTEGRATION={root}"),
                "NEBULA_ZDOTDIR_WAS_SET=0".to_owned(),
                "NEBULA_ORIGINAL_ZDOTDIR=".to_owned(),
            ] {
                assert!(out.contains(&line), "missing {line} in:\n{out}");
            }
            let out = std::process::Command::new("/bin/sh")
                .arg("-c")
                .arg(shell_dispatch(&root, TOKEN))
                .env("SHELL", &stub)
                .env("ZDOTDIR", "/orig")
                .output()
                .unwrap();
            let out = String::from_utf8_lossy(&out.stdout);
            assert!(out.contains("NEBULA_ZDOTDIR_WAS_SET=1"), "{out}");
            assert!(out.contains("NEBULA_ORIGINAL_ZDOTDIR=/orig"), "{out}");
        }
        /// rcfile 缺失（未安装/用户 `--remove` 过/缓存里的 root 已陈旧）与既非
        /// bash 也非 zsh 的默认 shell 都退成普通登录 shell——与上游回退语义一致，
        /// 绝不让通道死掉。
        #[test]
        fn dispatch_falls_back_to_a_plain_login_shell_without_rcfiles() {
            if !std::env::consts::FAMILY.eq("unix") {
                return; // Windows 无 /bin/sh 与 Unix 权限位，整组跳过
            }
            let (dir, root) = temp_root();
            let bash = stub_shell(dir.path(), "bash");
            let out =
                run_command(&shell_dispatch(&root, TOKEN), &[("SHELL", bash.to_str().unwrap())]);
            assert!(out.contains("STUB_ARGS:-l\n"), "missing rcfile must degrade: {out}");

            let fish = stub_shell(dir.path(), "fish");
            std::fs::write(std::path::Path::new(&root).join("bashrc"), "").unwrap();
            let out =
                run_command(&shell_dispatch(&root, TOKEN), &[("SHELL", fish.to_str().unwrap())]);
            assert!(out.contains("STUB_ARGS:-l\n"), "other shells stay login-only: {out}");
        }
        /// 敌意 root 路径（空格、单引号、`$`）必须逐字符活着到达远端——双层
        /// 引号（外层 `sh -c` 包裹 + 内层赋值）都走 POSIX 单引号折叠；token
        /// 用单引号包裹（loopback 测试按 `'` 切 32 位 hex 提取 token）。
        #[test]
        fn dispatch_quotes_hostile_paths_and_the_token() {
            if !std::env::consts::FAMILY.eq("unix") {
                return; // Windows 无 /bin/sh 与 Unix 权限位，整组跳过
            }
            let dir = tempfile::tempdir().unwrap();
            let hostile = dir.path().join("ro me'$\"x");
            std::fs::create_dir_all(&hostile).unwrap();
            std::fs::write(hostile.join("bashrc"), "").unwrap();
            let root = hostile.to_string_lossy().into_owned();
            let stub = stub_shell(dir.path(), "bash");
            let out =
                run_command(&shell_dispatch(&root, TOKEN), &[("SHELL", stub.to_str().unwrap())]);
            assert!(out.contains(&format!("STUB_ARGS:--rcfile {root}/bashrc -i\n")), "{out}");
            assert!(out.contains(&format!("PEBREL_REMOTE_HOOK_TOKEN={TOKEN}")), "{out}");
            let command = shell_dispatch(&root, TOKEN);
            assert!(command.starts_with("exec sh -c '"), "{command}");
        }
        /// 与 `res/hooks/remote_shell.py` 的环境契约逐项一致：同一替身 shell 下
        /// 两套 dispatch 导出的契约变量必须相等（root 相关路径除外——shell.py 的
        /// root 取自脚本自身位置）。python3 缺席的环境跳过。
        #[test]
        fn dispatch_env_contract_mirrors_remote_shell_py() {
            if !std::env::consts::FAMILY.eq("unix") {
                return; // Windows 无 /bin/sh 与 Unix 权限位，整组跳过
            }
            let python = std::process::Command::new("python3").arg("--version").output();
            if python.is_err() || !python.unwrap().status.success() {
                return;
            }
            let (dir, root) = temp_root();
            let stub = stub_shell(dir.path(), "bash");
            std::fs::write(std::path::Path::new(&root).join("bashrc"), "").unwrap();

            let ours =
                run_command(&shell_dispatch(&root, TOKEN), &[("SHELL", stub.to_str().unwrap())]);
            let script = concat!(env!("CARGO_MANIFEST_DIR"), "/res/hooks/remote_shell.py");
            let output = std::process::Command::new("python3")
                .arg(script)
                .arg(TOKEN)
                .env("SHELL", &stub)
                .env_remove("ZDOTDIR")
                .output()
                .unwrap();
            assert!(output.status.success(), "remote_shell.py failed: {output:?}");
            let theirs = String::from_utf8_lossy(&output.stdout);
            for line in [
                format!("PEBREL_REMOTE_HOOK_TOKEN={TOKEN}"),
                format!("NEBULA_REMOTE_HOOK_TOKEN={TOKEN}"),
                "PEBREL_PANE_REMOTE=1".to_owned(),
                "NEBULA_PANE_REMOTE=1".to_owned(),
                "PEBREL_REMOTE_LOGIN=1".to_owned(),
            ] {
                assert!(theirs.contains(&line), "remote_shell.py diverged: {line}");
                assert!(ours.contains(&line), "fast path diverged: {line}");
            }
            assert!(ours.contains("STUB_ARGS:--rcfile"), "{ours}");
            assert!(theirs.contains("STUB_ARGS:--rcfile"), "{theirs}");
        }
    }
}
