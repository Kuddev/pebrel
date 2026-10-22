//! SSH 资产、远程执行与文件传输的本地控制面。
//!
//! 供 `pebrel cli` 与任何本机客户端使用。凭据边界是硬性的：请求里只有
//! `asset_id`，密码/私钥/口令始终留在系统凭据库，由本进程在连接时取用；
//! 任何方法都不接受、也不返回 secret 字节。
//!
//! 除 `ssh.shell` 外，全部在传输线程上直接执行：冷连接认证是秒级、同步是
//! 分钟级，投给 UI 事件线程会把窗口冻住。

use super::*;
use crate::cli::ShortOutput;
use clap::{Args, Subcommand};
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::OpenFlags;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const DEFAULT_TIMEOUT_MS: u64 = 60_000;
const MAX_TIMEOUT_MS: u64 = 24 * 60 * 60 * 1000;
const MAX_COMMAND_BYTES: usize = 8 * 1024;
const MAX_ASSETS: usize = 64;
const MAX_AUDIT_BYTES: u64 = 4 * 1024 * 1024;
const AUDIT_COMMAND_BYTES: usize = 4096;
const TRANSFER_CHUNK: usize = 64 * 1024;

// ---- 协议入口 ----

pub(super) fn connection(
    stream: &mut TcpStream,
    request: ApiRequest,
    sink: &EventSink,
    hub: &RuntimeHub,
) -> Result<(), IoError> {
    let method = request.method.clone();
    let result = match method.as_str() {
        "ssh.assets.list" => assets_list(),
        "ssh.assets.show" => assets_show(&request.params),
        "ssh.credentials.list" => credentials_list(&request.params),
        "ssh.exec" => exec(&request.params),
        "ssh.upload" => transfer(&request.params, Direction::Upload),
        "ssh.download" => transfer(&request.params, Direction::Download),
        "ssh.sync" => sync(&request.params),
        "ssh.shell" => shell(&request.params, sink, hub),
        _ => Err(ApiError::new("method_not_found", format!("unknown SSH method {method}"))),
    };
    let response = match result {
        Ok(value) => ApiResponse::success(request.id, value),
        Err(error) => ApiResponse::failure(request.id, error),
    };
    write_response(stream, &response)
}

// ---- 闸门与审计 ----

/// 授权闸门：默认关。关着的时候连一次网络请求都不该发出去——失败必须发生在
/// 解析出目标之前，否则"关着"只是话术。
fn require_ai_control(method: &str, asset_id: &str) -> Result<(), ApiError> {
    let raw = nebula_settings::RawSettings::load();
    if nebula_settings::RuntimeSettings::from_raw(&raw).ssh_ai_control {
        return Ok(());
    }
    audit("denied", asset_id, false, json!({"method": method}));
    Err(ApiError::new(
        "ssh_control_disabled",
        "AI control of saved servers is off. Enable \"Allow AI to control servers\" in Settings › SSH.",
    )
    .details(json!({ "method": method, "asset_id": asset_id, "setting": "ssh_ai_control" })))
}

/// 追加一行审计。写不进去只降级为日志——审计是诊断线索，不是策略执行点。
fn audit(event: &str, asset_id: &str, ok: bool, extra: Value) {
    let path = crate::display::nebula_data_dir().join("ssh_audit.jsonl");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if std::fs::metadata(&path).map(|meta| meta.len() > MAX_AUDIT_BYTES).unwrap_or(false) {
        let _ = std::fs::rename(&path, path.with_extension("jsonl.1"));
    }
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let mut record = json!({ "ts": ts, "event": event, "asset_id": asset_id, "ok": ok });
    if let (Some(record), Some(extra)) = (record.as_object_mut(), extra.as_object()) {
        for (key, value) in extra {
            record.insert(key.clone(), value.clone());
        }
    }
    let mut line = record.to_string();
    line.push('\n');
    match std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        Ok(mut file) => {
            if let Err(error) = file.write_all(line.as_bytes()) {
                warn!("SSH audit write failed: {error}");
            }
        },
        Err(error) => warn!("SSH audit open failed: {error}"),
    }
}

