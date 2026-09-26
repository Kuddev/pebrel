## 查看当前终端与运行状态

在 Pebrel 的本地终端运行：

```sh
pebrel env --pretty
pebrel ctl describe --pretty
pebrel pane list
```

`env`显示调用环境，`describe`列出当前运行时支持的能力，`pane list`列出可以定位的窗格。后续命令中的窗格 ID，要使用实际列表里的值。

命令找不到时，检查当前环境是否为本地 Pebrel 窗格，以及 `PEBREL_CLI`指向的程序。SSH 服务器上不会自动安装本机的 Pebrel CLI。

## 读取另一个窗格的输出

假设目标窗格 ID 为 `17`，读取最近 80 行：

```sh
pebrel pane read 17 --lines 80
```

多个窗口中存在相同 ID 时，补充对应的窗口参数。不要固定复用上一次启动中的 ID，关闭并重建窗格后应重新查询。

## 发送一条命令

确认目标停在普通 Shell 提示符，并位于正确项目目录后，发送：

```sh
pebrel pane send 17 "git status --short" --wait
```

`send`会提交文本并发送回车，`--wait`等待相应状态变化。只想插入文字供人工检查时，使用 `--no-submit`，不要同时加等待执行结束的选项。

读取窗格输出确认执行结果。等待结束不等于项目测试成功，成功与否仍由实际命令输出和退出状态判断。

## 发送保留换行的文本

将文本写入 UTF-8 文件 `task.txt`，然后执行：

```sh
pebrel pane paste 17 --from-file task.txt --wait
```

该方式保留换行，目标需要支持 bracketed paste。文件应是普通文本，输入上限为 32 KiB；不用于发送二进制、控制序列或给远端自动转发本机文件。

## 独立运行一个命令

需要在目标窗格的已知目录执行非交互命令，同时不改变其 Shell 输入时，可以使用：

```sh
pebrel pane exec 17 -- git status --short
```

`exec`直接启动独立子进程。命令和参数分别传入，管道、重定向和变量展开不会自动作为 Shell 语法执行。需要交互式程序时，仍应在终端会话中运行。

## 向 AI 工具分配任务

先执行 `pebrel agent list`确认工具名称或稳定 ID，再发送任务。例如，列表中的目标确实名为 `codex`时：

```sh
pebrel agent send codex "检查当前项目的测试入口，先不要修改文件" --wait
pebrel agent read codex --lines 80
```

自动化应使用列表返回的身份，而不是仅根据窗口焦点猜测目标。命名 Agent 和隔离目录的完整示例见[隔离 AI 工作目录](agent-worktrees.md)。

## 超时后如何处理

先读取目标状态和最近输出。如果任务已经收到但仍在执行，继续查看或等待；不要立即重复发送同一条任务，以免执行两次。

参数和能力相关错误，可以用 `pebrel ctl describe --pretty`检查当前版本，并参阅[运行时控制参考](https://github.com/Kuddev/pebrel/blob/9dc058d12765893553d5fc7a2c37c870c96168b0/docs/runtime-control-api.md)。运行时发现文件包含本机控制凭据，不应公开、同步给他人或转发端口。
