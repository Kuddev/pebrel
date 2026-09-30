//! 托管协程把宿主调用让出到 Rust；读取 Runtime 时不持有 Lua 的执行栈。

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use mlua::{
    ChunkMode, Function, HookTriggers, Lua, LuaOptions, LuaSerdeExt, MultiValue, StdLib, Table,
    ThreadStatus, Value as LuaValue, VmState,
};
use serde_json::Value;

use super::{
    Error, Execution, INSTRUCTION_QUANTUM, MAX_INSTRUCTIONS, MAX_LUA_MEMORY, MAX_RUNTIME_CALLS,
    Package, Result, RuntimeCall, json,
};

const REQUEST: &str = "pebrel.runtime.request";

#[cfg(test)]
thread_local! {
    static VM_CREATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(super) fn vm_creation_count() -> usize {
    VM_CREATIONS.with(std::cell::Cell::get)
}

struct Budget {
    deadline: Instant,
    ticks: Arc<AtomicUsize>,
    calls: usize,
}

impl Budget {
    fn remaining(&self) -> Result<Duration> {
        if self.ticks.load(Ordering::Relaxed) >= MAX_INSTRUCTIONS {
            return Err(Error::new("lua_instruction_limit", "Lua instruction budget exhausted"));
        }
        self.deadline
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| Error::new("plugin_timeout", "plugin execution deadline reached"))
    }
}

pub(super) fn execute(
    package: &Package,
    handler: &str,
    arguments: Value,
    timeout: Duration,
    call: &mut RuntimeCall<'_>,
) -> Result<Execution> {
    let mut budget = Budget {
        deadline: Instant::now() + timeout,
        ticks: Arc::new(AtomicUsize::new(0)),
        calls: 0,
    };
    let source = package.source()?;
    budget.remaining()?;
    #[cfg(test)]
    VM_CREATIONS.with(|count| count.set(count.get() + 1));
    let lua = Lua::new_with(
        StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8 | StdLib::COROUTINE,
        LuaOptions::default(),
    )
    .map_err(lua_error)?;
    lua.set_memory_limit(MAX_LUA_MEMORY).map_err(lua_error)?;
    let request: Function = lua
        .load(
            r#"
        local yield, raise = coroutine.yield, error
        return function(method, params)
            local ok, result = yield('pebrel.runtime.request', method, params or {})
            if not ok then raise(result, 0) end
            return result
        end
    "#,
        )
        .eval()
        .map_err(lua_error)?;
    let globals = lua.globals();
    // 不留绕过宿主的文件入口，也不让非托管 coroutine 绕过指令分片。
    for name in
        ["coroutine", "dofile", "loadfile", "load", "require", "print", "warn", "collectgarbage"]
    {
        globals.raw_set(name, LuaValue::Nil).map_err(lua_error)?;
    }
    let entry = lua
        .load(source.strip_prefix('\u{feff}').unwrap_or(&source))
        .set_mode(ChunkMode::Text)
        .set_name(package.manifest.entry.as_deref().unwrap_or("plugin"))
        .into_function()
        .map_err(lua_error)?;
    let exports = drive(&lua, entry, MultiValue::new(), package, &mut budget, call)?;
    let LuaValue::Table(exports) = exports else {
        return Err(Error::new(
            "invalid_lua_entry",
            "entry must return a table of command functions",
        ));
    };
    // raw_get 避免未托管的 __index 在查找 handler 时执行插件代码。
    let function: Function = exports.raw_get(handler).map_err(lua_error)?;
    let runtime = lua.create_table().map_err(lua_error)?;
    runtime.raw_set("call", request).map_err(lua_error)?;
    let context = lua.create_table().map_err(lua_error)?;
    context.raw_set("runtime", runtime).map_err(lua_error)?;
    context.raw_set("null", lua.null()).map_err(lua_error)?;
    context
        .raw_set(
            "array",
            lua.create_function(|lua, values: Option<Table>| {
                let values = match values {
                    Some(values) => values,
                    None => lua.create_table()?,
                };
                values.set_metatable(Some(lua.array_metatable()))?;
                Ok(values)
            })
            .map_err(lua_error)?,
        )
        .map_err(lua_error)?;
    let arguments = lua.to_value(&arguments).map_err(lua_error)?;
    let args = MultiValue::from_vec(vec![LuaValue::Table(context), arguments]);
    let result = drive(&lua, function, args, package, &mut budget, call)?;
    let result = json::from_lua(&lua, result)?;
    Ok(Execution {
        result,
        engine: "lua",
        lua_memory_bytes: lua.used_memory(),
        instruction_ticks: budget.ticks.load(Ordering::Relaxed),
    })
    // 命令作用域结束即释放 VM；这里没有全局缓存或常驻插件对象。
}

