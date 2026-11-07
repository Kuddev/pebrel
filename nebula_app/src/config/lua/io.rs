//! Lua 5.4's `io` library for configuration code, which Luau does not provide.
//!
//! Configuration is trusted local code and previously ran with the complete Lua 5.4
//! standard library. Reads are buffered whole files or whole command outputs, which
//! keeps `read`/`lines`/`seek` simple; both are bounded. Update modes (`r+`, `w+`,
//! `a+`) and input/output redirection are not provided and fail with a message.

use std::fs;
use std::io::{Read as _, Seek as _, SeekFrom, Write as _};
use std::path::Path;
use std::process::{ExitStatus, Stdio};

use crate::platform::lua_host;
use mlua::{
    AnyUserData, Function, IntoLuaMulti, Lua, MetaMethod, MultiValue, UserData, UserDataMethods,
    Value,
};

/// Upper bound for one buffered file or command output.
const MAX_BUFFER_BYTES: u64 = 64 * 1024 * 1024;

enum Handle {
    Buffer { data: Vec<u8>, position: usize, status: Option<ExitStatus> },
    Writer { file: fs::File, binary: bool },
    Stdin,
    Stdout,
    Stderr,
    Closed,
}

pub(super) struct LuaFile(Handle);

pub(super) fn install(lua: &Lua) -> mlua::Result<()> {
    let io = lua.create_table()?;
    let stdout = lua.create_userdata(LuaFile(Handle::Stdout))?;
    io.raw_set("stdin", lua.create_userdata(LuaFile(Handle::Stdin))?)?;
    io.raw_set("stdout", stdout.clone())?;
    io.raw_set("stderr", lua.create_userdata(LuaFile(Handle::Stderr))?)?;
    io.raw_set(
        "open",
        lua.create_function(|lua, (path, mode): (String, Option<String>)| {
            open(lua, &path, mode.as_deref().unwrap_or("r"))
        })?,
    )?;
    io.raw_set(
        "lines",
        lua.create_function(|lua, (path, formats): (Option<String>, MultiValue)| {
            let Some(path) = path else {
                return Err(mlua::Error::runtime("reading standard input is not available"));
            };
            let file = match read_file(&path, false) {
                Ok(data) => lua.create_userdata(LuaFile(Handle::Buffer {
                    data,
                    position: 0,
                    status: None,
                }))?,
                Err(error) => return Err(mlua::Error::runtime(io_message(&path, &error))),
            };
            lines(lua, file, formats, true)
        })?,
    )?;
    io.raw_set(
        "popen",
        lua.create_function(|lua, (command, mode): (String, Option<String>)| {
            if !matches!(mode.as_deref(), None | Some("r")) {
                return (Value::Nil, "io.popen supports only mode 'r' in Pebrel configuration")
                    .into_lua_multi(lua);
            }
            match capture(&command) {
                Ok((data, status)) => lua
                    .create_userdata(LuaFile(Handle::Buffer {
                        data: lua_host::text_mode_read(data),
                        position: 0,
                        status: Some(status),
                    }))?
                    .into_lua_multi(lua),
                Err(error) => {
                    (Value::Nil, io_message(&command, &error), errno(&error)).into_lua_multi(lua)
                },
            }
        })?,
    )?;
    let write_target = stdout.clone();
    io.raw_set(
        "write",
        lua.create_function(move |lua, values: MultiValue| {
            write(lua, write_target.clone(), values)
        })?,
    )?;
    io.raw_set("read", lua.create_function(|_, _: MultiValue| Ok(Value::Nil))?)?;
    io.raw_set(
        "close",
        lua.create_function(|lua, file: Option<AnyUserData>| match file {
            Some(file) => file.borrow_mut::<LuaFile>()?.close(lua),
            None => (Value::Nil, "cannot close standard file").into_lua_multi(lua),
        })?,
    )?;
    io.raw_set(
        "type",
        lua.create_function(|_, value: Value| {
            Ok(match value {
                Value::UserData(data) => match data.borrow::<LuaFile>() {
                    Ok(file) if matches!(file.0, Handle::Closed) => Some("closed file"),
                    Ok(_) => Some("file"),
                    Err(_) => None,
                },
                _ => None,
            })
        })?,
    )?;
    for (name, default) in [("input", "stdin"), ("output", "stdout")] {
        let current: AnyUserData = io.raw_get(default)?;
        io.raw_set(
            name,
            lua.create_function(move |_, replacement: Value| match replacement {
                Value::Nil => Ok(current.clone()),
                _ => Err(mlua::Error::runtime(format!(
                    "io.{name} redirection is not available in Pebrel configuration"
                ))),
            })?,
        )?;
    }
    lua.globals().raw_set("io", io)
}

