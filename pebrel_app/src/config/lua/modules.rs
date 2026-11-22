//! Configuration-local modules, with no package.path or native library loader.

use std::collections::HashSet;
use std::fs::File;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use mlua::{ChunkMode, Lua, Table, Value};

const MAX_MODULES: usize = 128;
const MAX_MODULE_BYTES: usize = 256 * 1024;
const MAX_TOTAL_BYTES: usize = 4 * 1024 * 1024;

#[derive(Default)]
struct Loading {
    active: HashSet<PathBuf>,
    modules: usize,
    bytes: usize,
}

pub(super) fn install(
    lua: &Lua,
    config_dir: &Path,
    api: Table,
    watched: Arc<Mutex<Vec<PathBuf>>>,
) -> mlua::Result<()> {
    let root = config_dir.canonicalize().map_err(mlua::Error::external)?;
    let cache = lua.create_table()?;
    let loading = Arc::new(Mutex::new(Loading::default()));
    let require = lua.create_function(move |lua, name: String| {
        if matches!(name.as_str(), "pebrel" | "nebula") {
            return Ok(Value::Table(api.clone()));
        }
        let path = resolve(&root, &name).map_err(mlua::Error::external)?;
        let key = path.to_str().ok_or_else(|| mlua::Error::external("module path is not UTF-8"))?;
        if cache.contains_key(key)? {
            return cache.raw_get::<Value>(key);
        }
        {
            let mut state = loading.lock().unwrap_or_else(|lock| lock.into_inner());
            if state.active.contains(&path) {
                return Err(mlua::Error::external(format!("cyclic configuration module: {name}")));
            }
            if state.modules >= MAX_MODULES || state.active.len() >= 32 {
                return Err(mlua::Error::external("configuration module count/depth exceeded"));
            }
            state.modules += 1;
            state.active.insert(path.clone());
        }
        // Do not hold the Rust lock across evaluation: modules may require other modules.
        let result = (|| {
            let source = read_source(&path, MAX_MODULE_BYTES).map_err(mlua::Error::external)?;
            {
                let mut state = loading.lock().unwrap_or_else(|lock| lock.into_inner());
                state.bytes = state.bytes.saturating_add(source.len());
                if state.bytes > MAX_TOTAL_BYTES {
                    return Err(mlua::Error::external("configuration modules exceed 4 MiB"));
                }
            }
            {
                let mut paths = watched.lock().unwrap_or_else(|lock| lock.into_inner());
                if !paths.contains(&path) {
                    if paths.len() >= 1024 {
                        return Err(mlua::Error::external(
                            "configuration watch list exceeds 1024 paths",
                        ));
                    }
                    paths.push(path.clone());
                }
            }
            let result: Value = lua
                .load(source.strip_prefix('\u{feff}').unwrap_or(&source))
                .set_mode(ChunkMode::Text)
                .set_name(key)
                .eval()?;
            let result = if matches!(result, Value::Nil) { Value::Boolean(true) } else { result };
            cache.raw_set(key, result.clone())?;
            Ok(result)
        })();
        loading.lock().unwrap_or_else(|lock| lock.into_inner()).active.remove(&path);
        result
    })?;
    lua.globals().raw_set("require", require)
}

fn resolve(root: &Path, name: &str) -> Result<PathBuf, String> {
    if name.is_empty() || name.len() > 512 || name.contains(['\\', ':', '\0']) {
        return Err("use a configuration-relative module name".into());
    }
    let relative = if let Some(relative) = name.strip_prefix("./") {
        relative.to_owned()
    } else if name.contains('/') {
        name.to_owned()
    } else {
        // Existing Lua module names retain their dotted-directory convention.
        name.replace('.', "/")
    };
    let relative = Path::new(&relative);
    if relative.components().any(|part| !matches!(part, Component::Normal(_))) {
        return Err("modules must stay inside the configuration directory".into());
    }
    let base = root.join(relative);
    let candidates =
        if matches!(base.extension().and_then(|ext| ext.to_str()), Some("lua" | "luau")) {
            vec![base]
        } else {
            vec![
                base.with_extension("luau"),
                base.with_extension("lua"),
                base.join("init.luau"),
                base.join("init.lua"),
            ]
        };
    for path in candidates {
        if path.is_file() {
            let path = path.canonicalize().map_err(|error| error.to_string())?;
            if !path.starts_with(root) {
                return Err("module links must stay inside the configuration directory".into());
            }
            return Ok(path);
        }
    }
    Err(format!("configuration module not found: {name}"))
}

pub(super) fn read_source(path: &Path, limit: usize) -> std::io::Result<String> {
    let file = File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::other("configuration source must be a regular file"));
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(std::io::Error::other(format!("configuration source exceeds {limit} bytes")));
    }
    String::from_utf8(bytes)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}
