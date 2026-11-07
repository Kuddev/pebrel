//! Lua 5.4 `package`, `require`, `dofile`, `loadfile` and `load` for configuration.
//!
//! Luau ships none of these. Configuration files written for the previous Lua 5.4
//! runtime keep their module names, `package.path` edits, preload entries and custom
//! searchers. Files found through any of these paths join the reload watch list.
//! Luau-style `require './name'` additionally resolves next to the requiring file.

use std::fs;
use std::path::{Path, PathBuf};

use mlua::{Function, Lua, MultiValue, Table, Value};

use super::runtime::WatchList;
use super::syntax;

const DIRECTORY_SEPARATOR: &str = if cfg!(windows) { "\\" } else { "/" };
const MODULE_SUFFIXES: [&str; 4] = [".luau", ".lua", "/init.luau", "/init.lua"];

pub(super) fn install(
    lua: &Lua,
    config_dir: &Path,
    api: Table,
    watch: WatchList,
) -> mlua::Result<()> {
    let globals = lua.globals();
    let package = lua.create_table()?;
    let loaded = lua.create_table()?;
    for name in ["string", "table", "math", "os", "io", "coroutine", "utf8", "bit32", "debug"] {
        if let Ok(library @ Value::Table(_)) = globals.raw_get::<Value>(name) {
            loaded.raw_set(name, library)?;
        }
    }
    loaded.raw_set("_G", globals.clone())?;
    loaded.raw_set("pebrel", api.clone())?;
    loaded.raw_set("nebula", api)?;
    package.raw_set("loaded", loaded)?;
    package.raw_set("preload", lua.create_table()?)?;
    package.raw_set("path", default_path(config_dir))?;
    package.raw_set("cpath", "")?;
    package.raw_set("config", format!("{DIRECTORY_SEPARATOR}\n;\n?\n!\n-\n"))?;
    package.raw_set(
        "searchpath",
        lua.create_function(
            |_,
             (name, path, separator, replacement): (
                String,
                String,
                Option<String>,
                Option<String>,
            )| {
                Ok(match search_path(&name, &path, separator.as_deref(), replacement.as_deref()) {
                    Ok(found) => (Some(path_text(&found)), None),
                    Err(message) => (None, Some(message)),
                })
            },
        )?,
    )?;

    let file_watch = watch.clone();
    let load_module = lua.create_function(move |lua, (path, name): (String, String)| {
        let path = PathBuf::from(path);
        file_watch.add(&path)?;
        load_path(lua, &path).map_err(|error| {
            mlua::Error::runtime(format!(
                "error loading module '{name}' from file '{}':\n\t{error}",
                path.display()
            ))
        })
    })?;
    let relative_dir = config_dir.to_owned();
    let resolve_relative =
        lua.create_function(move |_, (caller, name): (Option<String>, String)| {
            let base = caller
                .map(PathBuf::from)
                .filter(|caller| caller.is_file())
                .and_then(|caller| caller.parent().map(Path::to_owned))
                .unwrap_or_else(|| relative_dir.clone());
            let stem = base.join(name.trim_end_matches(['/', '\\']));
            let mut tried = String::new();
            for suffix in MODULE_SUFFIXES {
                let candidate = PathBuf::from(format!("{}{suffix}", stem.display()));
                if candidate.is_file() {
                    let candidate = candidate.canonicalize().unwrap_or(candidate);
                    return Ok((Some(path_text(&candidate)), None));
                }
                tried.push_str(&format!("\n\tno file '{}'", candidate.display()));
            }
            Ok((None, Some(tried)))
        })?;

    let file_watch = watch;
    let load_file = lua.create_function(
        move |lua, (path, mode, environment): (Option<String>, Option<String>, Option<Table>)| {
            let Some(path) = path else {
                return Ok((None, Some("loading from standard input is not available".to_owned())));
            };
            if mode.as_deref() == Some("b") {
                return Ok((None, Some("attempt to load a text chunk (mode is 'b')".to_owned())));
            }
            let path = PathBuf::from(path);
            file_watch.add(&path)?;
            match load_path(lua, &path) {
                Ok(function) => {
                    if let Some(environment) = environment {
                        set_environment(lua, &function, environment)?;
                    }
                    Ok((Some(function), None))
                },
                Err(error) => Ok((None, Some(error.to_string()))),
            }
        },
    )?;
    let load_text = lua.create_function(
        |lua,
         (chunk, name, mode, environment): (
            Value,
            Option<String>,
            Option<String>,
            Option<Table>,
        )| {
            let (source, default_name) = match chunk {
                Value::String(text) => (text.as_bytes().to_vec(), None),
                Value::Function(reader) => {
                    let mut source = Vec::new();
                    loop {
                        match reader.call::<Value>(())? {
                            Value::String(piece) if !piece.as_bytes().is_empty() => {
                                source.extend_from_slice(&piece.as_bytes())
                            },
                            Value::Nil | Value::String(_) => break,
                            _ => {
                                return Ok((
                                    None,
                                    Some("reader function must return a string".to_owned()),
                                ));
                            },
                        }
                    }
                    (source, Some("=(load)".to_owned()))
                },
                other => {
                    return Err(mlua::Error::runtime(format!(
                        "bad argument #1 to 'load' (string expected, got {})",
                        other.type_name()
                    )));
                },
            };
            if mode.as_deref() == Some("b") {
                return Ok((None, Some("attempt to load a text chunk (mode is 'b')".to_owned())));
            }
            let source = String::from_utf8_lossy(&source).into_owned();
            let name = name.or(default_name).unwrap_or_else(|| source.clone());
            let prepared = syntax::prepare(&source);
            let mut chunk = lua.load(prepared.as_ref()).set_name(name);
            if let Some(environment) = environment {
                chunk = chunk.set_environment(environment);
            }
            Ok(match chunk.into_function() {
                Ok(function) => (Some(function), None),
                Err(error) => (None, Some(error.to_string())),
            })
        },
    )?;

    let prelude: Function = lua.load(PRELUDE).set_name("=pebrel-package").into_function()?;
    prelude.call::<()>((package, load_module, resolve_relative, load_file, load_text))
}

