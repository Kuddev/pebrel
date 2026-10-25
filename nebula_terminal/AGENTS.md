# Terminal core rules

- 本目录只拥有 grid、VT、PTY、输入编码、回放和终端行为；不得依赖产品视图、窗口状态或 GPUI 组件。
- 输入修复必须分别验证原生控制台读取方、翻译后字节流读取方和协商协议；不能用单一路径正常推断所有客户端兼容。
- PowerShell 剪贴板能力只报告当前原生编辑器，界面准入和回退依据见 [`clipboard paste`](../architecture/notes/nebula_app/gpui_shell/terminal/view/2026-10-23-powershell-clipboard-paste.md)。
- 字节流、解析器和回放测试要覆盖分块边界、静默源、错误/EOF 和平台差异。间歇问题需要能稳定失败的判据。
- 热路径变更提供代表性成本证据；分配、吞吐或内存结论限定到实际测量的内容、尺寸、平台和构建配置。
- 不为 UI 需求在终端核心增加产品概念。确需新接口时，公开最小命令/结果并保持状态私有。
- 重大协议、缓冲、线程、所有权或兼容性决策写入 `architecture/notes/nebula_terminal/<capability>/`；普通回归修复只需测试和变更说明。
- 快照中的 DIM 与反色颜色归属见 [`render`](../architecture/notes/nebula_terminal/render/)；颜色保持未解析，前端统一处理调色板和 OSC 覆盖。
- OSC 7501 记录、PTY 生命周期和 GPUI 投影的边界见 [`program_status`](../architecture/notes/nebula_terminal/program_status/)。
- 有界输出载荷、命令捕获与网格标记的所有权见 [`inline_content`](../architecture/notes/nebula_terminal/inline_content/2026-10-21-bounded-output-store.md)；前端分类命令，核心不解释 MIME 正文。