fn open(lua: &Lua, path: &str, mode: &str) -> mlua::Result<MultiValue> {
    let binary = mode.contains('b');
    let base = mode.trim_end_matches('b');
    if !matches!(base, "r" | "w" | "a" | "r+" | "w+" | "a+") {
        return Err(mlua::Error::runtime(format!(
            "bad argument #2 to 'open' (invalid mode '{mode}')"
        )));
    }
    let handle = match base {
        "r" => {
            read_file(path, binary).map(|data| Handle::Buffer { data, position: 0, status: None })
        },
        "w" => fs::File::create(path).map(|file| Handle::Writer { file, binary }),
        "a" => fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)
            .map(|file| Handle::Writer { file, binary }),
        _ => {
            return (
                Value::Nil,
                format!("{path}: update modes are not available in Pebrel configuration"),
            )
                .into_lua_multi(lua);
        },
    };
    match handle {
        Ok(handle) => lua.create_userdata(LuaFile(handle))?.into_lua_multi(lua),
        Err(error) => (Value::Nil, io_message(path, &error), errno(&error)).into_lua_multi(lua),
    }
}

fn read_file(path: &str, binary: bool) -> std::io::Result<Vec<u8>> {
    let file = fs::File::open(Path::new(path))?;
    let mut data = Vec::new();
    file.take(MAX_BUFFER_BYTES + 1).read_to_end(&mut data)?;
    if data.len() as u64 > MAX_BUFFER_BYTES {
        return Err(std::io::Error::other("file exceeds the 64 MiB configuration read limit"));
    }
    Ok(if binary { data } else { lua_host::text_mode_read(data) })
}

fn capture(command: &str) -> std::io::Result<(Vec<u8>, ExitStatus)> {
    let mut child =
        lua_host::shell_command(command).stdin(Stdio::null()).stdout(Stdio::piped()).spawn()?;
    let mut data = Vec::new();
    let read = match child.stdout.take() {
        Some(stdout) => stdout.take(MAX_BUFFER_BYTES + 1).read_to_end(&mut data),
        None => Ok(0),
    };
    if read.is_err() || data.len() as u64 > MAX_BUFFER_BYTES {
        let _ = child.kill();
        let _ = child.wait();
        read?;
        return Err(std::io::Error::other("command output exceeds the 64 MiB limit"));
    }
    Ok((data, child.wait()?))
}

/// `(true|nil, "exit"|"signal", code)`, the Lua 5.4 result of `close` and `os.execute`.
pub(super) fn exit_result(lua: &Lua, status: ExitStatus) -> mlua::Result<MultiValue> {
    if let Some(signal) = lua_host::exit_signal(status) {
        return (Value::Nil, "signal", signal).into_lua_multi(lua);
    }
    let success = if status.success() { Value::Boolean(true) } else { Value::Nil };
    (success, "exit", status.code().unwrap_or(-1)).into_lua_multi(lua)
}

/// `path: message`, without Rust's ` (os error N)` suffix.
pub(super) fn io_message(subject: &str, error: &std::io::Error) -> String {
    let text = error.to_string();
    let text = match text.rfind(" (os error ") {
        Some(index) => &text[..index],
        None => &text,
    };
    format!("{subject}: {text}")
}

pub(super) fn errno(error: &std::io::Error) -> i32 {
    error.raw_os_error().unwrap_or(0)
}

fn lines(
    lua: &Lua,
    file: AnyUserData,
    formats: MultiValue,
    close_at_end: bool,
) -> mlua::Result<Function> {
    lua.create_function(move |lua, ()| {
        let mut handle = file.borrow_mut::<LuaFile>()?;
        let values = handle.read(lua, formats.clone())?;
        if close_at_end && values.front().is_none_or(Value::is_nil) {
            handle.close(lua)?;
        }
        Ok(values)
    })
}

