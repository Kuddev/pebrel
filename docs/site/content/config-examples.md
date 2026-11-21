## 在已有配置中加入示例

以下完整示例可单独用于一份 `pebrel.lua`。已经有配置时，只复制需要的赋值，并放在已有的 `return config`之前，不要重复创建多份配置对象。

修改后运行 `pebrel config check`。字段检查通过后，再新建终端验证启动相关设置。

## 字体、背景与回滚历史

```lua
local pebrel = require 'pebrel'
local config = pebrel.config_builder()

config.font = {
  normal = { family = 'Maple Mono NF CN', style = 'Regular' },
  size = 13.0,
}
config.window = {
  opacity = 0.96,
  padding = { x = 8, y = 8 },
}
config.scrolling = { history = 10000 }

return config
```

将字体名称改为当前电脑可用的字体。不透明度范围为 0–1，1 表示完全不透明。GUI 中已有相关偏好时，也检查 **设置 → 外观**是否覆盖了期望效果。

## 不同系统选择不同 Shell

```lua
local pebrel = require 'pebrel'
local config = pebrel.config_builder()

if pebrel.platform.os == 'windows' then
  config.terminal = {
    shell = { program = 'pwsh.exe', args = { '-NoLogo' } },
  }
else
  config.terminal = { shell = '/bin/bash' }
end

return config
```

该例要求 Windows 已安装 PowerShell 7，其他系统存在 `/bin/bash`。程序路径与参数分开填写；需要选择 WSL 或已识别 Shell 时，日常使用可以直接在 **设置 → 终端**中选择。

## 为新终端设置环境变量

```lua
local pebrel = require 'pebrel'
local config = pebrel.config_builder()

config.env = {
  EDITOR = 'nvim',
  MY_PROJECT_ENV = 'development',
}

return config
```

新建终端后，PowerShell 使用 `$env:MY_PROJECT_ENV`，Bash / Zsh 使用 `echo "$MY_PROJECT_ENV"`检查变量。

这些变量适合编辑器和项目标记，不适合写入准备公开分享的密钥或 Token。

## 单独保存配色模块

在主配置旁建立 `theme.lua`：

```lua
return {
  primary = {
    background = '#161616',
    foreground = '#e8e8e8',
  },
}
```

主文件 `pebrel.lua`：

```lua
local pebrel = require 'pebrel'
local config = pebrel.config_builder()

config.colors = require 'theme'

return config
```

保存任一已加载模块后，查看重载结果。模块名不写 `.lua`后缀，文件位置保持在配置可查找的目录里。

## 清空 Profile 列表

显式空数组使用 `pebrel.array()`：

```lua
local pebrel = require 'pebrel'
local config = pebrel.config_builder()

config.profiles = pebrel.array()

return config
```

普通 `{}`表示空对象，不能在需要数组的地方替代空列表。包含条目时，使用连续的 Lua 数组，参见[自定义启动入口](shells.md)。