/// Compiles one file as a chunk named after its path, as `luaL_loadfile` would.
fn load_path(lua: &Lua, path: &Path) -> mlua::Result<Function> {
    let source = fs::read_to_string(path).map_err(|error| {
        mlua::Error::runtime(format!("cannot open {}: {error}", path.display()))
    })?;
    let source = source.strip_prefix('\u{feff}').unwrap_or(&source);
    let prepared = syntax::prepare(source);
    lua.load(prepared.as_ref()).set_name(format!("@{}", path.display())).into_function().map_err(
        |error| match syntax::hint_block(&syntax::migration_hints(source)) {
            Some(hints) => mlua::Error::runtime(format!("{error}\n{hints}")),
            None => error,
        },
    )
}

fn set_environment(lua: &Lua, function: &Function, environment: Table) -> mlua::Result<()> {
    let setfenv: Function = lua.globals().get("setfenv")?;
    setfenv.call::<()>((function.clone(), environment))
}

/// `package.searchpath` from Lua 5.4: the error lists every candidate tried.
fn search_path(
    name: &str,
    path: &str,
    separator: Option<&str>,
    replacement: Option<&str>,
) -> Result<PathBuf, String> {
    let separator = separator.unwrap_or(".");
    let name = if separator.is_empty() {
        name.to_owned()
    } else {
        name.replace(separator, replacement.unwrap_or(DIRECTORY_SEPARATOR))
    };
    let mut tried = Vec::new();
    for template in path.split(';').filter(|template| !template.is_empty()) {
        let candidate = template.replace('?', &name);
        if fs::File::open(&candidate).is_ok_and(|file| file.metadata().is_ok_and(|m| m.is_file())) {
            return Ok(PathBuf::from(candidate));
        }
        tried.push(format!("no file '{candidate}'"));
    }
    Err(tried.join("\n\t"))
}