fn write(lua: &Lua, file: AnyUserData, values: MultiValue) -> mlua::Result<MultiValue> {
    let mut text = Vec::new();
    for (index, value) in values.into_iter().enumerate() {
        match value {
            Value::String(value) => text.extend_from_slice(&value.as_bytes()),
            Value::Integer(value) => text.extend_from_slice(value.to_string().as_bytes()),
            Value::Number(value) => text.extend_from_slice(value.to_string().as_bytes()),
            other => {
                return Err(mlua::Error::runtime(format!(
                    "bad argument #{} to 'write' (string expected, got {})",
                    index + 1,
                    other.type_name()
                )));
            },
        }
    }
    let result = {
        let mut handle = file.borrow_mut::<LuaFile>()?;
        match &mut handle.0 {
            Handle::Writer { file, binary } => {
                if !*binary {
                    text = lua_host::text_mode_write(text);
                }
                file.write_all(&text)
            },
            Handle::Stdout => std::io::stdout().write_all(&text),
            Handle::Stderr => std::io::stderr().write_all(&text),
            Handle::Closed => return Err(mlua::Error::runtime("attempt to use a closed file")),
            Handle::Buffer { .. } | Handle::Stdin => {
                return (Value::Nil, "Bad file descriptor", 9).into_lua_multi(lua);
            },
        }
    };
    match result {
        Ok(()) => file.into_lua_multi(lua),
        Err(error) => (Value::Nil, error.to_string(), errno(&error)).into_lua_multi(lua),
    }
}

impl LuaFile {
    fn read(&mut self, lua: &Lua, formats: MultiValue) -> mlua::Result<MultiValue> {
        let (data, position) = match &mut self.0 {
            Handle::Buffer { data, position, .. } => (data, position),
            Handle::Closed => return Err(mlua::Error::runtime("attempt to use a closed file")),
            Handle::Stdin => return Value::Nil.into_lua_multi(lua),
            _ => return (Value::Nil, "Bad file descriptor", 9).into_lua_multi(lua),
        };
        let formats: Vec<Value> = if formats.is_empty() {
            vec![Value::String(lua.create_string("l")?)]
        } else {
            formats.into_iter().collect()
        };
        let mut results = Vec::with_capacity(formats.len());
        for format in formats {
            let value = match format {
                Value::Integer(count) => read_count(lua, data, position, count.max(0) as usize)?,
                Value::Number(count) => read_count(lua, data, position, count.max(0.0) as usize)?,
                Value::String(format) => {
                    let format = format.to_str()?.to_owned();
                    match format.trim_start_matches('*').chars().next() {
                        Some('a') => {
                            let rest = lua.create_string(&data[*position..])?;
                            *position = data.len();
                            Value::String(rest)
                        },
                        Some('l') => read_line(lua, data, position, false)?,
                        Some('L') => read_line(lua, data, position, true)?,
                        Some('n') => read_number(lua, data, position)?,
                        _ => {
                            return Err(mlua::Error::runtime(
                                "bad argument #1 to 'read' (invalid format)",
                            ));
                        },
                    }
                },
                _ => {
                    return Err(mlua::Error::runtime("bad argument #1 to 'read' (invalid format)"));
                },
            };
            let stop = value.is_nil();
            results.push(value);
            if stop {
                break;
            }
        }
        Ok(MultiValue::from_vec(results))
    }

    fn close(&mut self, lua: &Lua) -> mlua::Result<MultiValue> {
        match std::mem::replace(&mut self.0, Handle::Closed) {
            Handle::Closed => Err(mlua::Error::runtime("attempt to use a closed file")),
            handle @ (Handle::Stdin | Handle::Stdout | Handle::Stderr) => {
                self.0 = handle;
                (Value::Nil, "cannot close standard file").into_lua_multi(lua)
            },
            Handle::Buffer { status: Some(status), .. } => exit_result(lua, status),
            Handle::Writer { mut file, .. } => match file.flush() {
                Ok(()) => true.into_lua_multi(lua),
                Err(error) => (Value::Nil, error.to_string(), errno(&error)).into_lua_multi(lua),
            },
            Handle::Buffer { .. } => true.into_lua_multi(lua),
        }
    }

