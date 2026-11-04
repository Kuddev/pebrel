# Pebrel Luau Configuration

Pebrel uses vendored [Luau](https://luau.org/) for programmable configuration. Load its API with
`require 'pebrel'` and generate a configuration with `pebrel config init`.
Existing `require 'nebula'` calls resolve to the same API table. Legacy `.lua`
filenames remain discoverable, but scripts use Luau syntax and libraries: Lua 5.4-only
features such as `<close>`, integer bitwise operators, `io`, and `package` are not supported.

Lua configuration is executable local code. Only use configuration files and
modules that you trust. Pebrel does not download or execute remote
configuration during discovery.

## Quick Start

Create a localized, annotated configuration:

```text
pebrel config init --language system
pebrel config init --language zh-CN
pebrel config init --language en-US
```

Pebrel refuses to overwrite an existing file. `--force` first creates a
timestamped backup and only then atomically replaces the target.

Validate a configuration without opening a window:

```text
pebrel config check
pebrel config check --config-file D:/configs/pebrel.luau
```

## Basic Configuration

```lua
local pebrel = require 'pebrel'
local config = pebrel.config_builder()

config.window = {
    opacity = 0.96,
    decorations = 'Full',
}

config.font = {
    normal = { family = 'Maple Mono NF CN', style = 'Regular' },
    size = 13.0,
}

config.scrolling = {
    history = 10000,
}

config.env = {
    EDITOR = 'nvim',
}

return config
```

Returning a plain table is also supported, but `config_builder()` is
recommended because it validates top-level assignments as they are made.
Unknown fields and invalid values are errors by default.

## Native UI Defaults

The GPUI shell reads `config.ui` from the same discovered entry at startup:

```lua
config.ui = {
    theme = 'Nord',
    font_size = 16,           -- logical pixels, unlike config.font.size in points
    ui_font_size = 13,
    tabs_position = 'sidebar',
    sidebar_width = 230,
    pane_card_radius = 10,
    pane_card_gutter = 6,
    pane_card_shadow = true,
    opacity = 0.96,
    tab_bar_opacity = 0.92,
    cursor_motion = 'smooth',
    scrollbar_visibility = 'hover',
}
```

The supported field names and scalar shapes are defined by
[`ui_value_kind`](../nebula_settings/src/config_defaults.rs). Numeric ranges,
enum aliases and defaults continue to come from `RuntimeSettings::from_raw`;
unknown fields and incorrect scalar types fail `config check`. Background-image
paths in `config.ui` are relative to the entry configuration directory.

Explicit keys in `pebrel_settings.txt`, including empty values, take priority over
these defaults. Settings controls keep their current immediate-save behavior and
do not rewrite user code. Removing a saved key reveals the script value; restoring
Settings defaults removes known saved overrides and therefore reveals the script
configuration too. Removing a script field restores its built-in/theme default
when no saved override exists. Restart after editing the script. The startup
adapter installs one immutable defaults snapshot for all runtime settings readers;
rendering and Settings saves do not execute Luau.

For Luau entries, customized `font`, `colors`, and `mouse` sections also reach the
native presentation adapter. Window dimensions keep their existing startup-only
semantics. The typed parser and `config check` are shared with other formats.

## Shader Configuration

Background and terminal effects use `config.shaders` from the same entry:

```lua
config.shaders = {
    background = {
        enabled = true,
        path = 'shaders/background.glsl',
        speed = 0.8,
        frame_rate = 20,
        parameter = 0.5,
    },
    terminal = {
        enabled = true,
        paths = { 'shaders/scanlines.frag' },
        animation = 'focused',
        speed = 1.0,
        frame_rate = 20,
        parameters = { 0.4, 0.0, 0.0, 0.0 },
    },
}
```

Paths are relative to `pebrel.luau`, including when its data directory is elsewhere.
An omitted `enabled` is false: a path by itself never runs a shader. For an included
background use `preset = 'neon_vortex'` instead of `path`; the existing grain,
aurora ribbons and liquid silk presets are available too. Saved Settings values
override these defaults, and script edits take effect after restarting.

Both effects accept speed 0–10 (default 1) and frame rates 1–60 (default 20).
Speed 0 stops animation wakeups. Frame rate controls animation requests; terminal
output can still cause additional paints. Terminal animation accepts `off`,
`focused` or `always`; the default is `focused`. Hidden views, reduced motion,
renderer capabilities and existing resource limits continue to govern playback.
Background shaders currently use the Windows backend; terminal effects use the
current window's postprocessing capability.

Files ending in `.glsl` or `.frag` are compiled as GLSL. Existing `.wgsl` sources
remain supported and may be mixed with GLSL in an ordered terminal chain of up to
eight files/eight total passes. Each file is UTF-8 and at most 64 KiB. A complete
GLSL 450 fragment provides its own declarations and must fit Pebrel's resource
ABI. A versionless snippet defines `void mainImage(out vec4 color, in vec2 pixel)`;
Pebrel supplies its entry, `iResolution` and `iTime`. Coordinates use the surface's
top-left origin. This does not implement Shadertoy texture channels or includes.

Background snippets receive `iParameter` from `background.parameter`. Full
background fragments may declare one 16-byte uniform at set 0/binding 0 containing
width, height, scaled seconds and that parameter. Terminal snippets receive the
[`GLSL frame ABI`](../nebula_app/src/gpui_shell/terminal/effects/abi.glsl), including
`load_surface`, `sample_surface`, the palette/cursor data and `iParameters`.
The four values are also available as `frame.parameters` in WGSL. Frame ABI version
2 appends those values after the palette, preserving every earlier field offset.
Other textures, storage and resource bindings are rejected by the renderer's
existing validation. Shader reads and compilation stay on background workers.

Start with the [background example](../nebula_app/examples/shaders/background.glsl)
and [terminal example](../nebula_app/examples/shaders/scanlines.frag). The complete
configuration shape is defined in [`config/shaders.rs`](../nebula_app/src/config/shaders.rs).
`config check` validates configuration values and paths' syntax; actual shader
compilation occurs when an effect is enabled in a supported window.

## Arrays And Modules

Lua's `{}` does not distinguish an empty array from an empty object. Pebrel
treats an ordinary empty table as an object; use `pebrel.array()` for an empty
array:

```lua
config.profiles = pebrel.array()

config.profiles = pebrel.array {
    {
        name = 'Production SSH',
        command = 'ssh',
        args = { 'user@server' },
    },
}
```

Non-empty tables with consecutive integer keys are arrays automatically.
Sparse arrays and tables mixing integer and string keys are rejected.

Split large configurations into sibling modules:

```lua
-- theme.luau
return {
    primary = {
        foreground = '#d8dee9',
        background = '#16181d',
    },
}
```

```lua
-- pebrel.luau
local pebrel = require 'pebrel'
local config = pebrel.config_builder()
config.colors = require 'theme'
return config
```

Files successfully loaded through `require` are added to the live-reload watch
list. Extra local files can be registered with
`pebrel.add_to_config_reload_watch_list(path)`.

`require 'theme'` and `require './theme'` resolve from the entry configuration's
directory. Dotted module names still map to subdirectories. The loader tries
`.luau`, `.lua`, `init.luau`, then `init.lua`, caches successful results, and rejects
cycles and paths/links escaping that directory. Native module loading and
`package.path` are not available. A config generation allows a 1 MiB entry,
128 module loads, depth 32, 256 KiB per module and 4 MiB total module source.
The VM has a 32 MiB allocator limit, 1000000 safe-point interrupts and a two-second
execution deadline; these are not a process-memory or exact CPU-time guarantee.

## Shell IDs And Config Profiles

`pebrel --shell ID`, the runtime `shell=` preference and the Runtime API `shell`
parameter select detected shells or imported terminal profiles. A Lua
`config.profiles` entry does not register an ID for these selectors. Its `name`
is a display label; its optional `shell_id` identifies the shell family for icons
and integration. Passing `config check` validates the configuration, not a new
shell alias.

Imported profiles live in `terminal_profiles.json`. Their settings IDs combine
the stored shell family and profile identity as
`profile:<shell-id>|<profile-id>`, as defined by
[`Profile::settings_id`](../nebula_app/src/config/ui_config.rs).
The complete ID is case-sensitive and requires an actual, launchable store
entry. Adding `terminal_profile_id` to a Lua entry does not create that entry.
When using such an ID on the command line, quote it because it contains `|`.

To open a program with custom arguments directly, use `-e`, with all Pebrel
options before it. For example, on Windows with an Ubuntu WSL distribution:

```powershell
pebrel -e wsl.exe -d Ubuntu --cd '~'
```

This invokes WSL with its guest-home argument and does not look up a Lua profile.
Do not combine `-e` with `--shell`. For shell ID selection, Windows PowerShell
uses `powershell`; PowerShell 7 uses `pwsh` when it is detected. They are distinct
IDs. See the [Runtime API shell parameter](runtime-control-api.md) for request
and error behavior.

Lua `config.profiles` 的 `shell_id` 表示 Shell 家族，不是 `--shell`、`shell=` 或
Runtime API 的自定义别名；`config check` 成功也不代表注册了该 id。导入配置的完整
settings id 必须对应 `terminal_profiles.json` 中的实际条目，区分大小写，命令行中
需要加引号。要直接传入 WSL 等程序的启动参数，可使用上面的 `-e` 命令；所有
Pebrel 选项须放在 `-e` 之前，且不能同时指定 `--shell`。

## Terminal Selection Colors (TOML Compatibility)

For a legacy `pebrel.toml` configuration, the GPUI terminal accepts explicit
selection foreground and background colors in `#rrggbb` or `0xrrggbb` form:

```toml
[colors.selection]
foreground = "#102030"
background = "#e5e9f0"
```

Previously the GPUI adapter loaded the selection background but ignored the
foreground, which could leave selected text unreadable against a custom
background. It also cleared a custom foreground when applying a theme such as
Paper that does not declare selection colors. Both values now survive loading
and themes that do not override them. A theme that explicitly declares selection
colors, such as Nord, retains its existing precedence.

Missing or invalid foreground colors keep the existing fallback. Relative
`CellForeground` / `CellBackground` values are not added by this change. The
regression tests cover parsing and theme precedence; this does not change the
answer reader's selection overlay, formula selection layout, or the default
selection contrast. No new setting, dependency, or per-frame work is introduced.

## Platform Values

The module exposes stable runtime paths and platform data:

```lua
local pebrel = require 'pebrel'

if pebrel.platform.os == 'linux' then
    -- display_server is 'wayland', 'x11', or 'unknown'.
    pebrel.log_info('Linux display: ' .. pebrel.platform.display_server)
end

-- pebrel.config_file, pebrel.config_dir, pebrel.home_dir
-- pebrel.executable_dir, pebrel.version, pebrel.target_triple
```

## Discovery Order

An explicit `--config-file` wins, followed by `PEBREL_CONFIG_FILE` and then the
legacy `NEBULA_CONFIG_FILE` alias. Empty environment values are ignored. Explicit
paths must end in `.luau`, `.lua`, `.toml`, `.yml`, or `.yaml`.

Without an explicit path, Pebrel searches its own filenames before legacy
`nebula.*` names. Within each name, the format order is Luau, legacy Lua filename, TOML, YML, then YAML.
For example, `pebrel.toml` takes precedence over `nebula.lua`. An existing but
invalid chosen file reports an error; Pebrel does not silently load another file.

The default data directories are:

| Platform | Directory |
| --- | --- |
| Windows | `%APPDATA%/Pebrel` |
| macOS | `$HOME/Library/Application Support/Pebrel` |
| Linux | `${XDG_CONFIG_HOME:-$HOME/.config}/pebrel` |

`PEBREL_CONFIG_DIR` overrides the data directory, with `NEBULA_CONFIG_DIR` as a
fallback alias. When either nonempty override is set, automatic discovery stays
inside that directory; it does not read configuration from the executable,
home, or system directories. `pebrel config init` writes `pebrel.luau` into the
active data directory.

Windows Luau locations:

1. `pebrel.luau` beside `pebrel.exe` for portable installations.
2. `%APPDATA%/Pebrel/pebrel.luau`.
3. `%USERPROFILE%/.pebrel.luau`.

Linux Luau locations:

1. `$XDG_CONFIG_HOME/pebrel/pebrel.luau`.
2. `$XDG_CONFIG_HOME/pebrel.luau`.
3. `$HOME/.config/pebrel/pebrel.luau`.
4. `$HOME/.pebrel.luau`.
5. `/etc/pebrel/pebrel.luau`.

On macOS, the data-directory location is checked before the Linux-style XDG,
home, and system locations listed above. The same location order applies to
`.lua`, `.toml`, `.yml`, and `.yaml`, followed by legacy `nebula.*` names and locations.
Existing TOML configurations remain supported; YAML is a deprecated transition
format.

## Existing Nebula Data

On first startup, Pebrel copies legacy data into its data directory without
overwriting existing Pebrel files. Preferences use `pebrel_settings.txt`; new
main configuration files use `pebrel.luau`, `pebrel.toml`, `pebrel.yml`, or
`pebrel.yaml`. Session, SSH, and other files whose names do not contain the old
brand retain their names.

The source directory and old configuration names remain available for recovery
and for manual migration of existing modules. The migration records
completion only after all copies succeed, and can retry after an error. Once
complete, later launches do not copy old data back over deliberately removed
files. Existing newer files remain authoritative throughout the migration.

## Transactional Reload

In the legacy shell, configuration parsing and Luau execution run on one serial worker. Rapid file
saves are coalesced, and only the latest successful generation is published.
If Lua syntax, module loading, conversion, or validation fails, Pebrel keeps
the last-known-good `UiConfig` and Lua VM alive. Fixing and saving the file
causes the next valid generation to replace them together.

Linux and macOS package targets and native acceptance requirements are listed
in the [installation guide](../INSTALL.md). Configuration support alone is not
evidence that a native package has passed its runtime checks.

## 中文说明

`pebrel config init --language zh-CN` 会生成活动代码与英文模板完全一致、
但说明全部为简体中文的 UTF-8 配置。切换 Pebrel 界面语言不会改写已经存在的
Lua 文件。普通空表 `{}` 表示对象；空数组必须写成 `pebrel.array()`。保存配置
后，旧壳会保留上一份可用配置并在修复错误后恢复自动重载。GPUI 的脚本配置在启动时
读取；修改后重启生效，设置界面的即时更新沿用原有路径。

旧的 `require 'nebula'` 与新的 `require 'pebrel'` 指向同一 API。显式配置文件优先，
其次为 `PEBREL_CONFIG_FILE`、兼容的 `NEBULA_CONFIG_FILE`，然后依次查找 Pebrel 与
Nebula 文件；每种名称内按 Luau、兼容 Lua 文件名、TOML、YML、YAML 排序。首次迁移会非覆盖复制旧数据，
保留旧目录与文件供恢复使用，不删除来源，也不覆盖已有的新配置。
