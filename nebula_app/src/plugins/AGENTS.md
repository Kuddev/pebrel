# Plugin command foundation

- 本模块目前是显式 CLI 命令的冷路径，不是常驻插件宿主；GUI 启动、绘制和输入不得调用同步执行器。
- 清单是元数据权威；原生命令分支不读取 Lua 源码、不创建插件 VM。保持既有配置 Lua 独立。
- Lua 只交出可验证的普通数据与宿主请求；VM 配额之外还需约束 JSON 展开、包文件与协议帧。
- 不增加新语言运行时、后台扫描、全局缓存或空的生命周期框架；新增持续资源时再明确 owner、取消与代次。
- 当前事实和命令示例见 README；设计依据见 [命令地基](../../../architecture/notes/nebula_app/plugins/2026-09-28-bounded-command-foundation.md)。
