## 创建配置文件

主题、通知和 SSH 等常用设置可以直接在界面中完成。需要保存可复用的终端参数、环境变量或 Profile 时，可以创建 Lua 配置。

在可运行 Pebrel 命令的终端执行：

```sh
pebrel config init --language zh-CN
pebrel config check
```

第一条生成带中文说明的配置，第二条检查语法与字段。初始化默认不会覆盖已有文件。需要英文说明时使用 `--language en-US`。

## 修改一个选项

打开生成的 `pebrel.lua`，保留开头的配置创建与结尾的 `return config`。例如：

```lua
local pebrel = require 'pebrel'
local config = pebrel.config_builder()

config.scrolling = { history = 10000 }
config.env = { EDITOR = 'nvim' }

return config
```

这个示例设置终端历史行数和默认编辑器环境变量。`EDITOR='nvim'`不会安装程序；使用前仍需安装相应编辑器。

保存后再次运行 `pebrel config check`。检查通过，再新建终端查看需要在启动时生效的选项。

## 配置文件在哪里

标准数据目录为：

| 系统 | 目录 |
| --- | --- |
| Windows | `%APPDATA%\Pebrel` |
| macOS | `~/Library/Application Support/Pebrel` |
| Linux | `$XDG_CONFIG_HOME/pebrel`，未设置时为 `~/.config/pebrel` |

初始化时以命令输出的实际路径为准。便携配置、环境变量和显式参数也可能指定其他文件。

需要检查特定文件时运行：

```sh
pebrel config check --config-file /path/to/pebrel.lua
```

将路径换成实际位置。这个检查命令不需要打开应用窗口。

## 指定另一套配置

启动参数 `--config-file`优先于 `PEBREL_CONFIG_FILE`环境变量，再往后才是标准位置的自动查找。使用 **PEBREL_CONFIG_DIR**可以指定独立配置与数据目录，适合隔离测试或便携使用。

旧的 `NEBULA_*`变量保留兼容。新旧文件并存时，Pebrel 名称的配置优先；同名配置中，Lua 优先于 TOML，再到旧的 YAML 格式。选中的文件有错误时，修复这份文件即可，不要期待自动换到另一份。

## 保存后重新加载

有效配置变更会自动重载。重载失败时，正在运行的应用保留上次有效配置；修正提示的行或字段后，再保存一次。

Shell、环境变量和回滚容量等启动相关设置，应在新窗格中验证。字体或颜色也可能受到界面偏好的覆盖；同时设置了两处时，先检查界面中是否已有对应覆盖项。

## 将配置拆成多个文件

主文件可使用 `require`读取旁边的 Lua 模块。例如，把配色放在 `theme.lua`，再在主文件中写 `config.colors = require 'theme'`。模块返回配置表即可；被加载的本地模块也参与重载。

更多可复制的例子见[配置示例](config-examples.md)。完整字段与加载规则见仓库的 [Lua 配置参考](https://github.com/Kuddev/pebrel/blob/9dc058d12765893553d5fc7a2c37c870c96168b0/docs/lua-configuration.md)。

> [!NOTE]
> Lua 配置会执行本地代码。使用他人提供的配置前，先阅读内容；GUI 的 `pebrel_settings.txt`使用另一套偏好键，不能直接当作 Lua 配置粘贴。
