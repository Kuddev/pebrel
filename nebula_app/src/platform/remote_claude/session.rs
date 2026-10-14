//! 一次远程 Claude Code 会话的编排。
//!
//! 顺序（R5 的四阶段就挂在上面）：
//!
//! 1. 本机起一次性回环 sshd，先把主机密钥、登录密钥和配置准备好；
//! 2. `ssh <host> exec /bin/sh -s` 跑预检/下发脚本：判断 claude 是否可用、是否
//!    已登录，建远端镜像目录，写入本次登录私钥，并在远端选一个空闲回环端口；
//! 3. `ssh -tt -o ExitOnForwardFailure=yes -R …` 建立反向通道并前台运行 claude；
//!    远端脚本依次打印"回连通道建立 / 回连自检通过 / 会话启动"，退出时清掉
//!    自己的临时目录。
//!
//! 本机不替远端做兜底清理：远端目录的生命周期由远端脚本的 trap 和下一次连接
//! 的残留扫描负责，两边都删会与仍在运行的远端会话抢文件。

use std::io::{BufRead as _, IsTerminal as _, Write as _};
use std::process::{Command, Stdio};

use super::hosts::{self, HostCandidate};
use super::local;
use super::mirror;
use super::prompt::{PromptInput, system_prompt};
use super::script::{self, Provision, Report, Session, SessionTexts};
use super::sshd::LoopbackSshd;
use crate::cli::ClaudeOptions;
use crate::i18n::{LanguagePreference, Message, UiLanguage};

/// 远端 claude 退出码 255 在本进程里另有含义：那条 ssh 自己断了。
const SSH_TRANSPORT_FAILURE: i32 = 255;

pub(crate) fn run(options: ClaudeOptions) -> i32 {
    let language =
        LanguagePreference::from(nebula_settings::RuntimeSettings::load().language).resolved();
    match connect(&options, language) {
        Ok(code) => code,
        Err(message) => {
            stage_frame("failed", options.ssh.as_deref().unwrap_or_default(), &message);
            // 阶段与错误都走终端；GUI 壳没有第二条通道能比这里更早告诉用户原因。
            eprintln!("{message}");
            1
        },
    }
}

/// 会话建立阶段帧（OSC 777）：只在 Pebrel 自己托管的 pane 里发，宿主把它翻成
/// 连接卡片（`gpui_shell::terminal::ssh_connect_overlay::remote_claude_stage`）。
/// 其它终端与管道里这些字节只会变成垃圾，所以按 `TERM_PROGRAM` 门控。
fn stage_frame(stage: &str, host: &str, detail: &str) {
    use std::io::Write as _;

    if !card_hosted() {
        return;
    }
    // 帧是单行协议：换行会把后面的文本变成"看起来像帧"的垃圾。
    let one_line = |text: &str| text.replace(['\r', '\n'], " ");
    let mut stdout = std::io::stdout();
    let _ = write!(
        stdout,
        "\x1b]777;pebrel-remote-claude;{stage};{};{}\x07",
        one_line(host),
        one_line(detail)
    );
    let _ = stdout.flush();
}

/// 本进程的宿主是不是会画连接卡片的 Pebrel 面板。
///
/// 同一条判据管两件事：阶段帧发给宿主，对应的人读阶段行就不打（卡片就在同一
/// 块屏幕上，两份文本只会互相抢位置）。普通终端、管道、别的终端都走文字那条。
fn card_hosted() -> bool {
    std::env::var("TERM_PROGRAM").as_deref() == Ok("pebrel")
}

