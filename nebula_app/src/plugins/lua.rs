//! 托管协程把宿主调用让出到 Rust；读取 Runtime 时不持有 Luau 的执行栈。

use std::time::Duration;

use mlua::{
    ChunkMode, Function, Lua, LuaSerdeExt, MultiValue, StdLib, Table, ThreadStatus,
    Value as LuaValue,
};
use serde_json::Value;

use super::{
    Error, Execution, MAX_LUA_MEMORY, MAX_LUAU_INTERRUPTS, MAX_RUNTIME_CALLS, Package, Result,
    RuntimeCall, json,
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
    vm: crate::luau::Budget,
    calls: usize,
}

impl Budget {
    fn remaining(&self) -> Result<Duration> {
        self.vm.remaining().map_err(limit_error)
    }
}

pub(super) fn execute(
    package: &Package,
    handler: &str,
    arguments: Value,
    timeout: Duration,
    call: &mut RuntimeCall<'_>,
) -> Result<Execution> {
    let mut budget =
        Budget { vm: crate::luau::Budget::new(timeout, MAX_LUAU_INTERRUPTS), calls: 0 };
    let source = package.source()?;
    budget.remaining()?;
    #[cfg(test)]
    VM_CREATIONS.with(|count| count.set(count.get() + 1));
    let lua = budget
        .vm
        .vm(
            StdLib::TABLE
                | StdLib::STRING
                | StdLib::MATH
                | StdLib::UTF8
                | StdLib::COROUTINE
                | StdLib::BIT
                | StdLib::BUFFER
                | StdLib::VECTOR,
            MAX_LUA_MEMORY,
        )
        .map_err(lua_error)?;
    let request: Function = budget
        .vm
        .run(|| {
            lua.load(
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
        })
        .map_err(lua_error)?;
    let globals = lua.globals();
    // 不留绕过宿主的文件入口，也不让非托管 coroutine 绕过指令分片。
    for name in [
        "coroutine",
        "dofile",
        "loadfile",
        "load",
        "loadstring",
        "require",
        "print",
        "warn",
        "collectgarbage",
        "getfenv",
        "setfenv",
        "debug",
    ] {
        globals.raw_set(name, LuaValue::Nil).map_err(lua_error)?;
    }
    lua.sandbox(true).map_err(lua_error)?;
    let entry = lua
        .load(source.strip_prefix('\u{feff}').unwrap_or(&source))
        .set_mode(ChunkMode::Text)
        .set_name(package.manifest.entry.as_deref().unwrap_or("plugin"))
        .into_function()
        .map_err(lua_error)?;
    let exports = drive(&lua, entry, MultiValue::new(), package, &mut budget, call)?;
    let LuaValue::Table(exports) = exports else {
        return Err(Error::new(
            "invalid_luau_entry",
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
        engine: "luau",
        lua_memory_bytes: lua.used_memory(),
        interrupt_count: budget.vm.interrupt_count(),
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
    loop {
        budget.remaining()?;
        let values: MultiValue = budget.vm.run(|| thread.resume(args)).map_err(lua_error)?;
        budget.remaining()?;
        if thread.status() != ThreadStatus::Resumable {
            if values.len() > 1 {
                return Err(Error::new(
                    "invalid_luau_result",
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

fn limit_error(limit: crate::luau::Limit) -> Error {
    let code = match limit {
        crate::luau::Limit::Interrupts => "luau_interrupt_limit",
        crate::luau::Limit::Deadline => "plugin_timeout",
    };
    Error::new(code, limit)
}

fn lua_error(error: mlua::Error) -> Error {
    if let Some(limit) =
        error.chain().find_map(|source| source.downcast_ref::<crate::luau::Limit>())
    {
        return limit_error(*limit);
    }
    if let Some(original) = error.chain().find_map(|source| source.downcast_ref::<Error>()) {
        return original.clone();
    }
    let code = if matches!(error, mlua::Error::MemoryError(_)) {
        "luau_memory_limit"
    } else {
        "luau_error"
    };
    Error::new(code, error)
}