fn audit_command(command: &str) -> (String, bool) {
    if command.len() <= AUDIT_COMMAND_BYTES {
        return (command.to_owned(), false);
    }
    let mut cut = AUDIT_COMMAND_BYTES;
    while cut > 0 && !command.is_char_boundary(cut) {
        cut -= 1;
    }
    (command[..cut].to_owned(), true)
}

// ---- 资产 ----

fn load_profiles() -> Result<crate::ssh_profiles::SshProfiles, ApiError> {
    crate::ssh_profiles::SshProfiles::load(
        &crate::display::nebula_data_dir().join("ssh_profiles.json"),
    )
    .map_err(|error| {
        ApiError::new("ssh_profiles_unreadable", format!("cannot read saved servers: {error}"))
    })
}

fn asset_json(
    profile: &crate::ssh_profiles::SshProfileAuth,
    with_details: bool,
) -> Result<Value, ApiError> {
    // 只探测"存没存"，读到的字节立刻清零，绝不进返回值。
    let mut stored = crate::ssh_credentials::load_stored_password(&profile.destination)
        .unwrap_or_else(|error| {
            warn!("Could not probe saved SSH password: {error}");
            None
        });
    let has_stored_password = stored.is_some();
    if let Some(password) = stored.as_mut() {
        password.fill(0);
    }
    let mut asset = json!({
        "asset_id": profile.destination,
        "auth": profile.auth,
        "label": profile.label.as_deref().map(str::trim).filter(|label| !label.is_empty()),
        "has_stored_password": has_stored_password,
        "private_key_count": profile.private_keys.len(),
    });
    if !with_details {
        return Ok(asset);
    }
    let profiles = load_profiles()?;
    let organization = profiles.organization(&profile.destination);
    let keys: Vec<String> =
        profile.private_keys.iter().map(|path| path.display().to_string()).collect();
    asset["icon"] = json!(profile.icon.as_deref().map(str::trim).filter(|icon| !icon.is_empty()));
    asset["group"] = json!(organization.group);
    asset["tags"] = json!(organization.tags);
    asset["notes"] = json!(organization.notes);
    asset["private_keys"] = json!(keys);
    asset["jump_host"] = json!(profile.connection.jump_host);
    Ok(asset)
}

