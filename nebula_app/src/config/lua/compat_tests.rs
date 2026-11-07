//! Configuration written for the former Lua 5.4 runtime must load unchanged on Luau.

use std::fs;
use std::path::Path;

use super::runtime::ReloadSignal;
use super::{LoadedLuaConfig, load_lua_file};

fn write(root: &Path, name: &str, source: &str) {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, source).unwrap();
}

fn load(root: &Path, source: &str) -> Result<LoadedLuaConfig, String> {
    write(root, "pebrel.lua", source);
    load_lua_file(&root.join("pebrel.lua"), ReloadSignal::default()).map_err(|e| e.to_string())
}

fn watched(loaded: &LoadedLuaConfig, name: &str) -> bool {
    loaded.config.config_paths.iter().any(|path| path.ends_with(name))
}

#[test]
fn lua54_era_configuration_loads_unchanged() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    write(
        root,
        "theme.lua",
        "return { primary = { foreground = '#d8dee9', background = '#16181d' } }",
    );
    write(root, "lib/sizes.lua", "local M = {}\nM.history = 4000\nreturn M");
    write(root, "extra/init.lua", "return { columns = 120 }");
    write(root, "vendor/vendored.lua", "return { lines = 40 }");
    write(root, "fragment.lua", "return 'Production SSH'");
    write(root, "data.txt", "first line\nsecond line\n");
    let loaded = load(
        root,
        r#"
        local pebrel = require 'pebrel'
        local config = pebrel.config_builder()
        local base <const>, step <const> = require('lib.sizes').history, 2
        package.path = pebrel.config_dir .. '/vendor/?.lua;' .. package.path
        package.preload['virtual'] = function(name) return { name = name } end
        assert(require('virtual').name == 'virtual')
        assert(rawequal(require('pebrel'), package.loaded.nebula))
        assert(os.getenv('PATH') ~= nil and os.getenv('PEBREL_SURELY_UNSET_VARIABLE') == nil)

        local data = assert(io.open(pebrel.config_dir .. '/data.txt', 'r'))
        assert(data:read('l') == 'first line' and data:read('*l') == 'second line')
        assert(data:read('l') == nil and io.type(data) == 'file')
        data:close()
        assert(io.type(data) == 'closed file')
        local count = 0
        for _ in io.lines(pebrel.config_dir .. '/data.txt') do count = count + 1 end
        assert(count == 2)

        assert(math.type(3) == 'integer' and math.type(0.5) == 'float' and math.tointeger(7) == 7)
        assert(string.rep('ab', 3, ',') == 'ab,ab,ab' and string.format('%d', 3) == '3')
        assert(select('#', table.unpack({ 1, 2, 3 })) == 3 and utf8.char(20013) == '中')
        assert(load('return 1 + 1')() == 2 and loadfile(pebrel.config_dir .. '/fragment.lua')())
        warn('legacy warn call')

        config.colors = require 'theme'
        config.scrolling = { history = base * step, multiplier = 3 }
        config.window = { opacity = 1, dimensions = { columns = require('extra').columns, lines = require('vendored').lines } }
        config.profiles = pebrel.array { { name = dofile(pebrel.config_dir .. '/fragment.lua'), command = 'ssh' } }

        local fields = 0
        for _ in pairs(config) do fields = fields + 1 end
        assert(fields == 4, 'pairs over the builder sees ' .. fields .. ' fields')
        return config
        "#,
    )
    .unwrap();
    assert_eq!(loaded.config.scrolling.history(), 8000);
    for name in ["theme.lua", "sizes.lua", "init.lua", "vendored.lua", "fragment.lua"] {
        assert!(watched(&loaded, name), "{name} is not on the reload watch list");
    }
}

