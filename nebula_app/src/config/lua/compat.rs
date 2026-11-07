//! Lua 5.4 standard-library behavior that Luau omits, for trusted configuration code.
//!
//! The previous runtime opened every safe Lua 5.4 library. Luau keeps most of the
//! language and library surface, but drops `io`, most of `os`, `warn`, `__pairs`
//! and a few `math`/`string` details. Restoring them here lets existing
//! configuration files load unchanged. Plugins never receive this layer.

use std::fs;
use std::process::Stdio;

use mlua::{Function, IntoLuaMulti, Lua, MultiValue, Table, Value, Variadic};

use super::io;

pub(super) fn install(lua: &Lua) -> mlua::Result<()> {
    io::install(lua)?;
    let globals = lua.globals();
    let os: Table = globals.get("os")?;
    os.raw_set(
        "getenv",
        lua.create_function(|_, name: String| {
            Ok(std::env::var_os(name).map(|value| value.to_string_lossy().into_owned()))
        })?,
    )?;
    os.raw_set(
        "execute",
        lua.create_function(|lua, command: Option<String>| {
            let Some(command) = command else {
                return true.into_lua_multi(lua);
            };
            match crate::platform::lua_host::shell_command(&command).stdin(Stdio::null()).status() {
                Ok(status) => io::exit_result(lua, status),
                Err(error) => (Value::Nil, io::io_message(&command, &error), io::errno(&error))
                    .into_lua_multi(lua),
            }
        })?,
    )?;
    os.raw_set(
        "remove",
        lua.create_function(|lua, path: String| {
            let result = match fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.is_dir() => fs::remove_dir(&path),
                _ => fs::remove_file(&path),
            };
            file_result(lua, &path, result)
        })?,
    )?;
    os.raw_set(
        "rename",
        lua.create_function(|lua, (from, to): (String, String)| {
            file_result(lua, &from, fs::rename(&from, &to))
        })?,
    )?;
    os.raw_set(
        "tmpname",
        lua.create_function(|_, ()| {
            let file = tempfile::Builder::new().prefix("lua_").tempfile().map_err(|error| {
                mlua::Error::runtime(format!("unable to generate a unique filename: {error}"))
            })?;
            let (_, path) = file.keep().map_err(|error| mlua::Error::runtime(error.to_string()))?;
            Ok(path.to_string_lossy().into_owned())
        })?,
    )?;
    os.raw_set(
        "exit",
        lua.create_function(|_, _: MultiValue| -> mlua::Result<()> {
            Err(mlua::Error::runtime("os.exit is not available while Pebrel loads configuration"))
        })?,
    )?;
    os.raw_set(
        "setlocale",
        lua.create_function(|_, (locale, _category): (Option<String>, Option<String>)| {
            Ok(matches!(locale.as_deref(), None | Some("") | Some("C")).then_some("C"))
        })?,
    )?;
    globals.raw_set(
        "warn",
        lua.create_function(|_, parts: Variadic<String>| {
            let message = parts.concat();
            // Lua 5.4 control messages (`@on`, `@off`) toggle output; logging is always on.
            if parts.len() != 1 || !message.starts_with('@') {
                log::warn!(target: "pebrel_lua", "{message}");
            }
            Ok(())
        })?,
    )?;
    let prelude: Function = lua.load(PRELUDE).set_name("=pebrel-compat").into_function()?;
    prelude.call(())
}

fn file_result(lua: &Lua, path: &str, result: std::io::Result<()>) -> mlua::Result<MultiValue> {
    match result {
        Ok(()) => true.into_lua_multi(lua),
        Err(error) => {
            (Value::Nil, io::io_message(path, &error), io::errno(&error)).into_lua_multi(lua)
        },
    }
}

/// Pure-Lua parts. Luau numbers are doubles, so `math.type` reports whole numbers
/// as integers: `math.type(1.0)` is `"integer"` here, unlike Lua 5.4.
const PRELUDE: &str = r#"
local getmetatable, rawget, type = getmetatable, rawget, type
local floor, rep = math.floor, string.rep

local rawpairs = pairs
function pairs(value)
    local metatable = getmetatable(value)
    local metamethod = type(metatable) == "table" and rawget(metatable, "__pairs")
    if metamethod then
        local iterator, state, initial = metamethod(value)
        return iterator, state, initial
    end
    return rawpairs(value)
end

math.maxinteger = 9223372036854775807
math.mininteger = -9223372036854775808

function math.type(value)
    if type(value) ~= "number" then
        return nil
    end
    if value == floor(value) and value >= -2 ^ 63 and value < 2 ^ 63 then
        return "integer"
    end
    return "float"
end

function math.tointeger(value)
    if math.type(value) == "integer" then
        return value
    end
    return nil
end

function math.ult(left, right)
    if left < 0 then left += 2 ^ 64 end
    if right < 0 then right += 2 ^ 64 end
    return left < right
end

function string.rep(text, count, separator)
    if separator == nil or separator == "" or count <= 1 then
        return rep(text, count)
    end
    return rep(text .. separator, count - 1) .. text
end
"#;
