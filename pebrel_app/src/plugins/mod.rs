//! 插件命令的冷路径入口；普通 GUI 启动不构造插件实例、VM 或工作线程。

mod boundary;
pub(crate) mod cli;
pub(crate) mod install;
mod json;
mod lua;
mod package;
#[cfg(feature = "gpui-shell")]
pub(crate) mod themes;
#[cfg(not(feature = "gpui-shell"))]
mod themes {
    //! 没有主题库的构建不接受需要主题授权的插件。
    use super::{Error, Result};

    pub(super) fn check_bundled(_: &std::path::Path, themes: &[String]) -> Result<()> {
        if themes.is_empty() { Ok(()) } else { Err(unavailable()) }
    }

    pub(super) fn finish_for_user(
        _: serde_json::Value,
        _: &super::Package,
        _: &str,
        _: Option<&[super::package::Grant]>,
    ) -> Result<serde_json::Value> {
        Err(unavailable())
    }

    pub(super) fn install_bundled_for_user(
        _: &super::install::Library,
        _: &str,
    ) -> Result<serde_json::Value> {
        Ok(serde_json::json!([]))
    }

    fn unavailable() -> Error {
        Error::new("theme_library_unavailable", "this build has no theme library")
    }
}

use std::fmt;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

pub(crate) use package::Grant;
use package::Package;

const MAX_JSON_BYTES: usize = 64 * 1024;
const MAX_JSON_NODES: usize = 4096;
const MAX_JSON_DEPTH: usize = 32;
const MAX_LUA_MEMORY: usize = 8 * 1024 * 1024;
const MAX_LUAU_INTERRUPTS: usize = 100_000;
const MAX_RUNTIME_CALLS: usize = 16;

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Error {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

impl Error {
    pub(super) fn new(code: impl Into<String>, message: impl fmt::Display) -> Self {
        Self {
            code: code.into(),
            message: message.to_string().chars().take(2048).collect(),
            details: None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(output, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for Error {}

pub(crate) type Result<T> = std::result::Result<T, Error>;
type RuntimeCall<'a> = dyn FnMut(&str, Value, Duration) -> Result<Value> + 'a;

#[derive(Debug, Serialize)]
struct Execution {
    pub result: Value,
    pub engine: &'static str,
    /// 只报告本次 Luau VM 的当前分配量，不将其冒充整个进程的 RSS 或峰值。
    pub lua_memory_bytes: usize,
    pub interrupt_count: usize,
}

fn execute(
    package: &Package,
    command_id: &str,
    arguments: Value,
    timeout: Duration,
    call: &mut RuntimeCall<'_>,
) -> Result<Execution> {
    if timeout.is_zero() || timeout > Duration::from_secs(30) {
        return Err(Error::new("invalid_timeout", "timeout must be within 1..=30000 ms"));
    }
    let command = package
        .manifest
        .commands
        .iter()
        .find(|command| command.id == command_id)
        .ok_or_else(|| Error::new("command_not_found", command_id))?;
    let mut parameters = command.params.as_object().cloned().expect("validated command params");
    let Value::Object(arguments) = arguments else {
        return Err(Error::new("invalid_arguments", "command arguments must be an object"));
    };
    parameters.extend(arguments);
    let parameters = Value::Object(parameters);
    json::validate(&parameters)?;

    if let Some(method) = &command.method {
        package.allow_method(method)?;
        let result = call(method, parameters, timeout)?;
        json::validate(&result)?;
        // 资源命令在此返回，路径上没有 Lua::new、脚本读取或调度器初始化。
        return Ok(Execution { result, engine: "native", lua_memory_bytes: 0, interrupt_count: 0 });
    }

    lua::execute(
        package,
        command.handler.as_deref().expect("validated Luau command"),
        parameters,
        timeout,
        call,
    )
}

/// The one-shot Runtime transport shared by the CLI and the settings page.
/// It blocks, so GUI callers run it on a background executor.
fn runtime_call(method: &str, params: Value, timeout: Duration) -> Result<Value> {
    let response =
        crate::runtime_api::request_once_bounded(method, params, timeout).map_err(runtime_error)?;
    if !response.ok {
        let error = response
            .error
            .ok_or_else(|| Error::new("invalid_runtime_response", "missing error"))?;
        return Err(runtime_error(error));
    }
    response.result.ok_or_else(|| Error::new("invalid_runtime_response", "missing result"))
}

fn runtime_error(error: crate::runtime_api::ApiError) -> Error {
    let mut result = Error::new(error.code, error.message);
    // 部分完成与清理回执属于 Runtime 的结果，插件适配层不得将它们折叠成字符串。
    result.details = error.details;
    result
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod install_tests;
