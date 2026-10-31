//! Short-lived remote probes share the shell channel's close-on-drop ownership.
use super::{SessionError, lifecycle};
use russh::{Channel, ChannelMsg, client};
use std::time::Duration;

pub(super) async fn capture_private(
    channel: Channel<client::Msg>,
    command: &str,
) -> Result<Vec<u8>, SessionError> {
    let mut channel = lifecycle::own_channel(channel);
    channel.exec(true, command).await?;
    let mut stdout = Vec::new();
    let mut exit = None;
    while let Some(message) = channel.wait().await {
        match message {
            ChannelMsg::Data { data } => {
                if stdout.len().saturating_add(data.len()) > 8192 {
                    return Err("private_exec_output_limit".into());
                }
                stdout.extend_from_slice(&data);
            },
            ChannelMsg::Failure => return Err("private_exec_rejected".into()),
            ChannelMsg::ExitStatus { exit_status } => exit = Some(exit_status),
            ChannelMsg::Close => break,
            _ => {},
        }
    }
    channel.finish().await?;
    if exit != Some(0) {
        return Err("private_exec_failed".into());
    }
    Ok(stdout)
}

/// 外部调用方（`pebrel cli exec`）需要的是可判读的结果，不是"成功串/失败"：
/// 非零退出码、超时、stderr 都要原样带回，否则远端 `exit 7` 和"连不上"分不开。
#[derive(Default)]
pub(crate) struct ExecOutcome {
    pub(crate) exit_code: Option<u32>,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
    pub(crate) timed_out: bool,
    pub(crate) stdout_truncated: bool,
    pub(crate) stderr_truncated: bool,
}

fn collect_into(target: &mut Vec<u8>, data: &[u8], limit: usize, truncated: &mut bool) {
    let room = limit.saturating_sub(target.len());
    target.extend_from_slice(&data[..room.min(data.len())]);
    *truncated |= data.len() > room;
}

pub(super) async fn capture_outcome(
    channel: Channel<client::Msg>,
    command: &str,
    script: &[u8],
    budget: Duration,
    max_bytes: usize,
) -> Result<ExecOutcome, SessionError> {
    let mut channel = lifecycle::own_channel(channel);
    channel.exec(true, command).await?;
    let deadline = tokio::time::Instant::now() + budget;
    let mut outcome = ExecOutcome::default();
    let mut submitted = false;
    loop {
        // 逐条消息重设超时而不是包住整个循环：超时那一刻已经收到的字节仍然
        // 有价值，`stdout` 里半截日志比空字符串更接近事实。
        let message = match tokio::time::timeout_at(deadline, channel.wait()).await {
            Ok(Some(message)) => message,
            Ok(None) => break,
            Err(_) => {
                outcome.timed_out = true;
                break;
            },
        };
        match message {
            ChannelMsg::Success if !submitted => {
                if !script.is_empty() {
                    channel.data_bytes(script.to_vec()).await?;
                }
                channel.eof().await?;
                submitted = true;
            },
            ChannelMsg::Data { data } => {
                collect_into(&mut outcome.stdout, &data, max_bytes, &mut outcome.stdout_truncated)
            },
            ChannelMsg::ExtendedData { data, .. } => {
                collect_into(&mut outcome.stderr, &data, max_bytes, &mut outcome.stderr_truncated)
            },
            ChannelMsg::Failure => return Err("remote server rejected exec request".into()),
            ChannelMsg::ExitStatus { exit_status } => outcome.exit_code = Some(exit_status),
            ChannelMsg::Close => break,
            _ => {},
        }
    }
    let _ = channel.finish().await;
    Ok(outcome)
}

pub(super) async fn capture(
    channel: Channel<client::Msg>,
    command: &str,
    script: &[u8],
    budget: Duration,
    raw_destination: &str,
) -> Result<String, SessionError> {
    let mut channel = lifecycle::own_channel(channel);
    let collect = async {
        channel.exec(true, command).await?;
        let mut submitted = script.is_empty();
        let mut stdout = Vec::new();
        while let Some(message) = channel.wait().await {
            match message {
                ChannelMsg::Success if !submitted => {
                    channel.data_bytes(script.to_vec()).await?;
                    channel.eof().await?;
                    submitted = true;
                },
                ChannelMsg::Data { data } => {
                    if stdout.len().saturating_add(data.len()) > 16 * 1024 * 1024 {
                        return Err("remote probe exceeded its output budget".into());
                    }
                    stdout.extend_from_slice(&data);
                },
                ChannelMsg::Failure => return Err("remote server rejected exec request".into()),
                ChannelMsg::ExitStatus { exit_status } if exit_status != 0 => {
                    return Err(format!("remote command exited with status {exit_status}").into());
                },
                // 标准错误只当诊断线索，不混进结果——远端的 `ps: not found`
                // 之类抱怨不该被当成路径。
                ChannelMsg::ExtendedData { data, .. } => {
                    if let Ok(text) = std::str::from_utf8(&data) {
                        let text = text.trim();
                        if !text.is_empty() {
                            log::debug!("远端命令 stderr（{raw_destination}）: {text}");
                        }
                    }
                },
                ChannelMsg::Eof | ChannelMsg::Close => break,
                _ => {},
            }
        }
        Ok::<_, SessionError>(stdout)
    };

    let result = tokio::time::timeout(budget, collect).await;
    let closed = channel.finish().await;
    match result {
        // 远端文件名和路径未必是合法 UTF-8。有损转换让"大部分能读"胜过
        // "整次探测失败"；真正需要字节精度的路径操作走 SFTP，不走这里。
        Ok(stdout) => {
            closed?;
            Ok(String::from_utf8_lossy(&stdout?).into_owned())
        },
        Err(_) => Err(format!("远端命令超过 {} 秒未返回", budget.as_secs()).into()),
    }
}