#[test]
fn luau_relative_requires_resolve_next_to_the_requiring_file() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    write(root, "parts/window.luau", "return { columns = require('./size').columns }");
    write(root, "parts/size.lua", "return { columns = 99 }");
    let loaded = load(
        root,
        "local window: { columns: number } = require('./parts/window')\n\
         assert(require('./parts/window') == window)\n\
         return { window = { dimensions = { columns = window.columns, lines = 30 } } }",
    )
    .unwrap();
    assert!(watched(&loaded, "window.luau") && watched(&loaded, "size.lua"));
}

#[test]
fn missing_modules_list_every_searched_location() {
    let temp = tempfile::tempdir().unwrap();
    let error = load(temp.path(), "return require 'absent.module'").err().unwrap();
    assert!(error.contains("module 'absent.module' not found"), "{error}");
    assert!(error.contains("no field package.preload['absent.module']"), "{error}");
    assert!(
        error.contains("absent/module.lua'") || error.contains("absent\\module.lua'"),
        "{error}"
    );
}

#[test]
fn lua54_only_syntax_reports_its_luau_replacement() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let error = load(root, "local flags = 1\nflags = flags | 4\nreturn {}").err().unwrap();
    assert!(error.contains("line 2: bitwise operator `|`: use bit32.bor"), "{error}");
    assert!(error.contains("pebrel.lua:2"), "the error names the file and line: {error}");

    write(
        root,
        "module.lua",
        "for i = 1, 3 do\n  if i == 2 then goto continue end\n  ::continue::\nend\nreturn {}",
    );
    let error = load(root, "return require 'module'").err().unwrap();
    assert!(error.contains("error loading module 'module'"), "{error}");
    assert!(error.contains("line 2: goto"), "{error}");
}

#[test]
fn files_commands_and_process_control_follow_lua54_results() {
    let temp = tempfile::tempdir().unwrap();
    let loaded = load(
        temp.path(),
        r#"
        local dir = require('pebrel').config_dir
        local out = assert(io.open(dir .. '/out.txt', 'w'))
        assert(out:write('a', 1, '\n') == out)
        out:close()
        local append = assert(io.open(dir .. '/out.txt', 'a'))
        append:write('b\n')
        append:close()
        local text = io.open(dir .. '/out.txt'):read('a')
        assert(text == 'a1\nb\n', text)
        assert(os.rename(dir .. '/out.txt', dir .. '/moved.txt'))
        assert(os.remove(dir .. '/moved.txt'))
        local ok, message = os.remove(dir .. '/moved.txt')
        assert(ok == nil and message:find('moved.txt', 1, true))
        local missing, reason = io.open(dir .. '/nope.txt')
        assert(missing == nil and reason:find('nope.txt', 1, true))
        local scratch = os.tmpname()
        assert(os.remove(scratch))
        local seekable = assert(io.open(dir .. '/seek.txt', 'wb'))
        seekable:write('abcdef')
        seekable:close()
        local reader = assert(io.open(dir .. '/seek.txt', 'rb'))
        assert(reader:seek('set', 2) == 2 and reader:read(2) == 'cd')
        assert(reader:seek() == 4 and reader:seek('end') == 6 and reader:read(1) == nil)
        reader:close()
        assert(io.write('') == io.stdout and math.ult(1, -1) and not math.ult(-1, 1))

        local pipe = assert(io.popen('echo hello'))
        assert(pipe:read('l') == 'hello')
        assert(pipe:close() == true)
        local success, kind, code = os.execute('exit 3')
        assert(success == nil and kind == 'exit' and code == 3)
        assert(os.execute() == true)
        assert(not pcall(os.exit, 0))
        return { scrolling = { history = 7 } }
        "#,
    )
    .unwrap();
    assert_eq!(loaded.config.scrolling.history(), 7);
}

#[test]
fn whole_numbers_reach_integer_fields_and_fractions_stay_floats() {
    let temp = tempfile::tempdir().unwrap();
    let loaded = load(
        temp.path(),
        "return { scrolling = { history = 10 * 1000 }, window = { opacity = 0.5 } }",
    )
    .unwrap();
    assert_eq!(loaded.config.scrolling.history(), 10000);
}
