//! `pebrel cli` 用的无人值守 SSH 路径。
//!
//! 与终端 pane 共用连接池和凭据库，但**没有用户在场**：不弹密码框、不弹主机
//! 密钥确认、不写凭据库，尤其不执行 `authenticate` 结尾的 `forget_password`
//! ——否则一次凭据过期的自动尝试会静默删掉用户保存的密码。

use std::time::Duration;

use russh_sftp::client::SftpSession;

use super::{
    NoopSshEventHost, SessionError, SharedSession, SshDestination, exec, lifecycle,
    resolve_connection_route,
};

async fn unattended_session(raw_destination: &str) -> Result<SharedSession, SessionError> {
    let profiles_path = crate::display::nebula_data_dir().join("ssh_profiles.json");
    let raw = raw_destination.to_owned();
    let (destination, profile) = tokio::task::spawn_blocking(move || {
        let destination = SshDestination::resolve(&raw)?;
        let profiles = crate::ssh_profiles::SshProfiles::load(&profiles_path)
            .unwrap_or_else(|_| crate::ssh_profiles::SshProfiles::default());
        Ok::<_, std::io::Error>((destination, profiles.for_destination(&raw)))
    })
    .await
    .map_err(|err| format!("SSH 地址解析任务失败: {err}"))??;
    let route = resolve_connection_route(&destination, &profile, 0, None).await?;
    // 第四参数 false：未知主机键直接拒绝，而不是等人点确认。
    Ok(super::authenticated_route(&route, None::<&NoopSshEventHost>, false, false).await?.session)
}

pub(crate) async fn exec_outcome(
    raw_destination: &str,
    command: &str,
    stdin: &[u8],
    budget: Duration,
    max_bytes: usize,
) -> Result<exec::ExecOutcome, SessionError> {
    let session = unattended_session(raw_destination).await?;
    let channel = lifecycle::network("exec channel", session.channel_open_session()).await?;
    exec::capture_outcome(channel, command, stdin, budget, max_bytes).await
}

pub(crate) async fn open_sftp_unattended(
    raw_destination: &str,
) -> Result<SftpSession, SessionError> {
    let session = unattended_session(raw_destination).await?;
    let channel = lifecycle::network("SFTP channel", session.channel_open_session()).await?;
    channel.request_subsystem(true, "sftp").await?;
    Ok(SftpSession::new_with_config(
        channel.into_stream(),
        crate::ssh_sftp::limits::session_config(),
    )
    .await?)
}
