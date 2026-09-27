# 用户反馈回归测试清单

本清单覆盖 2026-09-23 汇总的 Hook、更新、窗口材质、链接、Codex 显示与
Windows 右键菜单与 F2 快捷键冲突反馈。它是验收合同，不表示下列功能已经全部修复。
`待验证`、`等待日志`、`部分覆盖` 均不得作为 Issue 已解决的依据。

## 执行与证据

- 每轮记录源码提交、未提交补丁、产品版本、Windows build、DPI、主题、字体、
  Shell 与 Codex 版本。记录实际命令、通过/失败数量及截图或日志位置。
- 自动回归使用实际生产函数、真实终端网格或进程夹具；不能只检查源码中出现
  某个字符串。原生截图不替代行为测试，编译成功也不替代视觉验收。
- 对修复先验证旧实现失败，再验证新实现通过；保留相邻功能的反例。
- 浅色/深色、100%/125%/150%/200% DPI、宽/窄窗口、分屏、滚动和主题热切换
  按条目覆盖。平台条件导致无法执行的项必须注明，不计入通过数量。
- 实机操作使用隔离配置与测试窗口。安装/注册表夹具只修改其临时测试根，
  不关闭用户会话，不覆盖正式安装，不触碰用户 Hook、凭据和发行版配置。
- 仅关闭已有代码与验收证据覆盖的问题单；部分修复说明剩余范围。
  发布版本尚未包含修复时必须说明交付状态，不把本地通过写成已发布。

## 回归项目

