//! 插件命令的冷路径入口；普通 GUI 启动不构造插件实例、VM 或工作线程。

pub(crate) mod cli;
mod json;
mod lua;
mod package;

use std::fmt;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use package::Package;

const MAX_JSON_BYTES: usize = 64 * 1024;
const MAX_JSON_NODES: usize = 4096;
const MAX_JSON_DEPTH: usize = 32;
const MAX_LUA_MEMORY: usize = 8 * 1024 * 1024;
const MAX_INSTRUCTIONS: usize = 1_000_000;
const INSTRUCTION_QUANTUM: usize = 10_000;
const MAX_RUNTIME_CALLS: usize = 16;

#[derive(Clone, Debug, Serialize)]
struct Error {
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

type Result<T> = std::result::Result<T, Error>;
type RuntimeCall<'a> = dyn FnMut(&str, Value, Duration) -> Result<Value> + 'a;

#[derive(Debug, Serialize)]
struct Execution {
    pub result: Value,
    pub engine: &'static str,
    /// 只报告本次 Lua VM 的当前分配量，不将其冒充整个进程的 RSS 或峰值。
    pub lua_memory_bytes: usize,
    pub instruction_ticks: usize,
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
        return Ok(Execution {
            result,
            engine: "native",
            lua_memory_bytes: 0,
            instruction_ticks: 0,
        });
    }

    lua::execute(
        package,
        command.handler.as_deref().expect("validated Lua command"),
        parameters,
        timeout,
        call,
    )
}

#[cfg(test)]
mod tests;
