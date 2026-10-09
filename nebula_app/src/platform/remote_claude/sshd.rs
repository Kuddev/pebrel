//! 本机侧的一次性回环 sshd。
//!
//! 远端 Claude Code 通过 `ssh -F … windows '<命令>'` 回到这里，所以本机需要
//! 一个只监听 `127.0.0.1`、只用临时密钥、只活到本次会话结束的 SSH 服务端：
//!
//! - 主机密钥与登录密钥都在内存里现生成（见 [`super::keys`]），落盘的目录
//!   只授予当前用户（见 [`super::local::create_session_dir`]）。
//! - 进程用 `CREATE_SUSPENDED` 起来后立刻挂进 Job Object（`KILL_ON_JOB_CLOSE`），
//!   Pebrel 被强杀时由操作系统回收整棵进程树。
//! - `SSH_TEST_ENVIRONMENT=1` 是 Windows OpenSSH 的无窗口开关：不设它，远端每
//!   执行一条命令都会在用户桌面上弹一个控制台窗口。

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::platform::process::ProcessGroup;

use super::keys::{self, KeyPair};
use super::local;

/// sshd 监听就绪的等待上限。冷启动实测在 1 秒内，15 秒只是坏掉时的出口。
const LISTEN_TIMEOUT: Duration = Duration::from_secs(15);

pub(super) struct LoopbackSshd {
    directory: PathBuf,
    child: Child,
    group: Option<ProcessGroup>,
    port: u16,
    identity: KeyPair,
    host: KeyPair,
}

impl LoopbackSshd {
    /// 在 `directory` 里生成密钥、写配置并把 `sshd.exe` 挂进 Job Object。
    pub(super) fn start(directory: PathBuf, openssh: &Path) -> Result<Self, String> {
        let started = Self::launch(&directory, openssh);
        if started.is_err() {
            // 失败路径也要收走临时密钥；成功路径由 Drop 负责。
            let _ = std::fs::remove_dir_all(&directory);
        }
        started
    }

    fn launch(directory: &Path, openssh: &Path) -> Result<Self, String> {
        let host = keys::generate()?;
        let identity = keys::generate()?;
        let host_key = directory.join("host");
        keys::write_private(&host_key, &host).map_err(|error| error.to_string())?;
        // 登录私钥只留在内存里（远端拿到它才能回连），本机 sshd 只认公钥。
        std::fs::write(directory.join("authorized_keys"), format!("{}\n", identity.public))
            .map_err(|error| error.to_string())?;

        let port = local::free_loopback_port()?;
        let config = directory.join("sshd_config");
        std::fs::write(&config, config_text(&directory, &host_key, port, openssh))
            .map_err(|error| error.to_string())?;

        let log = directory.join("sshd.log");
        let log_file =
            std::fs::File::create(&log).map_err(|error| format!("{}: {error}", log.display()))?;
        let mut command = Command::new(openssh.join("sshd.exe"));
        command
            .arg("-D")
            .arg("-e")
            .arg("-f")
            .arg(&config)
            // Windows OpenSSH: 命令子进程不创建控制台窗口；见模块文档。
            .env("SSH_TEST_ENVIRONMENT", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(log_file));
        crate::platform::process::configure_process_group(&mut command);
        let mut child = command.spawn().map_err(|error| format!("spawn sshd.exe: {error}"))?;
        let group = ProcessGroup::attach(&child)
            .map_err(|error| format!("attach sshd to the job object: {error}"))?;
        // 记录属主进程：强杀后 Drop 不跑，下一次连接据此立刻回收临时目录。
        let _ = std::fs::write(directory.join("sshd.pid"), child.id().to_string());
        if let Err(error) = wait_until_listening(&mut child, port, &log) {
            group.terminate(&mut child);
            let _ = child.wait();
            group.finish();
            return Err(error);
        }
        Ok(Self {
            directory: directory.to_owned(),
            child,
            group: Some(group),
            port,
            identity,
            host,
        })
    }

    /// 本机监听端口；远端 `-R` 的转发目标。
    pub(super) fn port(&self) -> u16 {
        self.port
    }