| ID | 问题与来源 | 自动回归 / 通过条件 | 实机验收 / 反例 | 当前证据 |
| --- | --- | --- | --- | --- |
| HOOK-255 | [#255](https://github.com/Kuddev/pebrel/issues/255)：Windows Codex Hook 被 PowerShell 解析为字符串而未执行 | 生成命令实际执行；`codex`、`--hooks=full/turns` 和 stdin JSON 完整传入；带空格、单引号、`$`、反引号的路径保持字面值 | Windows PowerShell 5.1 与 PowerShell 7；Codex 0.155.1/0.156.0；新会话、工具调用、提问、停止都无 ParserError | Windows 5.1 进程夹具通过；完整 Codex/PowerShell 7 会话待验收 |
| HOOK-LIFETIME | #258 前置故障：关闭 Pebrel 后 Hook 仍占用安装文件 | 保持 stdin 打开、连接后不读取命名管道时，真实 Hook 子进程仍在期限内以 0 退出；Cursor 继续允许提交；用户原有 notifier 仍执行 | 已运行的旧版 Hook 不会自动获得新期限；分别验证新启动的 native Hook 与旧进程恢复步骤 | 7 项单元 + 5 项真实进程回归通过 |
| HOOK-MIGRATE | 更新后旧 `hooks.json` / 归属标记仍含失效命令 | 旧受管命令迁移；重复安装幂等；保留 notify、第三方 Hook、显式禁用及用户编辑；卸载只移除自有条目 | 重启 Pebrel 后自动修复；用户修改过的冲突条目不能静默覆盖 | `ai_hook::` 120 通过、1 项既有忽略；后续改动需重跑相关项 |
| UPDATE-403 | 手动检查更新返回 HTTP 403 | 公共 Release 无需用户 token；覆盖 403/429、正常 JSON、其他错误、代理、受限响应；下载仍须官方 URL、精确资产名和 SHA-256 | 同一网络直连/代理、启动检查/手动检查；离线不得假报最新或成功 | 已随 #269 合入；403/429、代理、重定向与校验清单回归由原生 CI 覆盖；作者记录 macOS ARM64 实机查询通过；整合后五平台必需 CI 全绿 |
| UPDATE-EXIT5 | 安装器 `exit code 5`，当前/最新版本显示相同 | 失败不得标成功；保留原快照与安装日志；已退出进程、安装失败、部分替换、相同版本、校验失败分别验证 | 基于反馈者 `installer.log` 重现；核对原版本、目标版本、实际 EXE；不能把 Inno 退出码 5 当作 Win32 拒绝访问码 | 后续 #258 已提供修复安装成功日志；无限等待的 Hook 已补期限；安装前等待短暂占用释放，持续占用时在启动安装器前失败，避免部分替换；旧版本已有挂起进程仍需另行处理 |
| UPDATE-258 | [#258](https://github.com/Kuddev/pebrel/issues/258)：同版本修复成功却因主程序哈希不变报错 | 同版本主程序字节不变、Hook 文件修复后应成功；安装器失败或目标版本仍错误必须失败 | 同版本重装、首次升级部分成功后的修复；日志、实际版本和结果提示一致；原会话快照保留 | 12 项原生 handoff 场景通过，包含 `repair-identical` 与 `upgrade-noop` 正反例 |
| BLUR-236-A | [#236](https://github.com/Kuddev/pebrel/issues/236)：Acrylic 在屏幕边缘透出背景内容 | 切换材质/透明度不重复创建资源；新窗口、关闭、失焦、最大化/还原、队列退出及运行库失败路径 | 0% 与非零不透明度；明显文字/色块背景；左右/上下屏幕边缘、最大化、任务栏恢复、混合 DPI；失焦不出现不透明灰底 | [PR #250](https://github.com/Kuddev/pebrel/pull/250) 尚未合并；其跨平台 CI 通过不等于本轮实机验收 |
| BLUR-236-B | #236 的 Aero 与 Acrylic 兼容回退范围 | Windows 10、Win11 22H2+、缺失/不匹配运行库与失败挂接均可正常启动/切换/退出 | Aero 需单独验收；缺少受支持 Windows App Runtime 时旧边缘现象不能标为已解决 | #250 仅覆盖受支持 Acrylic；**Aero 和回退范围仍开放** |
| LINK-CJK | OSC 8 中文/全角字符的虚线只画半字、错位 | 从真实 VT 输出构建网格，中文两格、空格、ASCII、组合字符均按列覆盖；逐格绘制与连续区间的物理像素集合相同 | `A开始 菜单Z`、纯中文、混排；DPI/分屏/窄窗/换行；不随字体回补或字形分段重置虚线 | 真实 OSC 8 网格与 16 种 DPI/格宽组合像素覆盖回归通过；原生 Nord / Paper 混排截图已核对 |
| LINK-PATH | 英文路径下划线断开，带空格路径只匹配前半段 | 连续虚线相位；引号包围的盘符/UNC/`~/` 路径完整匹配；hover、点击使用同一边界；普通正文不误连 | `"C:\Program Files\Example Tools\probe.exe"`、中文与括号目录、相邻命令参数、路径换行 | 8 项 hint 测试与 5 项配置解析测试通过；原生带空格路径截图已核对 |
| LINK-PROMPT | WSL `user@host:~/.codex$` 吞入 `$`；绝对路径提示符没有下划线 | `~/` 与 `/mnt/d/...` 提示符中的 `$`/`#` 不可点；真实文件名 `~/cost$`、URL 尾部 `$`、路径内部 `$` 保留；下划线与命中边界一致 | Bash/WSL 彩色提示符、root 提示符、输入命令后、换行；OSC 8 显式 URI 不被启发式改写 | 正反匹配/命中回归通过；绝对路径打开应使用所属 WSL 发行版，不得套用宿主目录或 SSH 目标 |
| LINK-COMMAND | [#196](https://github.com/Kuddev/pebrel/issues/196)：macOS 链接点击无效且修饰键提示不正确 | Command+点击 URL、文件及 OSC 8；鼠标上报模式、先松修饰键、拖动取消、错误修饰键和禁用 hint；普通点击保持上报 | 中英韩提示、窄窗/DPI；真实文件与浏览器打开 | #266 含真实布局 GPUI 回归，作者记录用户 macOS 复测通过；整合后五平台必需 CI 全绿，#196 已随 #266 合并关闭 |
| UPDATE-MAC | [PR #269](https://github.com/Kuddev/pebrel/pull/269)：macOS 应用内安装 | 精确架构/包名/SHA-256/DMG/签名/版本；成功、即时启动失败回滚、取消、错误哈希、多实例、恢复凭据与命令大输出 | Apple Silicon/Intel；可写目录、磁盘映像启动、App Translocation、签名身份；保留原应用 | 作者记录 ARM64 原生五场景及实际窗口更新通过；整合后两个 macOS 目标 CI 通过，新增大输出及更新核心回归日志已核对；完整安装实测范围为 ARM64 |
| CODEX-SPARKLE | 对照终端中可见的 Codex 星点在 Pebrel 不显示 | 核对真彩色能力、OSC 10/11 回答、盲文点字与前/背景色；真实 PTY 录制/回放；关闭动画时不强制产生动画 | 相同 Codex 可执行文件/模型/设置，对照终端；分别覆盖 low/high/xhigh/max/ultra、Max→xhigh 回退、持续星点与一次性等级动画；以 CLI 实际输出为准，不能由终端自行按等级添加/过滤星点；不能伪造 `WT_SESSION` 来冒充其他终端 | 完整 Windows 子环境漏真彩色声明已修复；8 项环境回归、10 项身份/WSL 透传回归及 15 主题 VT 回放通过；真实产品星点字形/背景截图已核对；已观察到 Codex 0.154.0 在 xhigh 持续输出变化的星点；其源码持续星点与 Max/Ultra 一次性动画分别控制；与对照终端的等级切换差异仍待验收 |
| CODEX-MESSAGE | **所有主题**的 Codex 用户消息与 AI 回复背景缺少区分 | 保留/正确映射应用给出的用户消息背景；未提供背景时不得凭屏幕文字猜测角色；消息文本对比度不退化 | 遍历主题注册表中的全部内置主题；输入框与已发送的单/多行、中英混排消息；深浅切换、滚动回看；用户消息应有可辨认浅色背景区 | 15 个内置主题 + 225 种主题切换的背景保留回归通过；真实产品 Nord / Paper 的背景和星点截图已核对；其余主题有自动回放覆盖，仍需原生视觉抽查 |
| SHELL-MENU | 多个 WSL 发行版把资源管理器右键菜单铺满 | 保留一个普通“在 Pebrel 中打开”；其余发行版集中到一个级联菜单；零/一/多个发行版、重复安装、移除发行版、卸载与旧项迁移 | 文件夹对象/文件夹空白处；中文/空格/盘根路径；点击普通项与每个子项验证 Shell 和 cwd；保留第三方/其他安装拥有的键 | 已实现；真实 Inno 夹具累计 117 项通过，完整安装器编译通过；正式 Explorer 点击验收仍需进行 |
| KEYMAP-F2 | 默认 F2 重命名标签与 Codex CLI 按键冲突 | 设置中的“重命名标签”可改绑、清除、恢复；旧键释放给终端，新键只重命名当前标签；持久化后仍有效，菜单提示与真实绑定一致；已有其他显式绑定保留 | 侧栏/顶部标签、中文/英文界面；打开重命名后取消；更改后立即使用与重启恢复；真实 Codex CLI 确认可收到 F2 | 12 项模型/GPUI 自动回归通过，覆盖真实设置行与 VT、Win32、disambiguate、report-all 输入模式；旧 shell 编译通过；完整原生 CLI 会话待验收 |
| SSH-LABEL | [PR #274](https://github.com/Kuddev/pebrel/pull/274)：Shell 选择器未展示 SSH 设置中的主机名称 | 名称与原始目标分列显示；名称/目标均可搜索；空名称回退为目标与 SSH；连接动作与图标仍使用原始目标，排序/隐藏/置顶语义不变 | 三个入口、中文与长名称、窄窗口；选择命名主机仍进入原目标；不把名称当作主机地址 | 行模型回归已覆盖名称、目标、搜索与启动动作；原 PR 原生矩阵通过；原生视觉与真实 SSH 连接待验收 |

## 修复与评审入口

- Hook 命令及退出期限：[PR #260](https://github.com/Kuddev/pebrel/pull/260)，关联 #254 / #255 及 #258 的前置占用故障。
- 同版本修复判定：[PR #261](https://github.com/Kuddev/pebrel/pull/261)，关联 #258。
- 下划线与路径边界：[PR #262](https://github.com/Kuddev/pebrel/pull/262)。
- Windows / WSL 真彩色能力：[PR #263](https://github.com/Kuddev/pebrel/pull/263)，关联 #136。
- WSL 级联菜单：[PR #264](https://github.com/Kuddev/pebrel/pull/264)。
- 可配置标签重命名：[PR #272](https://github.com/Kuddev/pebrel/pull/272)。
- macOS 链接手势：[PR #266](https://github.com/Kuddev/pebrel/pull/266)。
- 更新限流恢复与 macOS 安装：[PR #269](https://github.com/Kuddev/pebrel/pull/269)。
- SSH 主机名称：[PR #274](https://github.com/Kuddev/pebrel/pull/274)。

本地组合回归与各 PR 的 CI、Code Owner 评审是不同证据；未合并、未发布不能写成已经交付。

## 当前测试入口

```powershell
cargo test --locked -p nebula --bin pebrel --features gpui-shell rename
cargo test --locked -p nebula_hook
cargo test --locked -p nebula --bin pebrel --features gpui-shell platform::environment::tests
cargo test --locked -p nebula --bin pebrel --features gpui-shell ai_hook::
cargo test --locked -p nebula --bin pebrel --features gpui-shell display::hint::tests
cargo test --locked -p nebula --bin pebrel --features gpui-shell config::ui_config::tests
cargo test --locked -p nebula --bin pebrel --features gpui-shell gpui_shell::terminal::link_underline::tests
cargo test --locked -p nebula --bin pebrel --features gpui-shell gpui_shell::terminal::element::tests
cargo test --locked -p nebula --bin pebrel --features gpui-shell display::terminal_color::tests
```

安装器既有入口为 `scripts/tests/installer.tests.ps1`、
`scripts/tests/installer-migration.tests.ps1`、`scripts/tests/test_update_handoff.ps1`。
菜单夹具已扩展零/一/多个发行版及所有权保护；handoff 已扩展同版本修复正例与错误版本反例。它们不代替正式安装或 Explorer 点击验收。

修改完成后还需按 `CONTRIBUTING.md` 执行架构、格式、受影响模块与真实产品检查；
架构门禁使用实际目标分支基线。新增门禁或 GitHub 强制保护需单独验证，
本清单不宣称启用了服务端保护。