fn connect(options: &ClaudeOptions, language: UiLanguage) -> Result<i32, String> {
    let project = project_directory(options.cwd.as_deref(), language)?;
    let relative = mirror::relative_mirror(&project).map_err(|error| {
        language
            .format(Message::RemoteClaudeErrorProjectUnmapped, &[("detail", &error.to_string())])
    })?;
    let project_key = mirror::project_key(&relative);
    let host = choose_host(options.ssh.as_deref(), language)?;
    // 卡片立刻出现：这一刻我们确实在启动本机回环服务。
    stage_frame("local", &host.destination, "");
    let windows_user = local::user_name().map_err(|detail| local_failure(language, &detail))?;
    let run_id = local::run_id().map_err(|detail| local_failure(language, &detail))?;

    let openssh = local::openssh_directory().map_err(|detail| {
        language.format(Message::RemoteClaudeErrorOpensshMissing, &[("detail", &detail)])
    })?;
    let directory =
        local::create_session_dir(&run_id).map_err(|detail| local_failure(language, &detail))?;
    let sshd = LoopbackSshd::start(directory, &openssh)
        .map_err(|detail| local_failure(language, &detail))?;
    if !card_hosted() {
        println!(
            "{}",
            language.format(
                Message::RemoteClaudeStageLocalChannel,
                &[("port", &sshd.port().to_string())]
            )
        );
    }

    let ports = local::candidate_ports().map_err(|detail| local_failure(language, &detail))?;
    let provision = script::provision_script(&Provision {
        run_id: &run_id,
        relative: &relative,
        project_key: &project_key,
        ports: &ports,
        identity: sshd.identity(),
    });
    let server = provision_remote(&host, &provision, language)?;
    // 预检通过：接下来那条 `ssh -R` 才是回连通道本体。
    stage_frame("tunnel", &host.destination, "");

    let powershell = local::powershell().map_err(|detail| local_failure(language, &detail))?;
    let ssh_config = format!(
        "{}/.pebrel-remote/projects/{project_key}/ssh_config",
        server.home.trim_end_matches('/')
    );
    let shell = local::command_shell();
    let prompt = system_prompt(&PromptInput {
        project: &project,
        ssh_config: &ssh_config,
        mirror: &server.mirror,
        shell,
        powershell: &powershell,
    });
    let probe = local::probe_command(&powershell, shell)
        .map_err(|detail| local_failure(language, &detail))?;
    let texts = session_texts(language, server.port);
    let command = script::session_command(&Session {
        run_id: &run_id,
        project_key: &project_key,
        remote_port: server.port,
        host: &host.destination,
        windows_user: &windows_user,
        host_key: sshd.host_key(),
        mirror: &server.mirror,
        probe: &probe,
        prompt: &prompt,
        claude_args: &options.args,
        texts: &texts,
        card: card_hosted(),
    });

    let code = run_session(&host, server.port, sshd.port(), &command, language)?;
    if code == SSH_TRANSPORT_FAILURE {
        return Err(ssh_failure(language, &host.destination, &format!("exit {code}")));
    }
    Ok(code)
}

/// 本机项目目录：规范化后去掉 `\\?\` 前缀，再作为镜像与提示词的输入。
fn project_directory(
    cwd: Option<&std::path::Path>,
    language: UiLanguage,
) -> Result<String, String> {
    let requested = match cwd {
        Some(path) => path.to_path_buf(),
        None => std::env::current_dir()
            .map_err(|error| project_failure(language, &format!("current directory: {error}")))?,
    };
    let canonical = std::fs::canonicalize(&requested)
        .map_err(|error| project_failure(language, &format!("{}: {error}", requested.display())))?;
    if !canonical.is_dir() {
        return Err(project_failure(
            language,
            &format!("{} is not a directory", canonical.display()),
        ));
    }
    Ok(mirror::display_path(&canonical))
}

/// 用户点名的服务器，或从候选中选一台（交互式终端才有选择余地）。
fn choose_host(requested: Option<&str>, language: UiLanguage) -> Result<ChosenHost, String> {
    let candidates = hosts::candidates();
    if let Some(name) = requested {
        let name = name.trim();
        if name.is_empty() {
            return Err(language.text(Message::RemoteClaudeErrorHostRequired).to_owned());
        }
        return Ok(ChosenHost {
            destination: resolved_destination(name, &candidates),
            identity: name.to_owned(),
        });
    }
    if candidates.is_empty() {
        return Err(language.text(Message::RemoteClaudeErrorNoHosts).to_owned());
    }
    if !std::io::stdin().is_terminal() {
        return Err(language.text(Message::RemoteClaudeErrorHostRequired).to_owned());
    }
    let selected = prompt_for_host(&candidates, language)?;
    Ok(ChosenHost {
        destination: resolved_destination(&selected.name, &candidates),
        identity: selected.name,
    })
}

/// 交给 `ssh` 的目的地：已保存主机可能带显式端口（`user@host:2222`），而
/// `ssh` 只把 `ssh://user@host:2222` 认成"主机 + 端口"，裸冒号形式会被当成
/// 主机名。转换规则复用 `ssh::run` 的同一处实现，不另写一份解析。
fn resolved_destination(name: &str, candidates: &[HostCandidate]) -> String {
    crate::ssh_session::ssh_config_probe_target(&hosts::resolve(name, candidates)).into_owned()
}

