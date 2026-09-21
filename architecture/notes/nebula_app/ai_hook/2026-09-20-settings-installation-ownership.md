# Hook 设置与安装所有权

## Status

Implemented; integration validation in progress.

## Context

Agents 菜单需要安装、关闭多种本地 Hook。仅新增 UI 开关会与启动守护和卸载器
冲突：用户关闭后，另一实例仍可能按旧授权安装回来。反复开启还必须保留
Codex 原 notifier，不能每次再套一层包装。

## Evidence

- `nebula_settings/src/agent_hooks.rs` 保存逐 Agent 的持久选择；缺失键兼容旧的
  `ai_hooks=0`，默认只开启 Claude Code 与 Codex。
- `nebula_app/src/ai_hook/win/settings.rs` 统一操作锁、写入和实际配置读回。
- `win/codex_hooks.rs` 的 ownership marker 与 `installation::merge_groups`
  共同限制删除范围；`win/codex_notify.rs` 保留原 notify 的命令参数。
- Claude [command hook 合同](https://code.claude.com/docs/en/hooks#exec-form-and-shell-form)
  明确 `args` 存在时直接 spawn；路径与参数维持分离，不回退到 shell 拼接。
  旧的 helper 名称子串识别会误认 `echo pebrel-hook.exe`，修改/删除改认完整调用。
- Cursor [Hook 文档](https://cursor.com/docs/agent/hooks) 规定提交前响应及
  completed/aborted/error 结果，Grok [Hook 文档](https://docs.x.ai/build/features/hooks)
  说明它也读取 Claude 和 Cursor 配置，不能将借用入口的事件误识别为另一工具。
- Oh My Pi 的官方 ExtensionAPI 提供 `agent_end`，没有 Pi 的 `agent_settled`；
  两者的包版本不可作为同一个事件合同。

## Decision

- UI 只表达操作和结果，不实现安装合并或生命周期规则。
- 可执行文件发现与本机配置操作归平台适配器所有，保留 `ai_hook::integrations`
  入口及安装器的私有访问边界。UI 与交互测试查询共享能力表，协议与生命周期
  仍由 AI 模块负责，不在控件中散布操作系统判断。
- 菜单、CLI、守护共用跨进程操作锁；先持久化选择再修改 Hook。守护拿锁后重读，
  不沿用操作前的设置快照。卸载先关闭全部接入，再尽力清理每一项。
- 检测区分“存在”“完整”“错误”。成功反馈必须来自读回校验，不能把安装函数
  返回“未修改”解释成配置已完整；原生 Codex 与 legacy notify 一并检查。
- 共享配置只修改本工具可认领的条目，独立文件复用现有 hash/marker 归属规则。
  无法读取、格式异常、用户编辑或其他操作占锁时保留原文并报告错误。
  Codex 已知 `--previous-notify` JSON 包装沿用有界解析，卸载保留外层 notifier，
  仅还原其中本工具包装的原 argv；未知编码不以名称片段作为删除依据。
- 默认安装仅含 Claude Code 和 Codex。其他接入在设置中开启；普通设置重置
  保留 Hook 授权，避免重置外观等偏好时重新授权。
- 同一设置视图的写操作在途时禁用重复提交，完成后刷新真实配置。已提交写入
  不依赖视图继续存活；弱实体与序号拒绝过期回调。
- 新增适配只转换 provider 事实，共享 pane lifecycle 保持状态权威。OMP 复用
  bridge 的传输及身份逻辑，但明确走自身 `agent_end` 合同。
- Pi/OMP 的 helper 异步启动错误被消费；POSIX 保持 controlling tty，Windows
  隐藏 helper 窗口。缺失 helper 不能终止 Agent，也不能阻止 Cursor 提交。

## Rejected alternatives

- 恢复旧分支的整个安装器：会回退新版 Codex 原生 Hook 和 Kimi 配置合同。
- 只在 UI 里保存布尔值：后台守护、CLI 和卸载不能看到同一授权边界。

## Consequences

本地设置只管理本机 Hook；SSH/WSL 内的配置仍由现有远端安装入口管理。
已运行 Agent 是否重载配置由其自身决定，菜单提示重启会话。
不新增运行时依赖、常驻 provider 状态机或事件缓存。守护将配置变动合并成
单个有界信号，定时扫描仅用于新目录和 watcher 不可用的情况。

## Validation

对应回归在 settings、Windows 安装器、native event、helper 和 Agents 控件的
现有测试模块中，覆盖格式/编辑保护、原生能力、重复安装移除、忙状态、失败
反馈和视图销毁。产物与真实 GUI 的验证另行记录，不由编译或单测代替。

## Supersedes

None.

## Revisit when

Provider 更改配置发现、事件、结果或响应合同；增加非 Windows 本地安装；
或者需要把逐 Agent 授权应用到已运行的远端会话。
