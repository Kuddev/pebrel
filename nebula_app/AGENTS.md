# Nebula application rules

- 本目录是应用编排和适配层；依赖方向保持为 UI/组合 → 应用能力 → 共享领域规则，不在视图里复制设置、持久化、终端状态或协议状态机。
- `src/gpui_shell` 负责 GPUI 呈现、命令和订阅。交互改动必须覆盖默认、hover、pressed、键盘焦点、disabled/loading 以及成功/失败中实际需要的状态。
- 操作形成“触发 → 执行 → 可感知结果”的闭环。成功只能在操作真实完成或底层接口明确接受后显示，不能把回调触发当成平台级成功。
- 异步工作必须有所有者、取消/过期策略和视图销毁清理；旧结果不得覆盖新状态。渲染回调不得同步等待磁盘、网络、子进程或长锁。
- 新 UI 文案使用现有 typed message ID、命名占位符和回退合同，遵循 [`../docs/internationalization.md`](../docs/internationalization.md)。
- 测试真实控件和布局路径；编译或状态单测不能代替 hover、命中区域、键盘路径和视觉验收。
- 只有跨层所有权、持久化、协议、线程/生命周期或重要性能取舍才写 note，路径镜像到 `architecture/notes/nebula_app/<capability>/`。
- Agent 集成、改键输入与 SSH 认证的因果记录分别位于 [`ai_hook`](../architecture/notes/nebula_app/ai_hook/)、[`input`](../architecture/notes/nebula_app/input/) 和 [`ssh_session`](../architecture/notes/nebula_app/ssh_session/)。