fn prompt_for_host(
    candidates: &[HostCandidate],
    language: UiLanguage,
) -> Result<HostCandidate, String> {
    println!("{}", language.text(Message::RemoteClaudeHostsPrompt));
    for (index, host) in candidates.iter().enumerate() {
        if host.label.is_empty() {
            println!("  {:>2}. {}", index + 1, host.name);
        } else {
            println!("  {:>2}. {} ({})", index + 1, host.label, host.name);
        }
    }
    let stdin = std::io::stdin();
    loop {
        print!("{}", language.text(Message::RemoteClaudeHostsChoice));
        std::io::stdout().flush().map_err(|error| error.to_string())?;
        let mut line = String::new();
        if stdin.lock().read_line(&mut line).map_err(|error| error.to_string())? == 0 {
            return Err(language.text(Message::RemoteClaudeErrorCancelled).to_owned());
        }
        let answer = line.trim();
        if answer.eq_ignore_ascii_case("q") {
            return Err(language.text(Message::RemoteClaudeErrorCancelled).to_owned());
        }
        if let Ok(index) = answer.parse::<usize>()
            && (1..=candidates.len()).contains(&index)
        {
            return Ok(candidates[index - 1].clone());
        }
    }
}

struct ChosenHost {
    /// 交给 `ssh` 的目的地（已解析过的别名或 `user@host`）。
    destination: String,
    /// AskPass 凭据表的键：用户输入的那个名字。
    identity: String,
}

struct ServerReport {
    home: String,
    mirror: String,
    port: u16,
}