    /// 远端登录用的临时私钥（OpenSSH 文本）。
    pub(super) fn identity(&self) -> &str {
        &self.identity.private
    }

    /// 本机 sshd 的主机公钥，写进远端的 `known_hosts`。
    pub(super) fn host_key(&self) -> &str {
        &self.host.public
    }
}

impl Drop for LoopbackSshd {
    fn drop(&mut self) {
        if let Some(group) = self.group.take() {
            group.terminate(&mut self.child);
            let _ = self.child.wait();
            // 关闭 Job 句柄：即使还有孙进程在跑也一并回收。
            group.finish();
        }
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

/// `sshd_config` 文本。
///
/// 故意不写 `AllowUsers`：Windows 账户名大小写与 OpenSSH 的模式匹配不一致时
/// 会把自己挡在门外；这里的安全性由"只监听回环 + 只信任本次临时密钥"提供。
/// 也不开 `AllowTcpForwarding`：本机 sshd 只接受回连登录，不需要自己做转发。
fn config_text(directory: &Path, host_key: &Path, port: u16, openssh: &Path) -> String {
    let slash = |path: &Path| path.display().to_string().replace('\\', "/");
    let mut config = vec![
        format!("Port {port}"),
        "ListenAddress 127.0.0.1".to_owned(),
        format!("HostKey \"{}\"", slash(host_key)),
        format!("AuthorizedKeysFile \"{}\"", slash(&directory.join("authorized_keys"))),
        "PubkeyAuthentication yes".to_owned(),
        "PasswordAuthentication no".to_owned(),
        "KbdInteractiveAuthentication no".to_owned(),
        "PermitEmptyPasswords no".to_owned(),
        "AllowTcpForwarding no".to_owned(),
        "PermitTunnel no".to_owned(),
        "X11Forwarding no".to_owned(),
        "PermitRootLogin no".to_owned(),
        "PrintMotd no".to_owned(),
        "LogLevel ERROR".to_owned(),
    ];
    let sftp = openssh.join("sftp-server.exe");
    if sftp.is_file() {
        // 远端提示词允许模型用 scp 传文件，scp 默认走 sftp 子系统。
        config.push(format!("Subsystem sftp \"{}\"", slash(&sftp)));
    }
    config.join("\n") + "\n"
}

fn wait_until_listening(child: &mut Child, port: u16, log: &Path) -> Result<(), String> {
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let deadline = Instant::now() + LISTEN_TIMEOUT;
    loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            return Err(format!("sshd exited with {status}: {}", log_tail(log)));
        }
        if std::net::TcpStream::connect_timeout(&address, Duration::from_millis(300)).is_ok() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "sshd did not listen on 127.0.0.1:{port} within {}s: {}",
                LISTEN_TIMEOUT.as_secs(),
                log_tail(log)
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// 失败时把 sshd 的最后几行日志带进错误里，不静默吞掉。
fn log_tail(path: &Path) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<_> = text.lines().filter(|line| !line.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(3)..].join(" | ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_pins_loopback_key_auth_and_sftp() {
        let directory = Path::new(r"C:\Temp\pebrel\sessions\abc");
        let openssh = Path::new(r"C:\Program Files\Pebrel\runtime\openssh");
        let config = config_text(directory, &directory.join("host"), 51234, openssh);
        assert!(config.contains("Port 51234\n"));
        assert!(config.contains("ListenAddress 127.0.0.1\n"));
        assert!(config.contains(r#"HostKey "C:/Temp/pebrel/sessions/abc/host""#));
        assert!(
            config.contains(r#"AuthorizedKeysFile "C:/Temp/pebrel/sessions/abc/authorized_keys""#)
        );
        assert!(config.contains("PasswordAuthentication no\n"));
        // 账户名大小写匹配的坑：不写 AllowUsers。
        assert!(!config.contains("AllowUsers"));
    }

    #[test]
    fn config_omits_sftp_when_the_bundle_has_none() {
        let directory = Path::new(r"C:\Temp\abc");
        let config = config_text(directory, &directory.join("host"), 1, Path::new(r"C:\missing"));
        assert!(!config.contains("Subsystem"));
    }
}
