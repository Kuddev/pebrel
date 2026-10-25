# Plugin command foundation

- 插件与产品 PR 的分工以 [BOUNDARIES.md](BOUNDARIES.md) 为权威：外观类明确可插件化；产品 Bug 与终端核心变更走 PR；缺少扩展点先补基础设施 PR，不将修复包装成必装插件。
- 核心终端、输入和会话所有权留在宿主；插件卸载或视图替换不销毁用户终端。扩展范围与当前已实现接口须分开描述。
- `boundary.rs` 是插件可调用 Runtime 方法的单一实现；清单只声明需求，不授予能力。新增 Runtime 方法不自动对插件开放，新增能力须审查授权范围和核心所有权。
- 本模块目前是显式 CLI 命令的冷路径，不是常驻插件宿主；GUI 启动、绘制和输入不得调用同步执行器。
- 清单是元数据权威；原生命令分支不读取 Lua 源码、不创建插件 VM。保持既有配置 Lua 独立。
- Lua 只交出可验证的普通数据与宿主请求；VM 配额之外还需约束 JSON 展开、包文件与协议帧。
- 不增加新语言运行时、后台扫描、全局缓存或空的生命周期框架；新增持续资源时再明确 owner、取消与代次。
- 当前事实和命令示例见 README；设计依据见 [命令地基](../../../architecture/notes/nebula_app/plugins/2026-09-28-bounded-command-foundation.md) 和 [核心体验边界](../../../architecture/notes/nebula_app/plugins/2026-10-01-core-experience-boundary.md)。