    fn seek(&mut self, lua: &Lua, whence: &str, offset: i64) -> mlua::Result<MultiValue> {
        let target = match whence {
            "set" => SeekFrom::Start(offset.max(0) as u64),
            "cur" => SeekFrom::Current(offset),
            "end" => SeekFrom::End(offset),
            _ => {
                return Err(mlua::Error::runtime(format!(
                    "bad argument #1 to 'seek' (invalid option '{whence}')"
                )));
            },
        };
        match &mut self.0 {
            Handle::Buffer { data, position, .. } => {
                let base = match target {
                    SeekFrom::Start(_) => 0,
                    SeekFrom::Current(_) => *position as i64,
                    SeekFrom::End(_) => data.len() as i64,
                };
                let next = base + if whence == "set" { offset.max(0) } else { offset };
                if next < 0 {
                    return (Value::Nil, "Invalid argument", 22).into_lua_multi(lua);
                }
                *position = (next as usize).min(data.len());
                (next as f64).into_lua_multi(lua)
            },
            Handle::Writer { file, .. } => match file.seek(target) {
                Ok(position) => (position as f64).into_lua_multi(lua),
                Err(error) => (Value::Nil, error.to_string(), errno(&error)).into_lua_multi(lua),
            },
            Handle::Closed => Err(mlua::Error::runtime("attempt to use a closed file")),
            _ => (Value::Nil, "Illegal seek", 29).into_lua_multi(lua),
        }
    }
}

fn read_count(lua: &Lua, data: &[u8], position: &mut usize, count: usize) -> mlua::Result<Value> {
    // Lua 5.4: `read(0)` is "" before end of file; every count reads nil at the end.
    if *position >= data.len() {
        return Ok(Value::Nil);
    }
    let end = position.saturating_add(count).min(data.len());
    let text = lua.create_string(&data[*position..end])?;
    *position = end;
    Ok(Value::String(text))
}

fn read_line(lua: &Lua, data: &[u8], position: &mut usize, keep: bool) -> mlua::Result<Value> {
    if *position >= data.len() {
        return Ok(Value::Nil);
    }
    let rest = &data[*position..];
    let (line, consumed) = match rest.iter().position(|byte| *byte == b'\n') {
        Some(index) => (&rest[..if keep { index + 1 } else { index }], index + 1),
        None => (rest, rest.len()),
    };
    let line = lua.create_string(line)?;
    *position += consumed;
    Ok(Value::String(line))
}

/// Reads the longest numeral prefix and converts it with Luau's own `tonumber`.
fn read_number(lua: &Lua, data: &[u8], position: &mut usize) -> mlua::Result<Value> {
    while data.get(*position).is_some_and(u8::is_ascii_whitespace) {
        *position += 1;
    }
    let start = *position;
    while data.get(*position).is_some_and(|byte| {
        byte.is_ascii_hexdigit() || matches!(byte, b'x' | b'X' | b'.' | b'+' | b'-' | b'p' | b'P')
    }) && *position - start < 200
    {
        *position += 1;
    }
    let tonumber: Function = lua.globals().get("tonumber")?;
    tonumber.call(lua.create_string(&data[start..*position])?)
}

impl UserData for LuaFile {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method_mut("read", |lua, this, formats: MultiValue| this.read(lua, formats));
        methods.add_function("lines", |lua, (file, formats): (AnyUserData, MultiValue)| {
            lines(lua, file, formats, false)
        });
        methods.add_function("write", |lua, (file, values): (AnyUserData, MultiValue)| {
            write(lua, file, values)
        });
        methods.add_method_mut("close", |lua, this, ()| this.close(lua));
        methods.add_method_mut(
            "seek",
            |lua, this, (whence, offset): (Option<String>, Option<i64>)| {
                this.seek(lua, whence.as_deref().unwrap_or("cur"), offset.unwrap_or(0))
            },
        );
        methods.add_method_mut("flush", |lua, this, ()| match &mut this.0 {
            Handle::Writer { file, .. } => match file.flush() {
                Ok(()) => true.into_lua_multi(lua),
                Err(error) => (Value::Nil, error.to_string(), errno(&error)).into_lua_multi(lua),
            },
            _ => true.into_lua_multi(lua),
        });
        methods.add_method("setvbuf", |_, _, _: MultiValue| Ok(true));
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(if matches!(this.0, Handle::Closed) {
                "file (closed)".to_owned()
            } else {
                format!("file ({:p})", this)
            })
        });
    }
}