fn drive(
    lua: &Lua,
    function: Function,
    mut args: MultiValue,
    package: &Package,
    budget: &mut Budget,
    call: &mut RuntimeCall<'_>,
) -> Result<LuaValue> {
    let thread = lua.create_thread(function).map_err(lua_error)?;
    let ticks = budget.ticks.clone();
    thread
        .set_hook(
            HookTriggers::new().every_nth_instruction(INSTRUCTION_QUANTUM as u32),
            move |_, _| {
                ticks.fetch_add(INSTRUCTION_QUANTUM, Ordering::Relaxed);
                // 使用 yield 而非可被 pcall 吞掉的异常，让预算裁定回到 Rust。
                Ok(VmState::Yield)
            },
        )
        .map_err(lua_error)?;
    loop {
        budget.remaining()?;
        let values: MultiValue = thread.resume(args).map_err(lua_error)?;
        budget.remaining()?;
        if thread.status() != ThreadStatus::Resumable {
            if values.len() > 1 {
                return Err(Error::new(
                    "invalid_lua_result",
                    "commands return one JSON-compatible value",
                ));
            }
            return Ok(values.into_iter().next().unwrap_or(LuaValue::Nil));
        }
        args = MultiValue::new();
        if values.is_empty() {
            continue;
        }
        let mut values = values.into_iter();
        let valid_tag = matches!(values.next(), Some(LuaValue::String(tag)) if tag.to_str().is_ok_and(|tag| tag == REQUEST));
        let Some(LuaValue::String(method)) = values.next() else {
            return Err(Error::new("invalid_runtime_request", "runtime method must be a string"));
        };
        let method = method.to_str().map_err(lua_error)?;
        let params =
            values.next().ok_or_else(|| Error::new("invalid_runtime_request", "missing params"))?;
        if !valid_tag || values.next().is_some() {
            return Err(Error::new("invalid_runtime_request", "unexpected coroutine yield"));
        }
        package.allow_method(&method)?;
        budget.calls += 1;
        if budget.calls > MAX_RUNTIME_CALLS {
            return Err(Error::new(
                "runtime_call_limit",
                "at most 16 runtime calls per invocation",
            ));
        }
        let params = json::from_lua(lua, params)?;
        if !params.is_object() {
            return Err(Error::new("invalid_runtime_request", "runtime params must be an object"));
        }
        let result = call(&method, params, budget.remaining()?);
        budget.remaining()?;
        args = match result {
            Ok(value) => {
                json::validate(&value)?;
                MultiValue::from_vec(vec![
                    LuaValue::Boolean(true),
                    lua.to_value(&value).map_err(lua_error)?,
                ])
            },
            Err(error) => MultiValue::from_vec(vec![
                LuaValue::Boolean(false),
                LuaValue::Error(Box::new(mlua::Error::external(error))),
            ]),
        };
    }
}

fn lua_error(error: mlua::Error) -> Error {
    if let Some(original) = error.chain().find_map(|source| source.downcast_ref::<Error>()) {
        return original.clone();
    }
    let code =
        if matches!(error, mlua::Error::MemoryError(_)) { "lua_memory_limit" } else { "lua_error" };
    Error::new(code, error)
}