fn assets_list() -> Result<Value, ApiError> {
    let profiles = load_profiles()?;
    let entries = profiles.entries();
    let mut assets = Vec::new();
    for profile in entries.iter().take(MAX_ASSETS) {
        assets.push(asset_json(profile, false)?);
    }
    Ok(json!({ "assets": assets, "truncated": entries.len() > MAX_ASSETS }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AssetParams {
    asset_id: String,
}

fn assets_show(params: &Value) -> Result<Value, ApiError> {
    let params: AssetParams = parse_params(params)?;
    let profiles = load_profiles()?;
    let profile = profiles.entries().iter().find(|entry| entry.destination == params.asset_id);
    let Some(profile) = profile else {
        let known: Vec<&str> =
            profiles.entries().iter().take(20).map(|entry| entry.destination.as_str()).collect();
        return Err(ApiError::new(
            "ssh_asset_not_found",
            format!("no saved server matches {}", params.asset_id),
        )
        .details(json!({ "asset_id": params.asset_id, "known": known })));
    };
    Ok(json!({ "asset": asset_json(profile, true)? }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CredentialsParams {
    #[serde(default)]
    asset_id: Option<String>,
}

/// 只回答"有没有"，永不回内容，也不提供写入动词——secret 没有安全的输入信道。
fn credentials_list(params: &Value) -> Result<Value, ApiError> {
    let params: CredentialsParams = parse_params(params)?;
    let profiles = load_profiles()?;
    let mut credentials = Vec::new();
    for profile in profiles.entries() {
        if params.asset_id.as_deref().is_some_and(|id| id != profile.destination) {
            continue;
        }
        let mut passphrase = profile.private_keys.iter().find_map(|path| {
            let key = std::fs::read(path).ok()?;
            crate::ssh_credentials::load_private_key_passphrase(&key).ok().flatten()
        });
        let has_passphrase = passphrase.is_some();
        if let Some(secret) = passphrase.as_mut() {
            secret.fill(0);
        }
        credentials.push(json!({
            "asset_id": profile.destination,
            "auth": profile.auth,
            "has_private_key_passphrase": has_passphrase,
            "private_keys": profile.private_keys
                .iter().map(|path| path.display().to_string()).collect::<Vec<_>>(),
        }));
        if credentials.len() >= MAX_ASSETS {
            break;
        }
    }
    Ok(json!({ "credentials": credentials }))
}

// ---- 远程执行 ----

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecParams {
    asset_id: String,
    command: String,
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u64,
    #[serde(default = "default_output_bytes")]
    max_output_bytes: usize,
}

fn default_timeout_ms() -> u64 {
    DEFAULT_TIMEOUT_MS
}

fn default_output_bytes() -> usize {
    DEFAULT_EXEC_OUTPUT_BYTES
}

/// 在传输线程上跑异步 SSH：`ssh_session::runtime()` 是进程级多线程 runtime，
/// 当前线程不是 tokio worker，`block_on` 不会冲突。
fn block_on<T>(
    future: impl std::future::Future<Output = Result<T, crate::ssh_session::SessionError>>,
) -> Result<T, ApiError> {
    let runtime = crate::ssh_session::runtime()
        .map_err(|error| ApiError::new("runtime_unavailable", error.to_string()))?;
    runtime.block_on(future).map_err(|error| {
        let message = error.to_string();
        let code = if message.contains("key") || message.contains("密钥") {
            "ssh_host_key_unverified"
        } else {
            "ssh_connect_failed"
        };
        ApiError::new(code, message)
    })
}

fn exec(params: &Value) -> Result<Value, ApiError> {
    let params: ExecParams = parse_params(params)?;
    if params.command.trim().is_empty() {
        return Err(ApiError::invalid_params("command is empty"));
    }
    if params.command.len() > MAX_COMMAND_BYTES || params.command.contains('\0') {
        return Err(ApiError::invalid_params(format!(
            "command must be at most {MAX_COMMAND_BYTES} bytes and free of NUL"
        )));
    }
    if params.timeout_ms == 0
        || params.timeout_ms > MAX_TIMEOUT_MS
        || params.max_output_bytes == 0
        || params.max_output_bytes > MAX_EXEC_OUTPUT_BYTES
    {
        return Err(ApiError::invalid_params("timeout or output budget out of range"));
    }
    require_ai_control("ssh.exec", &params.asset_id)?;
    let started = Instant::now();
    let outcome = block_on(crate::ssh_session::exec_outcome(
        &params.asset_id,
        &params.command,
        b"",
        Duration::from_millis(params.timeout_ms),
        params.max_output_bytes,
    ));
    let (command, command_truncated) = audit_command(&params.command);
    let mut fields = json!({
        "command": command,
        "command_truncated": command_truncated,
        "duration_ms": started.elapsed().as_millis() as u64,
    });
    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(error) => {
            fields["error"] = json!(error.code);
            audit("exec", &params.asset_id, false, fields);
            return Err(error);
        },
    };
    fields["exit_code"] = json!(outcome.exit_code);
    fields["timed_out"] = json!(outcome.timed_out);
    audit("exec", &params.asset_id, true, fields);
    // 非零退出码与超时都不是协议错误：远端确实执行了，结果就是它。
    Ok(json!({
        "asset_id": params.asset_id,
        "command": params.command,
        "exit_code": outcome.exit_code,
        "success": outcome.exit_code == Some(0) && !outcome.timed_out,
        "timed_out": outcome.timed_out,
        "duration_ms": started.elapsed().as_millis() as u64,
        "stdout": String::from_utf8_lossy(&outcome.stdout),
        "stderr": String::from_utf8_lossy(&outcome.stderr),
        "capture": {
            "stdout": { "bytes": outcome.stdout.len(), "truncated": outcome.stdout_truncated },
            "stderr": { "bytes": outcome.stderr.len(), "truncated": outcome.stderr_truncated },
        },
    }))
}

mod files;
use files::{Direction, sync, transfer};

// ---- 在 GUI 里开 tab ----

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ShellParams {
    asset_id: String,
    #[serde(default)]
    window_id: Option<u64>,
}

fn shell(params: &Value, sink: &EventSink, hub: &RuntimeHub) -> Result<Value, ApiError> {
    let params: ShellParams = parse_params(params)?;
    require_ai_control("ssh.shell", &params.asset_id)?;
    // 开 tab 是 UI 线程上的毫秒级动作，和 `new-tab` 同量级；
    // 真正的 SSH 连接由新建的 pane 自己建立。
    let result = dispatch_runtime_command(
        RuntimeCommand::SshShell { window_id: params.window_id, asset_id: params.asset_id.clone() },
        sink,
        hub,
    )?;
    audit("ssh.shell", &params.asset_id, true, json!({ "window_id": result.get("window_id") }));
    Ok(result)
}

// ---- CLI ----

#[derive(Args, Debug)]
pub struct CliOptions {
    #[clap(subcommand)]
    pub command: CliCommand,
}

#[derive(Subcommand, Debug)]
pub enum CliCommand {
    /// List saved servers. Alias: hosts.
    #[clap(alias = "hosts")]
    Assets(AssetsOptions),
    /// Run one command on a saved server over a temporary connection.
    Exec(ExecOptions),
    /// Open an interactive SSH tab in the Pebrel window.
    Shell(ShellOptions),
    /// Copy a local file or directory to a server.
    Upload(UploadOptions),
    /// Copy a remote file or directory to this machine.
    Download(DownloadOptions),
    /// Mirror a local directory to a server directory.
    Sync(SyncOptions),
    /// Report whether a server has saved credentials. Never prints secrets.
    Credentials(CredentialsOptions),
}

#[derive(Args, Debug)]
pub struct AssetsOptions {
    #[clap(subcommand)]
    pub command: AssetsCommand,
}

#[derive(Subcommand, Debug)]
pub enum AssetsCommand {
    List(PlainOptions),
    Show(AssetIdOptions),
}

#[derive(Args, Debug)]
pub struct PlainOptions {
    #[clap(flatten)]
    pub output: ShortOutput,
}

#[derive(Args, Debug)]
pub struct AssetIdOptions {
    /// Asset id from `pebrel cli assets list`.
    pub asset_id: String,
    #[clap(flatten)]
    pub output: ShortOutput,
}

#[derive(Args, Debug)]
pub struct CredentialsOptions {
    #[clap(subcommand)]
    pub command: CredentialsCommand,
}

#[derive(Subcommand, Debug)]
pub enum CredentialsCommand {
    List(OptionalAssetOptions),
}

#[derive(Args, Debug)]
pub struct OptionalAssetOptions {
    #[clap(default_value = "")]
    pub asset_id: String,
    #[clap(flatten)]
    pub output: ShortOutput,
}

#[derive(Args, Debug)]
pub struct ExecOptions {
    /// Asset id from `pebrel cli assets list`.
    pub asset_id: String,
    /// One complete remote shell command. Omit it and pass --stdin instead.
    #[clap(default_value = "")]
    pub command: String,
    /// Read the command from stdin. Preferred in PowerShell, where quoting is lossy.
    #[clap(long)]
    pub stdin: bool,
    #[clap(long, default_value_t = DEFAULT_TIMEOUT_MS)]
    pub timeout_ms: u64,
    #[clap(long, default_value_t = DEFAULT_EXEC_OUTPUT_BYTES)]
    pub max_output_bytes: usize,
    #[clap(flatten)]
    pub output: ShortOutput,
}

#[derive(Args, Debug)]
pub struct ShellOptions {
    pub asset_id: String,
    #[clap(long)]
    pub window: Option<u64>,
    #[clap(flatten)]
    pub output: ShortOutput,
}

#[derive(Args, Debug)]
pub struct UploadOptions {
    pub asset_id: String,
    pub local_path: String,
    pub remote_path: String,
    #[clap(flatten)]
    pub output: ShortOutput,
}

#[derive(Args, Debug)]
pub struct DownloadOptions {
    pub asset_id: String,
    pub remote_path: String,
    pub local_path: String,
    #[clap(flatten)]
    pub output: ShortOutput,
}

#[derive(Args, Debug)]
pub struct SyncOptions {
    pub asset_id: String,
    pub local_path: String,
    pub remote_path: String,
    /// Remove remote files that do not exist locally. Refuses shallow targets.
    #[clap(long)]
    pub delete: bool,
    #[clap(flatten)]
    pub output: ShortOutput,
}

/// 先走驻留 GUI 的 runtime（复用其连接池与已认证会话），GUI 不在时直接在本
/// 进程执行。`pebrel` 这一个二进制里既有 CLI 又有完整 SSH 栈与凭据访问，读
/// `ssh_profiles.json` + 系统凭据库即可无 GUI 运行；只有 `shell`（要开窗口）
/// 必须依赖 runtime。
fn call<F>(
    pretty: bool,
    method: &str,
    mut params: Value,
    timeout: Duration,
    headless: F,
) -> Result<(), Box<dyn Error>>
where
    F: Fn(&Value) -> Result<Value, ApiError>,
{
    if let Some(path) = params.get("local_path").and_then(Value::as_str) {
        if path.is_empty() {
            return Err(super::cli::CliError::new("invalid_params", "local path is empty").into());
        }
        params["local_path"] = json!(std::path::absolute(path)?);
    }
    let response = response_or_headless(
        super::cli::request_once(method, params.clone(), timeout),
        &params,
        headless,
    )?;
    super::cli::print_response(&response, pretty)
}

fn response_or_headless<F>(
    response: Result<ApiResponse, Box<dyn Error>>,
    params: &Value,
    headless: F,
) -> Result<ApiResponse, Box<dyn Error>>
where
    F: Fn(&Value) -> Result<Value, ApiError>,
{
    match response {
        Ok(response) => Ok(response),
        Err(error)
            if error
                .downcast_ref::<super::cli::CliError>()
                .is_some_and(|error| error.code() == "runtime_unavailable") =>
        {
            let response = match headless(params) {
                Ok(value) => ApiResponse::success("cli", value),
                Err(error) => ApiResponse::failure("cli", error),
            };
            Ok(response)
        },
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssh_headless_fallback_only_runs_before_submission() {
        let calls = std::cell::Cell::new(0);
        let headless = |_: &Value| {
            calls.set(calls.get() + 1);
            Ok(json!({"executed":true}))
        };
        for code in ["submission_outcome_unknown", "runtime_no_response", "invalid_response"] {
            let response = Err(super::super::cli::CliError::new(code, "lost response").into());
            assert!(response_or_headless(response, &json!({}), &headless).is_err());
        }
        let error =
            Err(super::super::cli::CliError::new("runtime_unavailable", "not connected").into());
        assert!(response_or_headless(error, &json!({}), &headless).unwrap().ok);
        assert_eq!(calls.get(), 1);
        let denied = ApiResponse::failure("test", ApiError::new("ssh_control_disabled", "off"));
        assert!(!response_or_headless(Ok(denied), &json!({}), &headless).unwrap().ok);
        assert_eq!(calls.get(), 1);
    }
}

fn shell_only(
    pretty: bool,
    method: &str,
    params: Value,
    timeout: Duration,
) -> Result<(), Box<dyn Error>> {
    match super::cli::request_once(method, params, timeout) {
        Ok(response) => super::cli::print_response(&response, pretty),
        Err(error) => {
            let response = ApiResponse::failure(
                "cli",
                ApiError::new(
                    "runtime_unavailable",
                    format!("{error}; 启动 Pebrel 才能打开 SSH 会话标签"),
                ),
            );
            super::cli::print_response(&response, pretty)
        },
    }
}

const CLI_TIMEOUT: Duration = Duration::from_secs(120);

fn interactive_command(options: &ExecOptions) -> Result<String, Box<dyn Error>> {
    if !options.stdin {
        return Ok(options.command.clone());
    }
    let mut buffer = String::new();
    std::io::stdin().read_to_string(&mut buffer)?;
    Ok(buffer.trim_end_matches(['\r', '\n']).to_owned())
}

pub fn run_cli(options: CliOptions) -> Result<(), Box<dyn Error>> {
    match options.command {
        CliCommand::Assets(assets) => match assets.command {
            AssetsCommand::List(options) => {
                call(options.output.pretty, "ssh.assets.list", json!({}), CLI_TIMEOUT, |_| {
                    assets_list()
                })
            },
            AssetsCommand::Show(options) => call(
                options.output.pretty,
                "ssh.assets.show",
                json!({ "asset_id": options.asset_id }),
                CLI_TIMEOUT,
                assets_show,
            ),
        },
        CliCommand::Credentials(credentials) => match credentials.command {
            CredentialsCommand::List(options) => {
                let mut params = json!({});
                if !options.asset_id.is_empty() {
                    params["asset_id"] = json!(options.asset_id);
                }
                call(
                    options.output.pretty,
                    "ssh.credentials.list",
                    params,
                    CLI_TIMEOUT,
                    credentials_list,
                )
            },
        },
        CliCommand::Exec(options) => {
            let command = interactive_command(&options)?;
            call(
                options.output.pretty,
                "ssh.exec",
                json!({
                    "asset_id": options.asset_id,
                    "command": command,
                    "timeout_ms": options.timeout_ms,
                    "max_output_bytes": options.max_output_bytes,
                }),
                Duration::from_millis(options.timeout_ms).saturating_add(Duration::from_secs(10)),
                exec,
            )
        },
        CliCommand::Shell(options) => {
            let mut params = json!({ "asset_id": options.asset_id });
            if let Some(window) = options.window {
                params["window_id"] = json!(window);
            }
            shell_only(options.output.pretty, "ssh.shell", params, CLI_TIMEOUT)
        },
        CliCommand::Upload(options) => call(
            options.output.pretty,
            "ssh.upload",
            json!({
                "asset_id": options.asset_id,
                "local_path": options.local_path,
                "remote_path": options.remote_path,
            }),
            CLI_TIMEOUT,
            |params| transfer(params, Direction::Upload),
        ),
        CliCommand::Download(options) => call(
            options.output.pretty,
            "ssh.download",
            json!({
                "asset_id": options.asset_id,
                "remote_path": options.remote_path,
                "local_path": options.local_path,
            }),
            CLI_TIMEOUT,
            |params| transfer(params, Direction::Download),
        ),
        CliCommand::Sync(options) => call(
            options.output.pretty,
            "ssh.sync",
            json!({
                "asset_id": options.asset_id,
                "local_path": options.local_path,
                "remote_path": options.remote_path,
                "delete": options.delete,
            }),
            CLI_TIMEOUT,
            sync,
        ),
    }
}