/// 预检/下发：脚本走 stdin，报告走 stdout 的 `PEBREL|…` 行。
fn provision_remote(
    host: &ChosenHost,
    script_text: &str,
    language: UiLanguage,
) -> Result<ServerReport, String> {
    let ssh = crate::ssh::find_ssh();
    let mut command = Command::new(&ssh);
    command
        .args(["-o", "ConnectTimeout=10", "--"])
        .arg(&host.destination)
        .arg(script::PROVISION_COMMAND)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let askpass = askpass_environment(host);
    if let Some(askpass) = &askpass {
        command.envs(&askpass.values);
    }
    // 非交互探针：不压掉就会从无控制台的 GUI 进程里弹出控制台窗口。
    crate::platform::process::hidden_command(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| ssh_failure(language, &host.destination, &error.to_string()))?;

    let mut stdin = child.stdin.take().ok_or_else(|| "ssh stdin".to_owned())?;
    let script_text = script_text.to_owned();
    let writer = std::thread::spawn(move || {
        // 写失败（例如 ssh 已经退出）不是权威错误：脚本没送到时远端不会报
        // ready，下面按退出码与 stderr 报告，不在这里重复判断。
        let _ = stdin.write_all(script_text.as_bytes());
        // 关掉 stdin：预检脚本靠 EOF 结束读取，一直开着会等到超时。
        drop(stdin);
    });
    let output = child
        .wait_with_output()
        .map_err(|error| ssh_failure(language, &host.destination, &error.to_string()))?;
    let _ = writer.join();
    if let Some(askpass) = askpass {
        let _ = std::fs::remove_file(askpass.attempt_path);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut report = ServerReport { home: String::new(), mirror: String::new(), port: 0 };
    let mut ready = false;
    for line in stdout.lines() {
        match script::parse_report(line) {
            Some(Report::Home(home)) => report.home = home,
            Some(Report::Mirror(mirror)) => report.mirror = mirror,
            Some(Report::Port(port)) => report.port = port,
            Some(Report::Ready) => ready = true,
            Some(Report::Error { code, detail }) => {
                return Err(report_failure(language, &code, &detail));
            },
            // 登录 shell 的问候语、提示符或其他噪声：不是报告行，忽略。
            Some(Report::Claude(_) | Report::Auth(_)) | None => {},
        }
    }
    if !output.status.success() || !ready {
        let detail = stderr_detail(&output.stderr);
        return Err(ssh_failure(
            language,
            &host.destination,
            &if detail.is_empty() {
                format!("exit status {}", output.status.code().unwrap_or(-1))
            } else {
                detail
            },
        ));
    }
    if report.home.trim().is_empty() || report.mirror.trim().is_empty() || report.port == 0 {
        return Err(report_failure(language, "incomplete_report", stdout.trim()));
    }
    Ok(report)
}

/// 会话阶段：反向通道 + 前台 claude。stdin/stdout 交给用户终端。
fn run_session(
    host: &ChosenHost,
    remote_port: u16,
    local_port: u16,
    command: &str,
    language: UiLanguage,
) -> Result<i32, String> {
    let ssh = crate::ssh::find_ssh();
    let mut session = Command::new(&ssh);
    session
        .args(["-tt", "-o", "ExitOnForwardFailure=yes"])
        .args(["-o", "ServerAliveInterval=30", "-o", "ServerAliveCountMax=3"])
        .arg("-R")
        .arg(format!("127.0.0.1:{remote_port}:127.0.0.1:{local_port}"))
        .arg("--")
        .arg(&host.destination)
        .arg(command);
    let askpass = askpass_environment(host);
    if let Some(askpass) = &askpass {
        session.envs(&askpass.values);
    }
    // 交互会话：继承父控制台（见 `ssh::run`），不能加 CREATE_NO_WINDOW，
    // 否则用户看不到远程 Claude 的界面，也无法输入。
    let status = session.status();
    if let Some(askpass) = askpass {
        let _ = std::fs::remove_file(askpass.attempt_path);
    }
    let status =
        status.map_err(|error| ssh_failure(language, &host.destination, &error.to_string()))?;
    Ok(status.code().unwrap_or(SSH_TRANSPORT_FAILURE))
}

/// Pebrel 已保存的主机可能存了密码：复用 `pebrel ssh` 的 AskPass 通道。
fn askpass_environment(host: &ChosenHost) -> Option<crate::ssh::SshAskpassEnv> {
    let exe = std::env::current_exe().ok()?;
    Some(crate::ssh::build_askpass_env(&exe, &host.identity, std::process::id() as u64))
}

fn session_texts(language: UiLanguage, remote_port: u16) -> SessionTexts {
    SessionTexts {
        tunnel: language
            .format(Message::RemoteClaudeStageTunnel, &[("port", &remote_port.to_string())]),
        // 只有服务器知道的那个值用 `\0` 占位：`printf_template` 把它换成 `%s`。
        probe: script::printf_template(
            &language.format(Message::RemoteClaudeStageProbe, &[("detail", "\u{0}")]),
        ),
        session: language.text(Message::RemoteClaudeStageSession).to_owned(),
        not_provisioned: language.text(Message::RemoteClaudeErrorNotProvisioned).to_owned(),
        probe_failed: script::printf_template(
            &language.format(Message::RemoteClaudeErrorProbeFailed, &[("detail", "\u{0}")]),
        ),
        mirror_missing: language.text(Message::RemoteClaudeErrorMirrorMissing).to_owned(),
    }
}

/// 预检脚本的错误码 → 用户可执行的提示；未知错误码原样展示。
fn report_failure(language: UiLanguage, code: &str, detail: &str) -> String {
    let literal = match code {
        "claude_missing" => Some(Message::RemoteClaudeErrorClaudeMissing),
        "claude_logged_out" => Some(Message::RemoteClaudeErrorClaudeLoggedOut),
        "remote_ssh_missing" => Some(Message::RemoteClaudeErrorRemoteSshMissing),
        "home_missing" => Some(Message::RemoteClaudeErrorHomeMissing),
        "no_free_port" => Some(Message::RemoteClaudeErrorNoFreePort),
        _ => None,
    };
    if let Some(message) = literal {
        return language.text(message).to_owned();
    }
    let templated = match code {
        "claude_broken" => Some(Message::RemoteClaudeErrorClaudeBroken),
        "mirror_unwritable" => Some(Message::RemoteClaudeErrorMirrorUnwritable),
        "state_unwritable" => Some(Message::RemoteClaudeErrorStateUnwritable),
        _ => None,
    };
    match templated {
        Some(message) => language.format(message, &[("detail", detail)]),
        None => {
            language.format(Message::RemoteClaudeErrorReport, &[("code", code), ("detail", detail)])
        },
    }
}

fn local_failure(language: UiLanguage, detail: &str) -> String {
    language.format(Message::RemoteClaudeErrorLocalChannel, &[("detail", detail)])
}

fn project_failure(language: UiLanguage, detail: &str) -> String {
    language.format(Message::RemoteClaudeErrorProjectUnmapped, &[("detail", detail)])
}

fn ssh_failure(language: UiLanguage, host: &str, detail: &str) -> String {
    language.format(Message::RemoteClaudeErrorSshFailed, &[("host", host), ("detail", detail)])
}

/// ssh 的 stderr 末尾几行：ssh 自己的报错比退出码有用。
fn stderr_detail(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let lines: Vec<_> = text.lines().filter(|line| !line.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(3)..].join(" ")
}
