## 为并行任务使用不同目录

两个 AI 工具同时修改同一目录，可能覆盖对方的文件。Git worktree 可以让它们使用同一仓库的不同工作目录与分支。

开始前保存当前文件，在源项目中运行 `git status`。先提交或妥善保存未提交改动，再创建新的工作目录。

## 使用 Pebrel 创建命名 Agent

在本地 Pebrel 终端中运行：

```sh
pebrel pane list
pebrel ctl describe --pretty
```

确认源窗格的 ID、所属窗口及当前运行时支持的 Agent 能力。假设窗口为 `1`、源窗格为 `17`，且 Codex 已安装：

```sh
pebrel ctl agent-fork --window 1 --source-pane 17 --name docs-review --kind codex --pretty
```

把数字换成自己的列表结果。成功后，响应会给出创建的 worktree 和 Agent 信息，终端将在新目录中启动相应工具。

默认分支形如 `pebrel/docs-review`，工作目录位于主仓库旁的 `<仓库名>-worktrees/docs-review`。同名分支或目录已经存在时，改用新的任务名称，或先确认旧任务已不再使用。

## 核对目录再发任务

读取新 Agent：

```sh
pebrel ctl agent-get --agent docs-review --pretty
```

检查返回的目录与分支，再发送一项任务：

```sh
pebrel agent send docs-review "检查安装文档中的路径示例，列出需要修改的位置" --wait
pebrel agent read docs-review --lines 80
```

任务完成后，进入这个 worktree 查看 `git diff`并运行相关检查。再按项目的 Git 工作流程提交和合并。

## 只需要手动建立 worktree

也可以使用 Git 自带命令。在源仓库目录中运行下面的示例，再新建终端进入新目录：

```sh
git worktree add ../my-project-docs -b docs/review
```

该命令会创建一个新分支和目录。检查成功后，在新终端中进入 `../my-project-docs`，启动已安装的 AI CLI。

## SSH 上的项目

Pebrel 的本机 Agent worktree 创建入口不替远端 SSH 会话建立工作目录。需要在服务器上隔离任务时，在服务器终端中使用它的 Git 命令，并选择服务器可写路径。

## 完成后清理

确认所有修改已提交或保存，并退出使用该目录的工具。先运行 `git worktree list`核对目录，再用 `git worktree remove 实际目录`移除不再使用的工作副本。

不要直接删除仍有未提交改动的目录。对话分叉只复制会话上下文，需要文件隔离时仍按本页建立 worktree。
