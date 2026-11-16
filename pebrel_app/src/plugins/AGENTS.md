# Plugin command foundation

- 插件与产品 PR 的分工以 [BOUNDARIES.md](BOUNDARIES.md) 为权威：外观类明确可插件化；产品 Bug 与终端核心变更走 PR；缺少扩展点先补基础设施 PR，不将修复包装成必装插件。
- 核心终端、输入和会话所有权留在宿主；插件卸载或视图替换不销毁用户终端。扩展范围与当前已实现接口须分开描述。
- `boundary.rs` 是插件可调用 Runtime 方法的单一实现；清单只声明需求，不授予能力。新增 Runtime 方法不自动对插件开放，新增能力须审查授权范围和核心所有权。
- 本模块是显式命令的冷路径（CLI 或设置页按钮），不是常驻插件宿主；GUI 启动、绘制和输入不得调用同步执行器，设置页只在后台执行器里按次运行。
- 清单是元数据权威；原生命令分支不读取 Luau 源码、不创建插件 VM。配置与插件使用同一 Luau 引擎，但各自拥有 VM、模块与能力边界。
- Luau 只交出可验证的普通数据与宿主请求；VM 配额之外还需约束 JSON 展开、包文件与协议帧。
- 不增加 Luau 以外的语言运行时、后台扫描、全局缓存或空的生命周期框架；新增持续资源时再明确 owner、取消与代次。
- 安装库（`install.rs`）只复制清单声明的文件，回执由宿主写入；授权只来自用户的安装确认，不来自清单或包内文件。插件对主题库的写入集中在 `themes.rs`，复用主题包校验和主题文档校验，不另写主题规则，也不应用设置。
- 当前事实和命令示例见 README；设计依据见 [命令地基](../../../architecture/notes/pebrel_app/plugins/2026-09-28-bounded-command-foundation.md) 、[核心体验边界](../../../architecture/notes/pebrel_app/plugins/2026-10-01-core-experience-boundary.md) 和 [安装库与主题授权](../../../architecture/notes/pebrel_app/plugins/2026-11-09-install-library-and-theme-grant.md)。

- Luau 引擎、配置模块与不可捕获限额的依据见 [Luau VM 边界](../../../architecture/notes/pebrel_app/luau/2026-11-02-bounded-luau-vms.md)。