/// The configuration directory first, then Lua 5.4's `LUA_PATH` or built-in default.
fn default_path(config_dir: &Path) -> String {
    let dir = path_text(config_dir).replace('\\', "/");
    let inherited = ["LUA_PATH_5_4", "LUA_PATH"]
        .into_iter()
        .find_map(|name| std::env::var(name).ok())
        .map(|value| {
            let value = value.replace(";;", &format!(";{};", lua54_default_path()));
            value.trim_matches(';').to_owned()
        })
        .unwrap_or_else(lua54_default_path);
    format!("{dir}/?.luau;{dir}/?.lua;{dir}/?/init.luau;{dir}/?/init.lua;{inherited}")
}

fn lua54_default_path() -> String {
    if cfg!(windows) {
        let program = std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(Path::to_owned))
            .map(|path| path_text(&path))
            .unwrap_or_default();
        [
            "!\\lua\\?.lua",
            "!\\lua\\?\\init.lua",
            "!\\?.lua",
            "!\\?\\init.lua",
            "!\\..\\share\\lua\\5.4\\?.lua",
            "!\\..\\share\\lua\\5.4\\?\\init.lua",
            ".\\?.lua",
            ".\\?\\init.lua",
        ]
        .map(|template| template.replace('!', &program))
        .join(";")
    } else {
        [
            "/usr/local/share/lua/5.4/?.lua",
            "/usr/local/share/lua/5.4/?/init.lua",
            "/usr/local/lib/lua/5.4/?.lua",
            "/usr/local/lib/lua/5.4/?/init.lua",
            "./?.lua",
            "./?/init.lua",
        ]
        .join(";")
    }
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Lua-side half of the loader: the searcher protocol is plain Lua 5.4 behavior.
const PRELUDE: &str = r#"
local package, load_module, resolve_relative, load_file, load_text = ...
local error, ipairs, type = error, ipairs, type
local format, sub, concat = string.format, string.sub, table.concat
local info = debug.info

local function finish(key, name, loader, data)
    local result = loader(name, data)
    local loaded = package.loaded
    if result ~= nil then
        loaded[key] = result
    elseif loaded[key] == nil then
        loaded[key] = true
    end
    return loaded[key], data
end

package.searchers = {
    function(name)
        local loader = package.preload[name]
        if loader == nil then
            return format("\n\tno field package.preload['%s']", name)
        end
        return loader, ":preload:"
    end,
    function(name)
        local path = package.path
        if type(path) ~= "string" then
            error("'package.path' must be a string", 2)
        end
        local file, message = package.searchpath(name, path)
        if file == nil then
            return "\n\t" .. message
        end
        return load_module(file, name), file
    end,
}

function require(name)
    if type(name) ~= "string" then
        error(format("bad argument #1 to 'require' (string expected, got %s)", type(name)), 2)
    end
    local loaded = package.loaded
    if loaded[name] then
        return loaded[name]
    end
    if sub(name, 1, 2) == "./" or sub(name, 1, 3) == "../" then
        local file, tried = resolve_relative(info(2, "s"), name)
        if file == nil then
            error(format("module '%s' not found:%s", name, tried), 2)
        end
        if loaded[file] then
            return loaded[file], file
        end
        return finish(file, name, load_module(file, name), file)
    end
    local searchers = package.searchers
    if type(searchers) ~= "table" then
        error("'package.searchers' must be a table", 2)
    end
    local messages = {}
    for _, searcher in ipairs(searchers) do
        local loader, data = searcher(name)
        if type(loader) == "function" then
            return finish(name, name, loader, data)
        elseif type(loader) == "string" then
            messages[#messages + 1] = loader
        end
    end
    error(format("module '%s' not found:%s", name, concat(messages)), 2)
end

function loadfile(path, mode, env)
    return load_file(path, mode, env)
end

function dofile(path)
    local chunk, message = load_file(path)
    if chunk == nil then
        error(message, 0)
    end
    return chunk()
end

function load(chunk, name, mode, env)
    return load_text(chunk, name, mode, env)
end

_G.package = package
"#;
