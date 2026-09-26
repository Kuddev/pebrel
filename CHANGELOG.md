# Changelog / 更新日志

Every release entry is provided in English and Simplified Chinese.

每个版本条目均同时提供英文和简体中文说明。

## 1.9.1 - 2026-09-24

### English

#### Added

- Added in-app updates for packaged macOS applications on Apple Silicon and Intel. Verified updates can install after restart or on the next launch, then restore the saved workspace. Immediate relaunch failure rolls back to the original application. Installation requires a writable application location; copies running from a disk image or App Translocation must first be moved.
- Added SSH display names to the Shell picker, Quick Jump and command palette. Named hosts show their configured name alongside the original connection target; unnamed hosts keep the previous display. Search matches both the name and target, and selecting a row still connects to the original destination.
- Added a configurable tab-rename shortcut in Settings → Key Bindings → Tabs. The default remains F2; changing or clearing the shortcut lets F2 reach terminal applications such as Codex CLI. Changes apply without restarting, and menu shortcut hints follow the effective binding.

#### Fixed

- Fixed update checks failing when GitHub returns HTTP 403 or 429. The updater falls back to the official latest-release page and checksum manifest; if verified package metadata is unavailable, manual download remains available.
- Fixed macOS terminal links not opening with Command+left click. Link gestures also work when an application enables mouse reporting, while ordinary clicks still reach the application. Hover hints show the correct platform modifier in the selected language. Addresses [#196](https://github.com/Kuddev/pebrel/issues/196).
- Fixed Windows Codex Hook commands failing during PowerShell parsing, including executable paths with spaces or special characters. Existing Pebrel-managed commands are migrated while preserving user and third-party hooks. Addresses [#254](https://github.com/Kuddev/pebrel/issues/254) and [#255](https://github.com/Kuddev/pebrel/issues/255).
- Bounded Hook event forwarding when input remains open or the receiving application stops reading, so forwarding cannot wait indefinitely and keep the helper executable locked. Already running helpers from older versions do not acquire this timeout.
- Fixed successful same-version repair installations being reported as failures when the main executable remains byte-identical. Before setup starts, the updater briefly waits for installed Hook files to become writable; a persistent lock stops the update before setup can partially replace the installation. Installer success, package verification and the expected application version remain required. Addresses [#258](https://github.com/Kuddev/pebrel/issues/258).
- Fixed dashed link underlines covering only part of Chinese and other wide characters, and restarting their pattern at spaces or font boundaries. Underlines now follow terminal columns and display scaling.
- Fixed quoted Windows, UNC and home-directory paths containing spaces being recognized only up to the first space. Link highlighting and opening use the complete quoted path.
- Fixed WSL shell-prompt links including the trailing `$` or `#`, and absolute prompt paths losing their underline. Opening an absolute WSL prompt path uses the originating distribution; copying keeps the original path text.
- Fixed missing truecolor declarations in fresh Windows and WSL panes. Applications can retain their intended message-background colors across terminal themes, including Codex user-message backgrounds. Animation output remains controlled by the CLI; Pebrel does not add or filter stars according to reasoning level.
- Fixed tab context menus mixing hardcoded Chinese with the selected interface language. Menu labels now follow the active language across all 11 supported locales, including Korean, in both sidebar and top tabs.

#### Improved

- Organized Windows Explorer integration into one normal Pebrel open command and one WSL submenu containing the installed distributions. Installation migrates Pebrel-owned flat entries while preserving unrelated or user-edited commands.
- Expanded issue forms with operating-system, shell, pane-program and connection details, and clearer reproduction and evidence fields to make problem reports easier to investigate.

Windows x64 provides an installer and portable ZIP; Windows ARM64 provides a native portable ZIP. On Windows, the installer and automatic installation remain x64-only; packaged macOS applications support in-app installation. Linux x64 packages and macOS Apple Silicon / Intel DMGs remain Preview releases.

### 中文

#### 新增

- 为 Apple Silicon 和 Intel 上的 macOS 打包应用加入应用内更新：校验通过后，可选择重启安装或下次启动时安装，并恢复已保存的工作区；新应用立即重启失败时回滚到原应用。安装位置必须可写，从磁盘映像或 App Translocation 路径运行的副本需先移动到正常安装位置。
- 在 Shell 选择器、快速跳转和命令面板中显示 SSH 主机名称：已命名的主机显示名称与原始连接目标，未命名的主机保持原来的显示。搜索同时匹配名称和目标，选择后仍连接原始地址。
- 在“设置 → 按键映射 → 标签页”中新增可修改的“重命名标签”快捷键，默认仍为 F2。修改或清除后，F2 可传递给 Codex CLI 等终端程序；设置无需重启即可生效，菜单提示同步显示实际快捷键。

#### 修复

- 修复 GitHub 返回 HTTP 403 或 429 时检查更新失败的问题；改用官方最新版本页面与校验清单恢复查询，无法取得已验证安装包信息时仍可手动下载。
- 修复 macOS 终端链接无法通过 Command+左键打开的问题；终端应用启用鼠标上报后，链接手势仍可使用，普通点击继续传给应用。悬停提示按当前语言显示对应平台的修饰键。对应 [#196](https://github.com/Kuddev/pebrel/issues/196)。
- 修复 Windows Codex Hook 命令在 PowerShell 解析阶段失败的问题，正确处理程序路径中的空格和特殊字符；迁移 Pebrel 管理的旧命令，同时保留用户及第三方 Hook。对应 [#254](https://github.com/Kuddev/pebrel/issues/254)、[#255](https://github.com/Kuddev/pebrel/issues/255)。
- 为 Hook 事件转发增加等待期限，避免输入一直未关闭或接收端停止读取时无限等待并持续占用辅助程序文件。已经运行的旧版本 Hook 不会自动获得此超时机制。
- 修复同版本修复安装成功后，仅因主程序字节未改变而误报失败的问题。启动安装器前，会短暂等待已安装的 Hook 文件恢复可写；持续占用时提前停止，避免安装器只替换部分文件。仍要求安装器成功、安装包校验通过且最终应用版本正确。对应 [#258](https://github.com/Kuddev/pebrel/issues/258)。
- 修复中文及其他宽字符的链接虚线只覆盖半个字符，以及虚线在空格或字体边界处重新起算的问题；下划线按终端列和显示缩放连续绘制。
- 修复带引号的 Windows、UNC 和主目录路径包含空格时，只识别到第一个空格的问题；链接高亮与打开操作使用完整的引号内路径。
- 修复 WSL 提示符路径把末尾 `$`、`#` 包含进链接，以及绝对路径提示符缺少下划线的问题；打开 WSL 绝对提示符路径时使用所属发行版，复制时保留原始路径文本。
- 修复新建 Windows 和 WSL 面板缺少真彩色声明的问题，使应用能在不同终端主题下保留其指定的消息背景色，包括 Codex 用户消息背景。动画仍由 CLI 决定，Pebrel 不根据思考等级添加或过滤星点。
- 修复标签右键菜单在非中文界面中混用硬编码中文的问题；侧栏和顶部标签的菜单均跟随当前界面语言，覆盖包括韩语在内的全部 11 种支持语言。

#### 改进

- 整理 Windows 资源管理器右键菜单，保留一个普通 Pebrel 打开入口，并将已安装的 WSL 发行版集中到一个子菜单中；安装时迁移 Pebrel 自有的旧平铺入口，保留无关或经过用户编辑的命令。
- 完善问题反馈表单，补充操作系统、Shell、面板程序和连接方式等信息，并明确复现步骤与日志、截图要求，便于定位问题。

Windows x64 提供安装器和 ZIP 便携包，Windows ARM64 提供原生便携 ZIP；Windows 安装器和自动安装仍仅支持 x64；macOS 打包应用支持应用内安装。Linux x64 包及 macOS Apple Silicon／Intel DMG 继续标记为 Preview。

### Contributors

<a href="https://github.com/ZiChuanLan"><img src="https://github.com/ZiChuanLan.png?size=96" width="64" height="64" alt="@ZiChuanLan avatar"></a><a href="https://github.com/YinBuLiao"><img src="https://github.com/YinBuLiao.png?size=96" width="64" height="64" alt="@YinBuLiao avatar"></a><a href="https://github.com/Kuddev"><img src="https://github.com/Kuddev.png?size=96" width="64" height="64" alt="@Kuddev avatar"></a>

- **[@ZiChuanLan](https://github.com/ZiChuanLan)** — SSH host names in the Shell picker, Quick Jump and command palette. / Shell 选择器、快速跳转及命令面板中的 SSH 主机名称。([#274](https://github.com/Kuddev/pebrel/pull/274))
- **[@YinBuLiao](https://github.com/YinBuLiao)** — Tab-menu localization, macOS Command-click links, rate-limited update recovery and macOS installation. / 标签菜单多语言、macOS Command+点击链接、更新限流恢复及 macOS 安装。([#265](https://github.com/Kuddev/pebrel/pull/265), [#266](https://github.com/Kuddev/pebrel/pull/266), [#269](https://github.com/Kuddev/pebrel/pull/269))
- **[@Kuddev](https://github.com/Kuddev)** — Hook execution and lifetime, repair installation, terminal links and colors, Explorer integration, configurable tab renaming and issue reporting. / Hook 执行与生命周期、修复安装、终端链接和颜色、资源管理器集成、可配置的标签重命名及问题反馈。([#257](https://github.com/Kuddev/pebrel/pull/257), [#260](https://github.com/Kuddev/pebrel/pull/260), [#261](https://github.com/Kuddev/pebrel/pull/261), [#262](https://github.com/Kuddev/pebrel/pull/262), [#263](https://github.com/Kuddev/pebrel/pull/263), [#264](https://github.com/Kuddev/pebrel/pull/264), [#272](https://github.com/Kuddev/pebrel/pull/272))

---

**SHA256**

Checksums will be filled from the final release artifacts. / 校验值将在最终发布产物生成后填写。

- `Pebrel-v1.9.1-linux-x64-preview.AppImage`: `PENDING FINAL BUILD`
- `Pebrel-v1.9.1-linux-x64-preview.deb`: `PENDING FINAL BUILD`
- `Pebrel-v1.9.1-linux-x64-preview.tar.gz`: `PENDING FINAL BUILD`
- `Pebrel-v1.9.1-macos-arm64-preview.dmg`: `PENDING FINAL BUILD`
- `Pebrel-v1.9.1-macos-x64-preview.dmg`: `PENDING FINAL BUILD`
- `Pebrel-v1.9.1-windows-x64.zip`: `PENDING FINAL BUILD`
- `Pebrel-v1.9.1-windows-x64-setup.exe`: `PENDING FINAL BUILD`
- `Pebrel-v1.9.1-windows-arm64.zip`: `PENDING FINAL BUILD`

## 1.9.0 - 2026-09-22

### English

#### Added

- Added a native Windows ARM64 portable ZIP, alongside the Windows x64 installer and portable ZIP. ARM64 builds use a native runner and include matching ARM64 Hook and console runtime binaries.
- Added Kimi Code recognition, native session commands and managed lifecycle hooks, including its icon and terminal activity states.
- Added a dedicated Agents settings page for Claude Code, Codex, OpenCode, Cursor Agent, Kimi Code, Pi, Oh My Pi, GitHub Copilot and Grok. It shows detected CLI paths and actual Hook installation status beside each integration switch. Automatic Hook management is available on Windows.
- Added configurable notification duration: keep the existing defaults, choose 5, 10, 30 or 90 seconds, or keep notifications visible until dismissed. The selected duration takes precedence over an Agent's default banner lifetime.
- Added a persistent terminal font-ligature setting, enabled by default and applied to open sessions without restarting them. Addresses [#220](https://github.com/Kuddev/pebrel/issues/220).
- Added a Sponsors page under Settings → Home → Project and support, introducing Fluxion AI with its sign-up link.

#### Fixed

- Fixed Codex cold restoration using a Hook group identifier instead of the native conversation identity. Saved sessions with an older or unrecognized transcript filename retain their existing valid conversation ID.
- Fixed missing Windows local Codex identities by recovering the native conversation ID and file path from the running process. Later lifecycle-only events retain the verified identity.
- Fixed dead-end Codex restoration when a saved conversation no longer exists: recovery opens the native conversation chooser, including from CMD, and the tab menu offers an explicit conversation choice.
- Fixed the session crash-loop breaker counting a launch as failed while a restored AI conversation was still unconfirmed. Normal exits with a Codex conversation waiting at its startup prompts no longer quarantine the whole workspace after three launches, and the saved conversation target is retained for the next launch.
- Fixed shutdown getting stuck when some AI identities cannot be collected. Users can save the known state and exit; save errors and subsequently changed editor drafts still keep the application open.
- Fixed AI failures and cancellations appearing as successful completion. Claude failure events and Pi retry-aware outcomes now use the shared result handling.
- Fixed notification replacements retaining an old result or expiry deadline. A refreshed pane result uses the selected duration, and an older timer cannot dismiss its replacement.
- Fixed supported AI confirmation actions by checking that the request is still current and completing the required key-down/key-up sequence. Windows notification activation restores the corresponding application window.
- Fixed WSL Hook installation and event delivery to use the distribution's own user configuration. Command completions include the distribution and user options.
- Fixed inherited Ctrl+C suppression preventing interruption of CMD child commands, while preserving selection copying.
- Fixed native CMD command activity detection, multiline input and echoed text at pending-wrap boundaries.
- Fixed application 256-color blocks being overwritten by terminal prompt colors, while retaining explicit custom color overrides.
- Fixed saved SSH hosts ignoring the available system SSH agent in automatic authentication mode.
- Fixed terminal working-directory copying to use the path reported by the shell, including guest paths, while normal selection copying retains the original text.
- Fixed terminal and answer-reader opening of supported local Markdown links, file URIs and relative paths. Paths containing spaces, Chinese text or parentheses resolve against the originating pane's directory; missing paths produce visible feedback. Addresses [#203](https://github.com/Kuddev/pebrel/issues/203).
- Fixed file-editor selections and search matches blending into light and dark backgrounds. Plain-text and code editors reveal the configured workspace background, and the active line follows the theme.
- Fixed long Markdown outline headings overlapping adjacent rows. Full titles remain available through tooltips and copying. Addresses [#184](https://github.com/Kuddev/pebrel/issues/184).
- Fixed interrupted Markdown drags leaving a selection that continues to grow when the pointer moves without a pressed button.
- Fixed code-language menus extending beyond narrow windows and kept their search field and keyboard activation usable.
- Fixed SSH host icons appearing off-center because a font glyph's visible outline differs from its text advance.
- Removed the duplicate title preview shown while dragging a tab.

#### Improved

- Markdown opens in read-only rendered mode. Text selection, copying, code copying and formula actions remain available; editing requires an explicit switch to source mode.
- Code-block language selection remains available in the reader. It changes syntax highlighting without rewriting the Markdown, survives scrolling away and back, and leaves copied code unchanged.
- Kept source-mode drafts when returning to the reader, while preventing undo/redo shortcuts from editing the document in read-only mode.
- Bounded Markdown presentation work with virtual rows, visible-block image ownership and release of inactive document layouts. These changes reduce repeated work without imposing a machine-specific memory or speed promise.
- Reused repository snapshots and rendered visible Git/SVN drawer rows to reduce repeated work in large repositories.
- Centered the Agents page, enlarged all nine Agent marks using SVG assets, and separated CLI discovery from Hook installation status. Long paths remain accessible through tooltips.
- Improved settings search, SSH connection cards and filters, file-panel control alignment, and command-group navigation, ordering and add actions.
- Preserved other integrations and user-owned configuration when installing or removing a managed Agent Hook.
- Temporarily hid the backup entry from the settings sidebar and search; cloud-backup controls are not exposed in this release.

Windows x64 includes an installer and portable ZIP; Windows ARM64 includes a native portable ZIP. The installer and in-app automatic installation remain x64-only. Linux x64 packages and macOS Apple Silicon/Intel DMGs retain their Preview designation. macOS requires version 14 or later. Molecular structure rendering remains disabled.

### 中文

#### 新增

- 新增：Windows ARM64 原生便携 ZIP，与 Windows x64 安装器及便携 ZIP 一同提供。ARM64 使用原生运行器构建，并配套 ARM64 Hook 辅助程序与控制台运行时。
- 新增：Kimi Code 识别、原生会话命令和受管理的生命周期 Hook，包含品牌图标及终端活动状态。
- 新增：独立 Agents 设置页，覆盖 Claude Code、Codex、OpenCode、Cursor Agent、Kimi Code、Pi、Oh My Pi、GitHub Copilot 和 Grok。每项显示检测到的 CLI 路径，并在开关旁显示实际 Hook 安装状态；自动 Hook 管理适用于 Windows。
- 新增：通知停留时长设置：保持既有默认值，或选择 5、10、30、90 秒以及手动关闭。用户选择的时长优先于 Agent 自带的横幅默认时长。
- 新增：可持久保存的终端字体连字开关，默认开启，并可应用于已打开的会话，无需重启终端。对应 [#220](https://github.com/Kuddev/pebrel/issues/220)。
- 新增：设置首页「项目与支持」中的赞助商页面，介绍赞助商 Fluxion AI 并提供注册链接。

#### 修复

- 修复：Codex 冷恢复误用 Hook 分组标识而非原生对话身份的问题。旧快照中的会话文件名无法识别时，保留原有有效对话 ID。
- 修复：Windows 本地 Codex 身份缺失的问题，可从运行中的进程找回原生对话 ID 和文件路径；后续仅包含生命周期信息的事件会保留已核验身份。
- 修复：保存的 Codex 对话不存在时恢复流程无法继续的问题：包括 CMD 在内，可打开原生会话选择器，标签菜单也提供主动选择对话的入口。
- 修复：已恢复的 AI 对话尚未确认时，本次启动被会话断路器误计为失败的问题。Codex 对话停在启动提示上、正常退出三次后，不再把整个工作区隔离到 `session.crashed.json`；已保存的对话目标保留至下次启动。
- 修复：部分 AI 身份无法取得时关闭流程卡住的问题。用户可以保存已知状态并退出；保存失败或编辑器草稿随后发生变化时仍保持应用打开。
- 修复：AI 失败和取消事件被显示为成功完成的问题，Claude 失败事件及 Pi 重试后的结果统一进入通知结果处理。
- 修复：通知替换后残留旧结果或旧到期时间的问题。新的窗格结果遵循选定时长，旧计时器不能关闭替换后的通知。
- 修复：受支持 AI 确认动作：发送前检查请求仍然有效，并完成必要的按下、释放按键序列；Windows 通知激活时恢复对应应用窗口。
- 修复：WSL Hook 安装与事件传递，使用发行版自身的用户配置；命令补全同步包含发行版和用户选项。
- 修复：继承的 Ctrl+C 忽略状态导致 CMD 子命令无法中断的问题，同时保留选区复制。
- 修复：原生 CMD 命令活动检测、多行输入以及等待折行边界的回显文本跟踪。
- 修复：应用的 256 色块被终端提示符颜色覆盖的问题，保留明确配置的自定义颜色覆盖。
- 修复：已保存 SSH 主机在自动认证时忽略可用系统 SSH 代理的问题。
- 修复：终端工作目录复制，改为复制 Shell 上报的路径，包括访客环境路径；普通选区复制保留原始文字。
- 修复：终端和回答阅读页打开受支持的本地 Markdown 链接、文件 URI 及相对路径的行为。含空格、中文或括号的路径按原窗格目录解析，路径缺失时提供可见反馈。对应 [#203](https://github.com/Kuddev/pebrel/issues/203)。
- 修复：文件编辑器选区与查找命中融入浅色、深色背景的问题；普通文本和代码编辑器可透出已配置的工作区背景图，光标所在行跟随主题。
- 修复：Markdown 目录长标题压住相邻行的问题，完整标题仍可通过悬停提示和复制取得。对应 [#184](https://github.com/Kuddev/pebrel/issues/184)。
- 修复：Markdown 拖选被中断后，未按住鼠标移动仍继续扩大选区的问题。
- 修复：代码语言菜单在窄窗口中越界的问题，并保持搜索框和键盘操作可用。
- 修复：SSH 主机图标因字体可见轮廓与排版宽度不同而偏离中心的问题。
- 修复：移除拖动标签时重复出现的标题预览。

#### 改进

- 改进：Markdown 默认以只读方式渲染，保留文字选择、复制、代码复制及公式操作；编辑需要显式切换到源码模式。
- 改进：阅读模式仍可选择代码块语言，只改变语法高亮，不改写 Markdown；滚动离开再返回后保留选择，复制得到的代码内容不变。
- 改进：从源码模式返回阅读页时保留草稿，同时阻止只读模式下的撤销、重做快捷键修改文档。
- 改进：Markdown 使用虚拟列表、可见代码块和图片资源管理，并释放非活动文档的布局缓存，减少重复工作；不把单机结果写成固定的内存或速度承诺。
- 改进：Git／SVN 抽屉复用仓库快照，仅渲染可见条目，减少大型仓库中的重复工作。
- 改进：Agents 页面整体居中，九种 Agent 图标使用放大的 SVG 资源；CLI 是否存在与 Hook 是否安装分别展示，长路径可通过悬停提示查看。
- 改进：设置搜索、SSH 连接卡片和筛选、文件面板控件对齐，以及命令分组的导航、排序和新增操作。
- 改进：安装或移除受管理的 Agent Hook 时，保留其他集成及用户已有配置。
- 改进：暂时隐藏设置侧栏和搜索中的备份入口，本版不开放云备份控件。

Windows x64 提供安装器和 ZIP 便携包，Windows ARM64 提供原生便携 ZIP；安装器和应用内自动安装仍仅支持 x64。Linux x64 包及 macOS Apple Silicon／Intel DMG 继续标记为 Preview。macOS 最低版本为 14。分子结构渲染仍处于禁用状态。

### Contributors

<a href="https://github.com/Traveritas"><img src="https://github.com/Traveritas.png?size=96" width="64" height="64" alt="@Traveritas avatar"></a><a href="https://github.com/galact-byte"><img src="https://github.com/galact-byte.png?size=96" width="64" height="64" alt="@galact-byte avatar"></a><a href="https://github.com/821869798"><img src="https://github.com/821869798.png?size=96" width="64" height="64" alt="@821869798 avatar"></a><a href="https://github.com/tataraDM"><img src="https://github.com/tataraDM.png?size=96" width="64" height="64" alt="@tataraDM avatar"></a><a href="https://github.com/Aschenbath"><img src="https://github.com/Aschenbath.png?size=96" width="64" height="64" alt="@Aschenbath avatar"></a><a href="https://github.com/Kuddev"><img src="https://github.com/Kuddev.png?size=96" width="64" height="64" alt="@Kuddev avatar"></a>

- **[@Traveritas](https://github.com/Traveritas)** — Kimi Code hooks, recognition and branding. / Kimi Code Hook、识别和品牌图标。([#197](https://github.com/Kuddev/pebrel/pull/197))
- **[@galact-byte](https://github.com/galact-byte)** — CMD interrupts and activity, terminal application colors and Pi notification outcomes. / CMD 中断与活动、终端应用配色及 Pi 通知结果。([#206](https://github.com/Kuddev/pebrel/pull/206), [#207](https://github.com/Kuddev/pebrel/pull/207), [#208](https://github.com/Kuddev/pebrel/pull/208), [#215](https://github.com/Kuddev/pebrel/pull/215), [#216](https://github.com/Kuddev/pebrel/pull/216))
- **[@821869798](https://github.com/821869798)** — Markdown links and local file opening from terminals and the answer reader. / 终端及回答阅读页的 Markdown 链接和本地文件打开。([#204](https://github.com/Kuddev/pebrel/pull/204))
- **[@tataraDM](https://github.com/tataraDM)** — Readable single-line Markdown outline headings. / 修复 Markdown 目录长标题挤压。([#191](https://github.com/Kuddev/pebrel/pull/191))
- **[@Aschenbath](https://github.com/Aschenbath)** — Configurable notification duration and persistent notifications. / 可配置的通知停留时长和常驻通知。([#131](https://github.com/Kuddev/pebrel/pull/131))
- **[@Kuddev](https://github.com/Kuddev)** — Desktop integration, actionable notifications, session restoration, Markdown reader, Agents UI, review and release integration. / 桌面整合、可操作通知、会话恢复、Markdown 阅读、Agents 界面、审核及发布整合。([#210](https://github.com/Kuddev/pebrel/pull/210), [#217](https://github.com/Kuddev/pebrel/pull/217), [#221](https://github.com/Kuddev/pebrel/pull/221), [#222](https://github.com/Kuddev/pebrel/pull/222), [#225](https://github.com/Kuddev/pebrel/pull/225), [#227](https://github.com/Kuddev/pebrel/pull/227), [#231](https://github.com/Kuddev/pebrel/pull/231), [#232](https://github.com/Kuddev/pebrel/pull/232))

---

**SHA256**

- `Pebrel-v1.9.0-linux-x64-preview.AppImage`: `4da5ea590a82ab4f89dca14df1399e7c2cd27fa4fe308da6f0e95b8aa9ae7d22`
- `Pebrel-v1.9.0-linux-x64-preview.deb`: `bb6fff964942b750a794f2ea148e851719430dfffd9bb086a13e2f1e67d87722`
- `Pebrel-v1.9.0-linux-x64-preview.tar.gz`: `6526ed5f2f883bb5ca9600d2685c929056fbae9f2450259f34d65093a18deda6`
- `Pebrel-v1.9.0-macos-arm64-preview.dmg`: `daf2c6c381fbdf84c33c132e9c6c247e9f9dc6e86292cc5be855e635e6cfa6f3`
- `Pebrel-v1.9.0-macos-x64-preview.dmg`: `9f3f1465be9e1f3489be09ccf336d5bdc0a6ae40185062f677a6e857d47e1fe0`
- `Pebrel-v1.9.0-windows-x64.zip`: `1c219946450c7d93993394fd6d40c6e234d77547c9b4a98106c0d33ada9c27f9`
- `Pebrel-v1.9.0-windows-x64-setup.exe`: `52cefcdae9cd98035bcdd45944c020fc9acf9c2c497147ba337ed15069a07fe6`
- `Pebrel-v1.9.0-windows-arm64.zip`: `e2d062089afb19ddb9e3d078cc7f460a1d67cb36d95400cebab837f9fabda092`

## 1.8.2 - 2026-09-17

### English

#### Added

- Added Windows Explorer entries for opening a folder in a registered WSL distribution. Both a folder's context menu and the folder-background menu offer the distribution choices installed with Pebrel.
- Added `--shell <id>` to select a Shell or saved terminal profile for a new terminal. The choice reaches both a fresh launch and an already-running instance; an explicitly supplied directory overrides the profile's saved directory without changing the saved profile.
- Added persistent command groups, with drag-and-drop assignment and context-menu actions to move commands between groups or return them to Ungrouped. Deleting a group keeps its commands. This covers the grouping portion of the request; default-collapsed groups and automatic grouping by tool remain pending. Addresses [#149](https://github.com/Kuddev/nebula/issues/149).
- Added persistent deletion of built-in command recipes. Removed recipes stay hidden after reopening the command manager, including after changes from another window; saved user commands are retained.

#### Fixed

- Fixed WSL repeatedly printing `PROMPT_COMMAND` syntax errors at `__pebrel_connection() {` when Windows line endings entered embedded Shell scripts. Shared scripts are normalized before injection, preserving command exit codes and existing user SSH functions. Addresses [#188](https://github.com/Kuddev/nebula/issues/188).
- Fixed Git Bash leaving leading characters behind when editing or clearing wrapped input in byte-oriented locales. The managed prompt uses an ASCII marker when required and retains the Unicode marker in UTF-8 locales.
- Fixed premature completion notifications when the integration reports active background commands or monitors, which were previously missed by the background-work counter.
- Fixed hook-enabled AI sessions showing a completion toast when the screen was inferred to be idle while work was still running. Completion notifications now follow the integration’s turn events. Addresses [#162](https://github.com/Kuddev/nebula/issues/162).
- Fixed background operations briefly opening console windows on Windows, including process probes, Git operations and SSH helper commands.
- Fixed SSH terminals retaining their initial grid size after resizing a window or pane; the visible grid and remote terminal now receive the updated dimensions.
- Fixed "Reveal in file manager" opening the wrong folder when a Windows path contains spaces. The containing folder opens with the requested file selected. Addresses [#167](https://github.com/Kuddev/nebula/issues/167).
- Fixed moving the final tab to another window triggering a conflicting workspace update when the original window closes. Addresses [#150](https://github.com/Kuddev/nebula/issues/150).
- Fixed saved SSH connection names being replaced by remote directory or program titles. Both the sidebar and top-tab layouts retain the configured connection name. Addresses [#164](https://github.com/Kuddev/nebula/issues/164).
- Fixed Windows SSH configuration and identity-file paths containing spaces or quotes. Host discovery and connection resolution use the same user configuration file.
- Fixed SSH connection tests mishandling host-key confirmation and cancellation. Closing or editing the connection cancels the owned test, and an old result cannot overwrite the edited connection's status.
- Fixed the multiline command editor collapsing instead of retaining a usable editing height.
- Fixed Windows taskbar and relaunch icon properties and refreshed matching pinned shortcuts while preserving their launch arguments.

#### Improved

- Integrated keyboard-binding search into the global settings search, including action names and key combinations. Searching for the keyboard-settings section still shows the complete page.
- Made short, fixed settings choices directly visible as segmented buttons, including tab placement, new-tab position, display density and completion style.
- Expanded terminal-tab hover details to show the connection, working directory and current program or AI task without replacing the tab's stable name.
- Improved fallback handling for simple SSH configurations when the system SSH client cannot expand them. Advanced directives that require OpenSSH report a clear error instead of silently resolving an incomplete connection.
- Simplified the built-in recipes to selected Docker/Conda operations and Python/Git installation commands, while preserving saved user commands and persistent deletion choices.

Windows packages include a Pebrel installer and portable ZIP. Linux x64 packages and macOS Apple Silicon/Intel DMGs retain their Preview designation. macOS requires version 14 or later. Molecular structure rendering remains disabled.

### 中文

#### 新增

- 新增：Windows 资源管理器可选择已注册的 WSL 发行版打开目录；文件夹及文件夹空白处的右键菜单均提供随 Pebrel 安装注册的发行版选项。
- 新增：`--shell <id>` 可为新终端指定 Shell 或已保存的终端配置；首次启动和已运行实例都能接收该选择。本次明确指定的目录覆盖配置保存的目录，保存的配置本身保持不变。
- 新增：可持久保存的命令分组，支持拖动分配、右键移入其他分组或移回“未分组”；删除分组会保留其中的命令。本次完成分组部分，默认折叠及按工具自动分类仍待实现。对应 [#149](https://github.com/Kuddev/nebula/issues/149)。
- 新增：内置命令配方支持持久删除；重新打开命令管理器或从另一窗口修改后，被删除的配方仍保持隐藏，用户保存的命令予以保留。

#### 修复

- 修复：Windows 换行符混入内置 Shell 脚本后，WSL 在 `__pebrel_connection() {` 处反复输出 `PROMPT_COMMAND` 语法错误的问题。共享脚本在注入前统一换行，保留命令退出码及用户已有的 SSH 函数。对应 [#188](https://github.com/Kuddev/nebula/issues/188)。
- 修复：Git Bash 在按字节处理字符的语言环境中，编辑或清空折行输入后残留行首字符的问题；受管理的提示符在需要时使用 ASCII 标记，在 UTF-8 环境中保留 Unicode 标记。
- 修复：集成已上报正在运行的后台命令或监控任务，却因后台任务统计遗漏而提前弹出完成通知的问题。
- 修复：已启用 Hook 的 AI 会话仍在工作时，因屏幕被推断为空闲而弹出完成提示的问题；完成通知改为依据集成上报的回合事件。对应 [#162](https://github.com/Kuddev/nebula/issues/162)。
- 修复：Windows 后台操作短暂弹出控制台窗口的问题，覆盖进程探测、Git 操作及 SSH 辅助命令等路径。
- 修复：调整窗口或窗格尺寸后，SSH 终端仍保持初始网格大小的问题；可见网格与远端终端同步接收新的尺寸。
- 修复：Windows 路径含空格时，“在文件管理器中显示”打开错误目录的问题；现在会打开所在目录并选中目标文件。对应 [#167](https://github.com/Kuddev/nebula/issues/167)。
- 修复：将最后一个标签移动到另一窗口、原窗口随之关闭时触发工作区更新冲突的问题。对应 [#150](https://github.com/Kuddev/nebula/issues/150)。
- 修复：已保存的 SSH 连接名称被远端目录或程序标题替换的问题；侧栏和顶部标签布局均保留配置中的连接名称。对应 [#164](https://github.com/Kuddev/nebula/issues/164)。
- 修复：Windows SSH 配置及密钥文件路径含空格或引号时的处理问题；主机发现与连接解析使用同一份用户配置文件。
- 修复：SSH 连接测试中的主机密钥确认和取消处理；关闭或编辑连接会取消该测试，旧测试结果不能覆盖修改后连接的状态。
- 修复：多行命令编辑器收缩、无法保持可用编辑高度的问题。
- 修复：Windows 任务栏和重新启动入口的图标属性，并在保留启动参数的同时刷新匹配的已固定快捷方式。

#### 改进

- 改进：快捷键搜索统一接入设置页全局搜索，支持动作名称和按键组合；搜索快捷键设置分类时仍展示完整页面。
- 改进：选项较少且固定的设置直接显示为分段按钮，覆盖标签布局、新标签位置、显示密度和补全样式等选项。
- 改进：终端标签悬停提示展示连接、工作目录及当前程序或 AI 任务，同时保留稳定的标签名称。
- 改进：系统 SSH 客户端无法展开配置时，对简单 SSH 配置提供回退处理；需要 OpenSSH 的高级指令会明确报错，避免静默使用不完整的连接配置。
- 改进：将内置命令配方精简为选定的 Docker／Conda 操作及 Python／Git 安装命令，保留用户保存的命令和已持久保存的删除选择。

Windows 提供 Pebrel 安装器和 ZIP 便携包；Linux x64 包以及 macOS Apple Silicon／Intel DMG 继续标记为 Preview。macOS 最低运行版本为 14。分子结构渲染仍处于禁用状态。

### Contributors

<a href="https://github.com/AnxForever"><img src="https://github.com/AnxForever.png?size=96" width="64" height="64" alt="@AnxForever avatar"></a><a href="https://github.com/AlanWanco"><img src="https://github.com/AlanWanco.png?size=96" width="64" height="64" alt="@AlanWanco avatar"></a><a href="https://github.com/Traveritas"><img src="https://github.com/Traveritas.png?size=96" width="64" height="64" alt="@Traveritas avatar"></a><a href="https://github.com/Kuddev"><img src="https://github.com/Kuddev.png?size=96" width="64" height="64" alt="@Kuddev avatar"></a>

- **[@AnxForever](https://github.com/AnxForever)** — Fixed premature notifications and background console popups; added Shell selection and WSL Explorer entries. / 修复通知提前触发与后台控制台弹窗，新增 Shell 选择及 WSL 资源管理器入口。([#142](https://github.com/Kuddev/pebrel/pull/142), [#143](https://github.com/Kuddev/pebrel/pull/143), [#145](https://github.com/Kuddev/pebrel/pull/145))
- **[@AlanWanco](https://github.com/AlanWanco)** — Kept the SSH terminal grid and remote PTY synchronized on resize. / 修复调整尺寸后 SSH 终端网格与远端 PTY 不同步的问题。([#166](https://github.com/Kuddev/pebrel/pull/166))
- **[@Traveritas](https://github.com/Traveritas)** — Fixed revealing files whose Windows paths contain spaces. / 修复 Windows 路径含空格时无法正确定位文件的问题。([#168](https://github.com/Kuddev/pebrel/pull/168))
- **[@Kuddev](https://github.com/Kuddev)** — Fixed tab, SSH, Git Bash and WSL issues; improved command management and settings, and completed review and release integration. / 修复标签、SSH、Git Bash 与 WSL 问题，改进命令管理和设置，并完成审核与发布整合。([#171](https://github.com/Kuddev/pebrel/pull/171), [#173](https://github.com/Kuddev/pebrel/pull/173), [#175](https://github.com/Kuddev/pebrel/pull/175), [#183](https://github.com/Kuddev/pebrel/pull/183), [#189](https://github.com/Kuddev/pebrel/pull/189))

---

**SHA256**

- `Pebrel-v1.8.2-windows-x64.zip`: `9a1957a337b7747ba9a47ddf7be40d159e5fd33ab6b57efc6e1643bab5360ade`
- `Pebrel-v1.8.2-windows-x64-setup.exe`: `8c72f4921e663a70b2e9c2aa142388301396263f77b3ff4841ce096b75ad74d4`
- `Pebrel-v1.8.2-linux-x64-preview.AppImage`: `44da90755aa94734c434e00232f8ad17b5d7ebca1fe6ef898702629742272e61`
- `Pebrel-v1.8.2-linux-x64-preview.deb`: `3506a149ed0893ebb25e9a56b327f33187e3321f617594beda9a9fea4839af65`
- `Pebrel-v1.8.2-linux-x64-preview.tar.gz`: `304dcd53067493a7a82b34731ecdd2afc376e936a81b4671b9078ab465f334ae`
- `Pebrel-v1.8.2-macos-arm64-preview.dmg`: `e54bea2d9b30ec2f46dfd3dbf798ab45de6c4bab3f15a880da16c91758a6f3b4`
- `Pebrel-v1.8.2-macos-x64-preview.dmg`: `ca7e90b6c3dee0b9d3f844e2e2f1eb517dbaa6b5b5ef5ac9ea0b1fd598b8d14a`

## 1.8.1 - 2026-09-15

### English

#### Added

- Added seven scrollback limits: 1,000 / 2,000 / 5,000 / 10,000 / 20,000 / 50,000 / 100,000 lines. The default remains 10,000; changes apply to new terminals without truncating open sessions.
- Added a wheel-speed control from 0.25× to 4.00×, defaulting to 1.00×. Dragging previews the speed and releasing saves it; trackpad pixel scrolling, font zoom and completion-list scrolling keep their existing behavior.
- Added a custom theme library with saved snapshots, a preview editor, and theme import/export. Saved themes can retain terminal colors, typography and window appearance alongside the built-in themes.
- Added optional background update downloads and an option to install a verified download on the next application launch on Windows.

#### Fixed

- Fixed command history and completion candidates leaking between the parent shell and typed SSH/WSL connections. Integrated shell reports switch the active context and restore it on return; native PowerShell predictions retain the user's configuration.
- Restored the default terminal font to the exact Maple Mono Normal NF CN font bundled with 1.7.0 after the default changed in 1.8.0. Explicitly selected custom fonts remain available. Addresses [#148](https://github.com/Kuddev/nebula/issues/148).
- Fixed update checks and installer downloads ignoring the application's proxy configuration, including proxy exclusions and SOCKS connections.
- Fixed scheduled updates merging saved windows into one window. Update recovery retains each window's tabs and active tab; this does not change desktop position or window-size restoration.
- Fixed the Windows installer failing to reuse its registered installation directory when updating an existing installation.
- Restored the 1.7 tab-status alignment and prevented status glyphs from being clipped.

#### Improved

- Reduced unused scrollback allocations and reclaimed oversized row capacity when narrowing terminals, while preserving retained history and content. Large width reductions can take longer during synchronous reclamation; savings depend on the workload.
- Improved Pi conversation recovery by retaining the native session ID and session file together with each pane's shell and working directory. Failed restoration keeps the original target available for retry instead of treating command submission as successful recovery. Addresses [#146](https://github.com/Kuddev/nebula/issues/146).
- Improved Windows update handoff: save workspace state before exit, wait for the old process to stop before installing, verify the installer, and retain failure details when installation or restart does not complete.
- Improved cleanup of terminals on Windows when panes close, including console resources and process-monitoring handles; retained font ownership while text rendering still needs it.

Windows packages include a Pebrel installer and portable ZIP. Linux x64 packages and macOS Apple Silicon/Intel DMGs retain their Preview designation. macOS requires version 14 or later. Molecular structure rendering remains disabled.

### 中文

#### 新增

- 新增：七档回滚行数：1,000 / 2,000 / 5,000 / 10,000 / 20,000 / 50,000 / 100,000。默认仍为 10,000，仅影响新建终端，不截断已打开会话的历史。
- 新增：0.25×～4.00× 滚轮速度控制，默认 1.00×；拖动即时预览，松手保存。触控板像素滚动、字体缩放和补全列表滚动保持原有行为。
- 新增：自定义主题库，支持保存主题快照、预览编辑和主题导入导出；可在内置主题之外保存终端配色、字体排版及窗口外观。
- 新增：Windows 可选的后台更新下载，以及在下次启动应用时安装已校验下载包的选项。

#### 修复

- 修复：父 Shell 与手动进入的 SSH／WSL 连接之间混用命令历史和补全候选的问题；根据集成 Shell 的回执切换当前环境，返回后恢复父环境，并保留用户配置的 PowerShell 原生预测。
- 修复：将 1.8.0 更换后的默认终端字体恢复为与 1.7.0 完全相同的内置 Maple Mono Normal NF CN；用户明确选择的自定义字体仍可使用。对应 [#148](https://github.com/Kuddev/nebula/issues/148)。
- 修复：检查更新和下载安装器时忽略应用代理配置的问题，包括代理排除规则与 SOCKS 连接。
- 修复：计划更新将已保存的多个窗口合并成一个窗口的问题；更新恢复时保留各窗口的标签及活动标签，不改变桌面位置或窗口尺寸的恢复行为。
- 修复：Windows 安装器更新已有安装时未复用已注册安装目录的问题。
- 修复：恢复 1.7 的标签状态对齐方式，避免状态符号被裁切。

#### 改进

- 改进：减少闲置回滚行的预分配，并在缩窄终端时回收过大的行容量，保留历史行数及内容；大幅缩列时同步回收可能增加耗时，节省量取决于实际负载。
- 改进：Pi 会话恢复同时保留原生会话 ID、会话文件，以及各窗格的 Shell 和工作目录。恢复失败时保留原目标以便重试，不再仅凭恢复命令已提交就视为恢复成功。对应 [#146](https://github.com/Kuddev/nebula/issues/146)。
- 改进：Windows 更新交接在退出前保存工作区，等待旧进程退出后再安装，校验安装包，并在安装或重启未完成时保留失败详情。
- 改进：Windows 关闭窗格时释放控制台资源及进程监测句柄，并在文字渲染仍需使用字体时保持其有效。

Windows 提供 Pebrel 安装器和 ZIP 便携包；Linux x64 包以及 macOS Apple Silicon／Intel DMG 继续标记为 Preview。macOS 最低运行版本为 14。分子结构渲染仍处于禁用状态。

---

**SHA256**

- `Pebrel-v1.8.1-windows-x64.zip`: `4e6315d47c4b8d1db612a7ada0231c88b570dfd7dd1faec56bd498a8a2face18`
- `Pebrel-v1.8.1-windows-x64-setup.exe`: `1b94feebd50d135fa6e7330830659dc38457b4f5b4f6216eeabd64a1e415eccf`
- `Pebrel-v1.8.1-linux-x64-preview.AppImage`: `7b098a1806e20c22c6d34a848be26207108e9ceae9d15522b8ae0b2319c15aca`
- `Pebrel-v1.8.1-linux-x64-preview.deb`: `0b218ceda32e96f92eadec33e166211222b71faf4661cf54a4b8824789f7a18d`
- `Pebrel-v1.8.1-linux-x64-preview.tar.gz`: `26b5e01f543a014cf6250fc7dbfc8db9d19c38e5549f0c433b03637dc6550ea0`
- `Pebrel-v1.8.1-macos-arm64-preview.dmg`: `e1f0858bb507185fd4903ce2ca8308cf88d7e07172357c3f33b1f246a3d35c7f`
- `Pebrel-v1.8.1-macos-x64-preview.dmg`: `5b2dcd220a958646b04c8a6425f314ed58505ec4a015d141454cf23f75617de4`

## 1.8.0 - 2026-09-14

### English

#### Added

- Added confirmation before restoring default settings, with Cancel, Escape and backdrop dismissal available before any preferences change. Contributed by [@WilliamWang1721](https://github.com/WilliamWang1721) in [#115](https://github.com/Kuddev/pebrel/pull/115).
- Added Windows login-startup and silent-start switches in Settings → Advanced → Session lifecycle. Silent startup enables the tray so the window can be reopened; an explicit directory launch still opens a visible window. Contributed by [@WilliamWang1721](https://github.com/WilliamWang1721) in [#117](https://github.com/Kuddev/pebrel/pull/117).
- Added an independent interface text-size control, adjustable from 10 to 24 px without changing terminal text size. Contributed by [@WilliamWang1721](https://github.com/WilliamWang1721) in [#127](https://github.com/Kuddev/pebrel/pull/127).
- Added a separate terminal font fallback list for Chinese and other full-width characters. Font choices apply on Enter or leaving the field; the English and Chinese fields both default to bundled Maple Mono NF CN. Contributed by [@WilliamWang1721](https://github.com/WilliamWang1721) in [#127](https://github.com/Kuddev/pebrel/pull/127).
- Added “Focus follows mouse” in Settings → Interaction, off by default, and “Dim inactive panes” in Settings → Appearance, on by default. Both preferences take effect immediately.
- Added right-click actions to the Shell launcher for choosing the default Shell and, on Windows, opening the selected Shell inside a new administrator Pebrel window.
- Added Connect, Edit, and Delete to SSH launcher context menus. Delete uses the existing confirmation and undo flow.
- Added Trae CLI recognition and its tab icon, plus a dedicated Oh My Pi icon.
- Added CodeBuddy CLI recognition and its color tab icon, including the `codebuddy`, `cbc`, `codebuddy-code` and `codebuddy-lowmem` commands. Prewarm commands and old welcome text alone do not establish a foreground AI session.

#### Fixed

- Fixed font candidates being clipped near the window edge or covering their input after settings-page scrolling. The list opens above the field when needed, and both terminal-font fields align and display the current font.
- Fixed a bare `pebrel` launch opening a new terminal in the wrong directory. The inherited launch directory is preserved, including when handing the request to an existing Windows instance.
- Fixed restored WSL panes losing their saved guest directory during startup.
- Fixed queued AI resume commands and session identities being lost during Shell initialization. Compact Codex resume screens are recognized without another AI message, and exiting the CLI clears the foreground identity.
- Fixed long WSL directory titles overlapping distribution labels in the sidebar and top tabs.
- Fixed quiet terminals retaining an intermediate startup size. The final viewport size reaches the terminal even when no more output arrives.
- Fixed Windows terminal focus consuming `Alt+F4` and `Alt+Space`. `Alt+F4` now uses the normal window-close flow, and `Alt+Space`, followed by `N`, uses the system menu to minimize the window.
- Fixed the top Settings tab disappearing when switching to another tab. It now remains available until explicitly closed, including when tabs overflow or the tab layout changes.
- Fixed `Alt+1–9` and `Ctrl+1–9` tab shortcuts losing priority to terminal input. They now work while a terminal or CLI has focus and respect custom shortcut changes without restarting.
- Fixed WSL filename search failing when the directory path contains spaces or shell punctuation. Cancelling a search stops its own directory scan.
- Aligned macOS native window controls with the sidebar and Settings buttons, and preserved their reserved space in the top tab layout. Window controls follow the system appearance. Contributed by [@WilliamWang1721](https://github.com/WilliamWang1721) in [#114](https://github.com/Kuddev/pebrel/pull/114).
- Fixed the Git drawer retaining Chinese labels when the application resolves to English. Controls, history timestamps and application notices follow the selected language; switching languages preserves the commit draft and text selection. Contributed by [@gao-jian-bin](https://github.com/gao-jian-bin) in [#118](https://github.com/Kuddev/pebrel/pull/118).
- Fixed explicitly configured terminal selection text colors being ignored. Themes without their own selection foreground preserve that setting; themes with an explicit foreground retain their precedence. Contributed by [@Aschenbath](https://github.com/Aschenbath) in [#125](https://github.com/Kuddev/pebrel/pull/125).
- Fixed selected text disappearing behind opaque highlights in the answer reader and other selectable text views. Text remains visible while list and sidebar selection backgrounds retain their existing appearance. Contributed by [@Aschenbath](https://github.com/Aschenbath) in [#126](https://github.com/Kuddev/pebrel/pull/126).

#### Improved

- Reduced file browsing and filename-search memory use by starting recursive searches only when a query is entered, reusing a bounded cache and streaming paths beyond that cache. Repeated panel use releases obsolete results and cancels superseded searches. Case, whole-word and regular-expression options remain available, with visible feedback for incomplete searches and errors.
- Reduced background-image memory use with bounded image loading and reuse while resizing the window. Replacing an image releases the previous resources; extended backgrounds retain their fit, alignment and text readability.
- Replaced the low-resolution Claude Code icon with artwork exported from SVG at 1024 pixels, and prepared Agent icons at their display size for smoother tab edges.
- Restored monospace lettering for terminal directory labels, sidebar headers, tabs and split-pane titles while retaining the independent interface text-size setting.

Windows packages include a Pebrel installer and portable ZIP. Linux x64 packages and macOS Apple Silicon/Intel DMGs retain their Preview designation. macOS requires version 14 or later. Molecular structure rendering remains disabled.

### 中文

#### 新增

- 新增：恢复默认设置前的确认弹窗，可通过“取消”、Esc 或点击遮罩退出，确认前不会修改偏好。由 [@WilliamWang1721](https://github.com/WilliamWang1721) 在 [#115](https://github.com/Kuddev/pebrel/pull/115) 中贡献。
- 新增：Windows“设置 → 高级 → 会话生命周期”中的登录自启动和静默启动开关。静默启动会启用托盘以便重新打开窗口；显式打开目录时仍显示窗口。由 [@WilliamWang1721](https://github.com/WilliamWang1721) 在 [#117](https://github.com/Kuddev/pebrel/pull/117) 中贡献。
- 新增：独立界面字号设置，可在 10–24 px 间调整，不影响终端文字字号。由 [@WilliamWang1721](https://github.com/WilliamWang1721) 在 [#127](https://github.com/Kuddev/pebrel/pull/127) 中贡献。
- 新增：中文及其他全宽字符的独立终端字体回退列表。按回车或离开输入框后生效，中英文两项默认均采用内置 Maple Mono NF CN。由 [@WilliamWang1721](https://github.com/WilliamWang1721) 在 [#127](https://github.com/Kuddev/pebrel/pull/127) 中贡献。
- 新增：在“设置 → 交互”中加入“焦点跟随鼠标”，默认关闭；在“设置 → 外观”中加入“调暗非活动窗格”，默认开启。两项设置均即时生效。
- 新增：Shell 启动菜单新增右键操作，可设为默认 Shell；Windows 下还可在新的管理员 Pebrel 窗口内打开所选 Shell。
- 新增：SSH 启动菜单新增“连接、编辑、删除”右键操作；删除沿用已有的确认与撤销流程。
- 新增：Trae CLI 识别与标签图标，并为 Oh My Pi 加入独立图标。
- 新增：CodeBuddy CLI 识别与彩色标签图标，支持 `codebuddy`、`cbc`、`codebuddy-code`、`codebuddy-lowmem` 命令；预热命令和单独的历史欢迎文字不会被认定为前台 AI 会话。

#### 修复

- 修复：字体候选列表在窗口边缘被裁切、或设置页滚动后遮住输入框的问题。空间不足时向上展开，中英文终端字体输入框保持对齐并显示当前字体。
- 修复：不带目录参数运行 `pebrel` 时，新终端未继承实际启动目录的问题；交接给已有 Windows 实例时也会保留目录。
- 修复：WSL 窗格恢复时，在启动过程中丢失已保存的来宾目录的问题。
- 修复：Shell 初始化期间丢失排队的 AI 恢复命令和会话身份的问题。可识别 Codex 精简恢复界面，无需再发送 AI 消息；CLI 退出后清除前台身份。
- 修复：WSL 长目录标题与发行版名称在侧栏、顶部标签中互相覆盖的问题。
- 修复：安静终端停留在启动中间尺寸的问题；即使没有新的输出，也会收到布局稳定后的最终视口尺寸。
- 修复：Windows 终端获得焦点时吞掉 `Alt+F4` 和 `Alt+Space` 的问题。`Alt+F4` 现在进入正常窗口关闭流程，`Alt+Space` 后按 `N` 可通过系统菜单最小化窗口。
- 修复：顶部设置标签在切换到其他标签后消失的问题。现在只有主动关闭才会移除，标签溢出或切换标签布局时也会保留。
- 修复：`Alt+1–9` 和 `Ctrl+1–9` 标签快捷键优先级低于终端输入的问题。终端或 CLI 获得焦点时仍可切换标签，自定义快捷键的修改也无需重启即可生效。
- 修复：目录路径含空格或 Shell 特殊符号时 WSL 文件名搜索失败的问题；取消搜索会停止该次目录扫描。
- 修复：对齐 macOS 原生窗口按钮与侧栏、设置按钮，并在顶部标签布局中保留窗口按钮所需空间；窗口按钮外观跟随系统。由 [@WilliamWang1721](https://github.com/WilliamWang1721) 在 [#114](https://github.com/Kuddev/pebrel/pull/114) 中贡献。
- 修复：应用解析为英文时 Git 面板仍保留中文标签的问题。控件、历史时间和应用提示跟随所选语言，切换语言时保留提交草稿和文字选区。由 [@gao-jian-bin](https://github.com/gao-jian-bin) 在 [#118](https://github.com/Kuddev/pebrel/pull/118) 中贡献。
- 修复：终端忽略用户明确设置的选中文字颜色的问题。未指定选中文字颜色的主题会保留该设置；已明确指定的主题仍保持自身优先级。由 [@Aschenbath](https://github.com/Aschenbath) 在 [#125](https://github.com/Kuddev/pebrel/pull/125) 中贡献。
- 修复：回答阅读器等文本视图中，选中文字被不透明高亮遮住的问题。文字保持可读，列表与侧栏的选中背景保留原有外观。由 [@Aschenbath](https://github.com/Aschenbath) 在 [#126](https://github.com/Kuddev/pebrel/pull/126) 中贡献。

#### 改进

- 改进：降低文件浏览与文件名搜索的内存占用：输入关键词后才开始递归搜索，复用有容量上限的缓存，并流式查找缓存之外的路径。重复使用文件面板时释放过期结果、取消已被替代的搜索；保留大小写、整词和正则选项，并明确提示搜索未覆盖全部文件或发生错误。
- 改进：降低背景图内存占用，限制图片加载成本，并在调整窗口大小时复用图像资源；替换背景图会释放旧资源，扩展背景仍保持原有的适配、对齐和文字可读性。
- 改进：将 Claude Code 的低分辨率图标替换为从 SVG 导出的 1024 像素图像，并按显示尺寸处理 Agent 图标，使标签图标边缘更平滑。
- 改进：终端目录标签、侧栏标题、标签页及分屏标题恢复等宽字体，同时继续使用独立的界面字号设置。

Windows 提供 Pebrel 安装器和 ZIP 便携包；Linux x64 包以及 macOS Apple Silicon／Intel DMG 继续标记为 Preview。macOS 最低运行版本为 14。分子结构渲染仍处于禁用状态。

### Contributors

<a href="https://github.com/WilliamWang1721"><img src="https://github.com/WilliamWang1721.png?size=96" width="64" height="64" alt="@WilliamWang1721 avatar"></a><a href="https://github.com/gao-jian-bin"><img src="https://github.com/gao-jian-bin.png?size=96" width="64" height="64" alt="@gao-jian-bin avatar"></a><a href="https://github.com/Aschenbath"><img src="https://github.com/Aschenbath.png?size=96" width="64" height="64" alt="@Aschenbath avatar"></a>

**[@WilliamWang1721](https://github.com/WilliamWang1721)** - Improved macOS window controls, reset confirmation, Windows startup settings and independent interface/terminal font controls. / 改进 macOS 窗口按钮、恢复默认确认、Windows 启动设置，以及界面字号和终端字体设置。（[#114](https://github.com/Kuddev/pebrel/pull/114)、[#115](https://github.com/Kuddev/pebrel/pull/115)、[#117](https://github.com/Kuddev/pebrel/pull/117)、[#127](https://github.com/Kuddev/pebrel/pull/127)）

**[@gao-jian-bin](https://github.com/gao-jian-bin)** - Made Git drawer language follow application preferences while preserving editing state. / 让 Git 面板语言跟随应用设置，同时保留编辑状态。（[#118](https://github.com/Kuddev/pebrel/pull/118)）

**[@Aschenbath](https://github.com/Aschenbath)** - Preserved terminal selection foreground colors and readable text selections in the answer reader. / 修正终端选中文字颜色，并让回答阅读器的文字选区保持可读。（[#125](https://github.com/Kuddev/pebrel/pull/125)、[#126](https://github.com/Kuddev/pebrel/pull/126)）


---

**SHA256**

- `Pebrel-v1.8.0-windows-x64.zip`: `PENDING FINAL BUILD`
- `Pebrel-v1.8.0-windows-x64-setup.exe`: `PENDING FINAL BUILD`
- `Pebrel-v1.8.0-linux-x64-preview.AppImage`: `PENDING FINAL BUILD`
- `Pebrel-v1.8.0-linux-x64-preview.deb`: `PENDING FINAL BUILD`
- `Pebrel-v1.8.0-linux-x64-preview.tar.gz`: `PENDING FINAL BUILD`
- `Pebrel-v1.8.0-macos-arm64-preview.dmg`: `PENDING FINAL BUILD`
- `Pebrel-v1.8.0-macos-x64-preview.dmg`: `PENDING FINAL BUILD`

## 1.7.0 - 2026-09-12

### English

#### Added

- Added an AI message toast switch in Settings → Terminal → Alerts. Turning it off dismisses in-app AI cards and suppresses new ones, while retaining system notifications and tab status indicators. Addresses [#107](https://github.com/Kuddev/nebula/issues/107).
- Added editable local and SSH text documents with save operations, changed-file checks, and preservation of unsaved text when a save fails.
- Added reusable terminal layouts that open in a separate window and retain terminal launch settings without replaying command history.
- Added dragging files from the system file manager into local and WSL terminals to insert quoted paths without executing them. Local files for SSH sessions can be uploaded through Remote Files. Addresses [#106](https://github.com/Kuddev/nebula/issues/106).
- Expanded the built-in theme picker to 15 palettes. New Catppuccin Frappé and Macchiato join Latte and Mocha to complete all four flavors, alongside Breeze, Mint, and Glass light/dark variants. Glass uses static colors and does not automatically enable background blur.

#### Fixed

- Fixed rounded terminal box corners joining straight strokes one physical pixel off at some character sizes and display scales.
- Prevented Grok's Windows legacy-console fallback from hiding its dotted welcome logo in newly opened local terminals, while respecting explicit legacy-console overrides.
- Fixed the Default Shell setting and the new-terminal menu recommending PowerShell on macOS when no shell is configured. Both now use the same host-default resolver as the terminal backend. Contributed by [@YinBuLiao](https://github.com/YinBuLiao) in [#110](https://github.com/Kuddev/pebrel/pull/110).
- Fixed mixed shell icons giving settings-menu rows different heights. Shells without a brand image also receive an icon in the new-terminal menu. Contributed by [@YinBuLiao](https://github.com/YinBuLiao) in [#111](https://github.com/Kuddev/pebrel/pull/111).
- Shortened long and multi-line notification previews while retaining the original notification text in logs. Plain persistent notifications can be dismissed by clicking the card. Contributed in part by [@YinBuLiao](https://github.com/YinBuLiao) in [#112](https://github.com/Kuddev/pebrel/pull/112).
- Improved foreground contrast for terminal text on light backgrounds while preserving color pairs that are already readable. Addresses [#104](https://github.com/Kuddev/nebula/issues/104).
- Fixed the System language setting falling back to English when macOS applications launched from Finder have no locale environment variables.
- Corrected Antigravity status detection so ordinary replies do not look like pending confirmations, while active question and file-review prompts are recognized as waiting for input. Addresses [#100](https://github.com/Kuddev/nebula/issues/100). Contributed by [@821869798](https://github.com/821869798) in [#101](https://github.com/Kuddev/pebrel/pull/101).

#### Improved

- Changed the default appearance to 100% opacity with background blur off, so themes use solid backgrounds. Explicit opacity and blur choices are preserved.
- Made default sidebar and file-pane separators stay one physical pixel wide at high display scaling, retaining each theme's divider color.
- Improved Markdown reading with bounded preview work and shared background formula rendering; unsupported or oversized expressions retain their source.
- Added in-place feedback to code and formula copy controls, and kept document focus mode separate from operating-system fullscreen.
- Made Git-ignored files and directories distinguishable with italic text in the file tree.

Windows packages include a Pebrel installer and portable ZIP. The old-name installer alias is no longer published; older clients that require it can update by downloading the Pebrel installer from the Release page. Linux x64 packages and macOS Apple Silicon/Intel DMGs retain their Preview designation. Molecular structure rendering remains disabled.

### 中文

#### 新增

- 在“设置 → 终端 → 提醒”中新增“AI 消息弹窗”开关。关闭后收起已有的应用内 AI 卡片并停止显示新的卡片，同时保留系统通知和标签状态提示。对应 [#107](https://github.com/Kuddev/nebula/issues/107)。
- 新增本地和 SSH 文本文档编辑，支持保存、文件外部变更检查，并在保存失败时保留未保存的内容。
- 新增可复用的终端布局，在独立窗口中打开并保留终端启动设置，不重放命令历史。
- 新增从系统文件管理器向本地和 WSL 终端拖入文件，插入加引号的路径而不执行；SSH 会话所需的本地文件可通过“远程文件”上传。对应 [#106](https://github.com/Kuddev/nebula/issues/106)。
- 将内置主题选择器扩展为 15 套配色。新增 Catppuccin Frappé 和 Macchiato，与已有的 Latte、Mocha 补齐四款配色，同时包含 Breeze、Mint 及 Glass 浅色／深色主题。Glass 使用静态配色，不会自动开启背景模糊。

#### 修复

- 修复终端圆角框线在部分字符尺寸和显示缩放下，与相邻直线错开一个物理像素的问题。
- 避免 Grok 在新开的 Windows 本地终端中因错误启用旧控制台模式而隐藏点阵欢迎图案，同时保留用户显式设置的旧控制台覆盖值。
- 修复 macOS 未配置 Shell 时，“默认 Shell”设置和新建终端菜单推荐 PowerShell 的问题。两处现在与终端后端共用宿主默认 Shell 解析。由 [@YinBuLiao](https://github.com/YinBuLiao) 在 [#110](https://github.com/Kuddev/pebrel/pull/110) 中贡献。
- 修复不同 Shell 图标使设置菜单行高不一致的问题；没有品牌图片的 Shell 在新建终端菜单中也会显示图标。由 [@YinBuLiao](https://github.com/YinBuLiao) 在 [#111](https://github.com/Kuddev/pebrel/pull/111) 中贡献。
- 缩短长通知和多行通知的预览，同时在日志中保留通知原文。普通驻留通知可点击卡片关闭。部分修复由 [@YinBuLiao](https://github.com/YinBuLiao) 在 [#112](https://github.com/Kuddev/pebrel/pull/112) 中贡献。
- 改进浅色终端背景上的文字对比度，同时保留原本可读的配色。对应 [#104](https://github.com/Kuddev/nebula/issues/104)。
- 修复 macOS 从 Finder 启动、缺少语言环境变量时，“跟随系统”回退到英文的问题。
- 修正 Antigravity 状态识别：普通回答不再误报为等待确认，实际的问题选择框和文件审核框能识别为等待输入。对应 [#100](https://github.com/Kuddev/nebula/issues/100)。由 [@821869798](https://github.com/821869798) 在 [#101](https://github.com/Kuddev/pebrel/pull/101) 中贡献。

#### 改进

- 默认关闭背景模糊，并使用 100% 不透明度，让主题按实色显示；保留用户已明确设置的模糊和不透明度。
- 侧栏和文件区的默认分隔线在高缩放下保持单个物理像素宽度，并沿用主题分隔线颜色，让边界更轻。
- 改进 Markdown 阅读，限制预览处理量，并共享后台公式渲染；不支持或超限的表达式保留源码。
- 为代码和公式复制控件加入就地反馈，并将文档专注模式与系统全屏分开。
- 文件树中被 Git 忽略的文件和目录使用斜体显示，便于区分。

Windows 提供 Pebrel 安装器和 ZIP 便携包，不再提供旧名兼容安装器；依赖旧文件名的客户端可从 Release 页面下载 Pebrel 安装器完成更新。Linux x64 包以及 macOS Apple Silicon／Intel DMG 继续标记为 Preview。分子结构渲染仍处于禁用状态。

### Contributors

<a href="https://github.com/YinBuLiao"><img src="https://github.com/YinBuLiao.png?size=96" width="64" height="64" alt="@YinBuLiao avatar"></a><a href="https://github.com/821869798"><img src="https://github.com/821869798.png?size=96" width="64" height="64" alt="@821869798 avatar"></a><a href="https://github.com/Kuddev"><img src="https://github.com/Kuddev.png?size=96" width="64" height="64" alt="@Kuddev avatar"></a>

**[@YinBuLiao](https://github.com/YinBuLiao)** - Contributed host-default shell selection, consistent shell icons and spacing, and bounded notification previews. / 贡献宿主默认 Shell 选择、统一的 Shell 图标与间距，以及通知预览限幅。（[#110](https://github.com/Kuddev/pebrel/pull/110)、[#111](https://github.com/Kuddev/pebrel/pull/111)、[#112](https://github.com/Kuddev/pebrel/pull/112)）

**[@821869798](https://github.com/821869798)** - Corrected Antigravity confirmation and question-state detection. / 修正 Antigravity 确认与问题选择状态识别。（[#101](https://github.com/Kuddev/pebrel/pull/101)）

**[@Kuddev](https://github.com/Kuddev)** - Added document and layout workflows, the AI message toast preference, and release integration fixes. / 完成文档与布局工作流、AI 消息弹窗设置及发布整合修复。

---

**SHA256**

- `Pebrel-v1.7.0-windows-x64.zip`: `412c192b7d64754f5e4fa4ee930e89bb3d229e5e53b40cf6b26cd802640d4c49`
- `Pebrel-v1.7.0-windows-x64-setup.exe`: `b8787c1f7dd0232f09f20e09c9e20ad7160ed8a7613e7bc96edaf9d02811cd13`
- `Pebrel-v1.7.0-linux-x64-preview.AppImage`: `a69ef9bb334bfe78447e13487db63802922f56338fcc0fcffe04128dbd630b04`
- `Pebrel-v1.7.0-linux-x64-preview.deb`: `6d4d2712849697bb765d07d6bd79d6245b166cf3ee26d6d50622d7dbf819d1ec`
- `Pebrel-v1.7.0-linux-x64-preview.tar.gz`: `ef06460beb6b7d4cec8a7e74849dc0f1667439081f13e81f1f61f0b0a8263f3f`
- `Pebrel-v1.7.0-macos-arm64-preview.dmg`: `1380d39971d38fe652146c3e2b28de3e0c58b35ad40358ca88a308bab5844a9e`
- `Pebrel-v1.7.0-macos-x64-preview.dmg`: `2ed9962966420b45205674a7efdf32a278bc3e008f9258b128789388cc946361`

## 1.6.0 - 2026-09-07

### English

#### Added

- Added Linux x64 Preview packages in AppImage, DEB, and portable tar.gz formats, plus separate macOS Preview DMGs for Apple Silicon and Intel, alongside the stable Windows installer and ZIP. RPM packages are not included. macOS packages use ad-hoc signing and may require Open Anyway in System Settings > Privacy & Security on first launch. Addresses [#87](https://github.com/Kuddev/nebula/issues/87).
- Added clipboard image paste for local, WSL, and SSH terminals. Pebrel saves the image as a PNG and inserts a path that the session can access; SSH images are uploaded first. Attachment thumbnails and images in a CLI's conversation still depend on that CLI.
- Added a reader for captured Claude Code and Codex answers, with Markdown, formulas, original text, and local image previews.
- Added separate SOCKS5/HTTP proxy and jump-host settings for each SSH host, with saved proxy credentials and a connection-route preview. Addresses [#90](https://github.com/Kuddev/nebula/issues/90).
- Added an optional Windows system-proxy setting for newly opened local terminals. It is off by default and does not change existing sessions. Contributed by [@Sakyvo](https://github.com/Sakyvo) in [#94](https://github.com/Kuddev/pebrel/pull/94).
- Added nine UI language choices alongside English and Simplified Chinese: Traditional Chinese, French, German, Spanish, Brazilian Portuguese, Italian, Russian, Japanese, and Korean. Untranslated text falls back to English.
- Added 25 application icon palettes with light and dark previews. Titanium is now the default icon.
- Added settings search and a confirmed Restore defaults action. Preferences are backed up before resetting, and saved hosts and other user data are retained.
- Added Allow and Deny actions to notifications for recognized terminal confirmation prompts. Actions stop being available when the prompt or session changes, and each request can be handled only once.

#### Fixed

- Fixed reopening the workspace restoring only the tab for Codex sessions without a hook-reported conversation ID. On WSL and Linux, Pebrel can now identify the current conversation and save it before window close or the application's Quit action. Native Windows and macOS sessions continue to rely on hook-reported IDs.
- Fixed quitting after closing the last regular window replacing saved tabs with an empty workspace. Deliberately closing every tab still starts an empty workspace next time.
- Fixed `Ctrl+C` ignoring selected terminal text. It now copies and clears the selection; without a selection, it still interrupts the running command. Contributed by [@Sakyvo](https://github.com/Sakyvo) in [#93](https://github.com/Kuddev/pebrel/pull/93).
- Fixed `Ctrl+Backspace` deleting only one character in supported terminal input modes and the managed PowerShell prompt. It now deletes a word. Contributed by [@Sakyvo](https://github.com/Sakyvo) in [#92](https://github.com/Kuddev/pebrel/pull/92).
- Fixed `Shift+Enter` being treated as plain Enter in negotiated multiline input modes, and Antigravity continuing to show a busy indicator after an answer finished. `Ctrl+Enter`, `Alt+V`, and other modified keys also retain their negotiated modifiers. Addresses [#95](https://github.com/Kuddev/nebula/issues/95). Contributed in part by [@821869798](https://github.com/821869798) in [#96](https://github.com/Kuddev/pebrel/pull/96).
- Fixed Windows SSH sessions losing an application's first terminal-capability response during startup, which could leave a full-screen command-line application waiting on a blank screen.
- Fixed duplicated WSL and SSH tabs losing their known working directory. WSL tabs also retain the selected distribution and launch settings.
- Fixed file search showing results from a previously opened directory or missing newly created, renamed, and deleted files.
- Fixed long confirmation messages being clipped by the dialog instead of wrapping to fit.
- Fixed the Runtime CLI showing an error dialog when the program receiving its output closed the pipe. It now finishes cleanly in that case.

#### Improved

- Improved the upgrade from Nebula to Pebrel: the application, commands, installer, and managed shortcuts use the new name. The installer migrates the standard old installation directory while preserving custom locations; configuration migration keeps old data and does not overwrite newer files.
- Improved continuity after the repository rename. Old `Kuddev/nebula` repository and Git links redirect to `Kuddev/pebrel`, and historical download filenames remain available.
- Improved package naming to use `Pebrel-v<version>-<system>-<architecture>` consistently. A byte-identical Windows installer with the old Nebula filename remains available for existing automatic-update clients.
- Improved SSH connection feedback: startup can be cancelled, stalled connection stages time out, and an unexpected disconnect leaves the terminal and error visible. This improves the disappearing-window and stuck-connection behavior. Addresses [#89](https://github.com/Kuddev/nebula/issues/89).
- Improved streamed formula recognition and layout, while preserving the original text when a formula cannot be displayed. KaTeX syntax support remains partial. Addresses [#84](https://github.com/Kuddev/nebula/issues/84).
- Improved notifications so clicking one focuses the terminal that raised it, even after moving its tab to another window. Activity in a different terminal no longer suppresses that notification.
- Improved activity indicators so they continue animating when another application has focus; hidden and minimized windows stop requesting animation frames.
- Improved settings with clearer groups, compact navigation, and a normal closable Settings tab. Light-theme selection and hover colors are less intense, and SSH editing makes common usernames and host controls easier to find.
- Improved completion lists with wheel scrolling, visible keyboard selections, and hover feedback. Changing the query clears the old list position.

### 中文

#### 新增

- 新增 Linux x64 Preview 的 AppImage、DEB 和 tar.gz 便携包，以及分别适用于 Apple Silicon 和 Intel 的 macOS Preview DMG；Windows 提供正式版安装器与 ZIP 便携包。本次不包含 RPM。macOS 包采用临时签名，首次启动可能需要在“系统设置 > 隐私与安全性”中选择“仍要打开”。对应 [#87](https://github.com/Kuddev/nebula/issues/87)。
- 新增本地、WSL 和 SSH 终端的剪贴板图片粘贴：将图片保存成 PNG，再插入当前会话可以访问的路径，SSH 会先上传图片。输入框缩略图和发送后的图片显示仍取决于所用 CLI。
- 新增 Claude Code 和 Codex 回答阅读器，支持阅读已捕获回答的 Markdown、公式、原文和本地图片。
- 新增每台 SSH 主机独立的 SOCKS5/HTTP 代理与跳板设置，可保存代理凭据并预览连接路线。对应 [#90](https://github.com/Kuddev/nebula/issues/90)。
- 新增让新建本地终端使用 Windows 系统代理的选项，默认关闭，不影响已经打开的会话。由 [@Sakyvo](https://github.com/Sakyvo) 在 [#94](https://github.com/Kuddev/pebrel/pull/94) 中贡献。
- 新增繁体中文、法语、德语、西班牙语、巴西葡萄牙语、意大利语、俄语、日语和韩语界面选项，加上原有英语和简体中文共十一种语言；尚未翻译的内容显示英文。
- 新增 25 款应用图标配色，提供浅色和深色预览，并将钛银设为默认图标。
- 新增设置搜索和需要确认的“恢复默认”操作；重置前备份偏好设置，保留已保存主机等用户数据。
- 新增可识别终端确认提示的通知操作，可直接选择允许或拒绝；提示或会话变化后操作失效，同一请求只能处理一次。

#### 修复

- 修复部分 Codex 会话重开应用后只恢复标签、没有接续对话的问题。在 WSL 和 Linux 中，即使 hook 未上报 ID，也能识别当前对话，并在关闭窗口或通过应用内“退出”操作结束前保存；原生 Windows 和 macOS 会话仍依赖 hook 上报的 ID。
- 修复关闭最后一个普通窗口后再退出，已保存的标签被空工作区覆盖的问题；主动关闭全部标签时，下次仍从空工作区开始。
- 修复选中终端文字后按 `Ctrl+C` 没有复制的问题：现在会复制并清除选区，没有选区时仍中断当前命令。由 [@Sakyvo](https://github.com/Sakyvo) 在 [#93](https://github.com/Kuddev/pebrel/pull/93) 中贡献。
- 修复受支持的终端输入模式和受管理 PowerShell 提示符中，`Ctrl+Backspace` 只删除一个字符的问题，现在可按词删除。由 [@Sakyvo](https://github.com/Sakyvo) 在 [#92](https://github.com/Kuddev/pebrel/pull/92) 中贡献。
- 修复已协商多行输入模式下 `Shift+Enter` 被当成普通回车，以及 Antigravity 回答结束后仍一直转圈的问题；`Ctrl+Enter`、`Alt+V` 等按键也会保留协商协议要求的修饰信息。对应 [#95](https://github.com/Kuddev/nebula/issues/95)。其中部分修复由 [@821869798](https://github.com/821869798) 在 [#96](https://github.com/Kuddev/pebrel/pull/96) 中贡献。
- 修复 Windows SSH 会话启动时丢失应用首个终端能力应答，可能使全屏命令行程序一直停在空白画面的问题。
- 修复复制 WSL 和 SSH 标签后丢失已知工作目录的问题；WSL 同时保留所选发行版与启动设置。
- 修复文件搜索残留上一个目录的结果，以及新建、重命名或删除文件后结果未及时更新的问题。
- 修复较长确认消息被对话框截断的问题，现在会换行并适应内容高度。
- 修复接收输出的程序关闭管道后，Runtime CLI 弹出错误窗口的问题；这种情况现在会正常结束。

#### 改进

- 改进 Nebula 到 Pebrel 的升级体验：应用、命令、安装器和受管理的快捷方式统一使用新名称。安装器会迁移标准旧安装目录并保留自定义位置；配置迁移保留旧数据，不覆盖较新的文件。
- 改进仓库改名后的访问衔接：旧 `Kuddev/nebula` 仓库与 Git 链接会跳转到 `Kuddev/pebrel`，历史下载文件名继续保留。
- 改进安装包命名，统一采用 `Pebrel-v<版本>-<系统>-<架构>`；同时提供与正式安装器字节一致的旧 Nebula 文件名资产，保证旧版自动更新客户端仍能找到安装包。
- 改进 SSH 连接反馈：连接准备期间可以取消，停滞阶段会超时结束，意外断连后保留终端并显示错误。本次处理了窗口消失和连接一直卡住的相关表现。对应 [#89](https://github.com/Kuddev/nebula/issues/89)。
- 改进连续输出时的公式识别与排版，无法显示的公式保留原文；KaTeX 语法支持仍有范围限制。对应 [#84](https://github.com/Kuddev/nebula/issues/84)。
- 改进通知定位：点击通知会回到发出通知的终端，即使其标签已移动到另一个窗口；其他终端的活动不再抑制这条通知。
- 改进运行状态指示器，切到其他应用后仍继续播放动画，窗口隐藏或最小化时停止请求动画帧。
- 改进设置的分组、导航和标签体验，设置页可像普通标签一样关闭；降低浅色主题的选中与悬停颜色强度，SSH 编辑器中的常用用户名和主机控件也更容易找到。
- 改进补全列表，支持滚轮浏览、自动显示键盘选中项和悬停反馈；查询变化后清除旧列表位置。

### Contributors

<a href="https://github.com/Sakyvo"><img src="https://github.com/Sakyvo.png?size=96" width="64" height="64" alt="@Sakyvo avatar"></a><a href="https://github.com/821869798"><img src="https://github.com/821869798.png?size=96" width="64" height="64" alt="@821869798 avatar"></a>

**[@Sakyvo](https://github.com/Sakyvo)** - Added the optional Windows terminal proxy, selection-aware `Ctrl+C`, and word deletion with `Ctrl+Backspace`. / 新增可选的 Windows 终端代理、按选区处理的 `Ctrl+C`，以及 `Ctrl+Backspace` 按词删除。（[#94](https://github.com/Kuddev/pebrel/pull/94)、[#93](https://github.com/Kuddev/pebrel/pull/93)、[#92](https://github.com/Kuddev/pebrel/pull/92)）

**[@821869798](https://github.com/821869798)** - Contributed `Shift+Enter` multiline input and Antigravity completion and permission-state detection. / 贡献 `Shift+Enter` 多行输入，以及 Antigravity 回答结束与权限状态识别。（[#96](https://github.com/Kuddev/pebrel/pull/96)）

---

**SHA256**

- `Pebrel-v1.6.0-windows-x64.zip`: `d57f9c61988ca950da1088415717137b85e022475e046d611f3595b48e66ce2d`
- `Pebrel-v1.6.0-windows-x64-setup.exe`: `369d5f93f34d7f90bde91b51a4f66259d1f277619f7e2b618fb6e03d449f605f`
- `NebulaTerminal-1.6.0-windows-x64-setup.exe`: `369d5f93f34d7f90bde91b51a4f66259d1f277619f7e2b618fb6e03d449f605f`
- `Pebrel-v1.6.0-linux-x64-preview.AppImage`: `91e44fef5f56b3bc5f840a603cbcbf1a8186c4fa042ea012a47b11855cc305cd`
- `Pebrel-v1.6.0-linux-x64-preview.deb`: `f3f4f6292f5460dd7ed626d9a5d8279ae89f7cce1bd21c90db7acd4164eef57a`
- `Pebrel-v1.6.0-linux-x64-preview.tar.gz`: `0e56fb2a01d8cce71e4cda8290f8edde1868d37e588cdbf443bc42dee18dd22b`
- `Pebrel-v1.6.0-macos-arm64-preview.dmg`: `1b747113b2d6f868f2547a365d72a0ec4a2d045900708f2f7c18315d2691cf74`
- `Pebrel-v1.6.0-macos-x64-preview.dmg`: `1bf53ef3dc957a8e3bac89b5a516f4c1153651c48eb58e912180f3ab48a46548`

## 1.5.0 - 2026-09-01

### English

#### Added
- Added separate username and host fields for SSH connections.
- Added recent-connection and OpenSSH configuration suggestions for SSH usernames.
- Added a Remote Files workspace that opens after SSH authentication.
- Remote Files can follow the interactive remote shell's working directory.
- Remote Files provides parent navigation, refresh, and file, directory, and symlink details.
- Added multi-file SFTP uploads.
- Added recursive directory uploads.
- Added recursive file and directory downloads to a chosen local destination.
- Added remote directory creation, rename, and recursive delete.
- Added copy and paste between remote directories and SSH destinations.
- Added overwrite, skip, and keep-both choices for transfer conflicts.
- Added type-mismatch protection and symlink warnings for transfers.
- Added an option to skip unchanged remote files.
- Added byte-level SFTP progress with preparation and error states.
- Added transfer cancellation while keeping remote navigation available.
- Added a compact saved-command manager for searching, running, editing, and removing reusable commands.
- Added a repository-aware `Add to .gitignore` action that finds the correct repository root and writes the selected path.
- Added a Git history view with smooth semantic tracks for commit topology.
- Added distinct branch, tag, merge, and conflict icons with matching meanings.
- Local and remote refs are now classified correctly in Git history.
- Added a three-column conflict-resolution tab for comparing ours, the result, and theirs.
- Saving a resolved conflict writes the file and stages it in Git.
- Added indexed filename search to the Files drawer with case, whole-word, and regex controls. Addresses [#88](https://github.com/Kuddev/nebula/issues/88) for the search-box portion; the requested `Ctrl+F` shortcut remains open.
- Added a persistent option to skip risky multi-line paste confirmation. Contributed by [@Sakyvo](https://github.com/Sakyvo) in [#76](https://github.com/Kuddev/nebula/pull/76).
- Added an option to hide tab close buttons in both tab layouts while keeping middle-click close. Contributed by [@Sakyvo](https://github.com/Sakyvo) in [#77](https://github.com/Kuddev/nebula/pull/77).

#### Fixed
- Fixed GPUI text inputs sending `Ctrl+V` to the terminal behind them.
- Fixed the saved-command manager expanding to an oversized empty area when it contains only a few rows.
- Fixed switching between SSH tabs reloading Remote Files; each pane now retains its confirmed directory contents, selection, and scroll position.
- Fixed Remote Files starting SFTP requests before SSH authentication completes.
- Stale directory results are discarded after the user changes pane or path.
- Fixed duplicate SSH password prompts.
- Failed pooled SSH transports are no longer reused; the affected channel retries once on a fresh connection. Addresses [#50](https://github.com/Kuddev/nebula/issues/50) for post-failure reconnect only; the intermittent disconnect and rendering reports remain open.
- A failed SSH channel no longer terminates unrelated panes.
- Fixed TortoiseSVN dialogs failing to open with `ERROR_INVALID_HANDLE` (`os error 6`) when Nebula has no inheritable console handles.
- Direct WSL Agent commands are now recognized from the complete committed command.
- WSL Agent launches through `npx`, `node`, and `uvx` are recognized under the Agent name.
- Broadcast Enter now reaches command tracking in every receiving WSL pane.
- WSL Agent and runtime activity is cleared when the command completes.
- Stale WSL Agent state is cleared when the original guest-shell prompt returns.

#### Improved
- Improved SFTP throughput with bounded file concurrency and bounded memory use.
- Uploads now pipeline remote writes.
- Large downloads now use parallel segmented reads.
- SFTP publication now uses sibling staging files and atomic replacement.
- Interrupted replacements retain a recoverable backup.
- Transfers preserve timestamps and permissions where supported.
- Cancellation safely cleans up temporary transfer state.
- Improved remote-directory following across modern and legacy SSH servers.
- Foreground shells entered through `su` or `sudo` can report their remote directory.
- Ambiguous remote sessions fall back safely instead of guessing a path.
- WSL shell integration now reports command completion and exit status.
- WSL shell integration now reports cwd and the next prompt boundary.
- WSL integration keeps the guest's configured login shell.
- Existing `WSLENV` entries are preserved instead of overwritten.
- Added Linux GPUI platform wiring for both Wayland and X11 as groundwork for the native Linux port.
- Isolated the GPUI product shell from legacy window, input, rendering, and event adapters so Linux can use the same product architecture.
- Made GPUI the default product shell and kept the legacy shell behind an explicit feature, so ordinary release builds follow the same product path used by the packaged application.

### 简体中文

#### 新增
- SSH 连接新增分离的用户名与主机字段。
- SSH 用户名新增最近连接与 OpenSSH 配置建议。
- 新增 SSH 认证完成后打开的 Remote Files 远程文件工作区。
- Remote Files 可跟随交互式远端 shell 的工作目录。
- Remote Files 提供上一级、刷新以及文件、目录和符号链接详情。
- SFTP 新增多文件上传。
- SFTP 新增递归目录上传。
- 新增把远端文件和目录递归下载到指定本地位置。
- 新增远端目录创建、重命名和递归删除。
- 新增跨远端目录和 SSH 目标的复制粘贴。
- 传输冲突新增覆盖、跳过和保留两者三种选择。
- 传输新增类型不匹配保护和符号链接警告。
- 新增跳过未变化远端文件的选项。
- 新增按字节显示的 SFTP 进度以及准备和错误状态。
- 新增传输取消；取消期间仍可继续浏览远端目录。
- 新增紧凑的保存命令管理器，支持搜索、运行、编辑和删除可复用命令。
- 新增仓库感知的“加入 `.gitignore`”操作，会定位正确的仓库根并写入所选路径。
- 新增 Git 历史视图，使用丝滑的语义轨道展示提交拓扑。
- 分支、标签、合并和冲突使用含义匹配的独立图标。
- Git 历史现在会正确分类本地与远端引用。
- 新增三栏冲突解决 Tab，可并排比较 ours、结果与 theirs。
- 保存冲突结果后会写回文件并在 Git 中暂存。
- 文件抽屉新增带索引的文件名搜索，以及区分大小写、全词和正则选项。此项对应 [#88](https://github.com/Kuddev/nebula/issues/88) 的搜索框部分；需求中的 `Ctrl+F` 快捷键仍未完成。
- 新增永久跳过高风险多行粘贴确认的选项。由 [@Sakyvo](https://github.com/Sakyvo) 在 [#76](https://github.com/Kuddev/nebula/pull/76) 中贡献。
- 新增在两种标签布局中隐藏关闭按钮的选项，同时保留中键关闭。由 [@Sakyvo](https://github.com/Sakyvo) 在 [#77](https://github.com/Kuddev/nebula/pull/77) 中贡献。

#### 修复
- 修复 GPUI 文本输入框把 `Ctrl+V` 发送到背后终端的问题。
- 修复保存命令管理器只有少量结果时仍展开出大块空白区域的问题。
- 修复切换 SSH 标签时重新加载 Remote Files；每个 pane 现在会保留已经确认的目录内容、选中项和滚动位置。
- 修复 SSH 认证完成前 Remote Files 就发起 SFTP 请求的问题。
- 用户切换 pane 或路径后返回的过期目录结果会被丢弃。
- 修复 SSH 重复密码提示。
- 不再复用已经失效的 SSH 连接；受影响的 channel 会在全新连接上重试一次。此项对应 [#50](https://github.com/Kuddev/nebula/issues/50) 的失败后重连部分；间歇断连和渲染报告仍保持开放。
- 单个 SSH channel 失败不再终止无关 pane。
- 修复 Nebula 没有可继承控制台句柄时，TortoiseSVN 对话框因 `ERROR_INVALID_HANDLE`（`os error 6`）无法打开。
- 现在会从完整的已提交命令识别直接启动的 WSL Agent。
- 通过 `npx`、`node` 和 `uvx` 启动的 WSL Agent 会显示正确的 Agent 名称。
- 广播 Enter 现在会进入每个接收方 WSL pane 的命令跟踪。
- 命令完成后会清除 WSL Agent 与 Runtime 活动态。
- 原始 guest shell 提示符恢复后会清除过期的 WSL Agent 状态。

#### 改进
- SFTP 通过有界文件并发提升吞吐，同时限制内存占用。
- 上传现在使用流水线式远端写入。
- 大文件下载现在使用并行分段读取。
- SFTP 发布改用同目录暂存文件和原子替换。
- 替换中断时会保留可恢复备份。
- 远端支持时会保留时间戳与权限。
- 取消操作会安全清理临时传输状态。
- 改进现代与旧版 SSH 服务上的远端目录跟随。
- 可定位经 `su` 或 `sudo` 进入的前台 shell 工作目录。
- 远端会话存在歧义时会安全回退，不猜测路径。
- WSL shell 集成现在会上报命令完成和退出状态。
- WSL shell 集成现在会上报 cwd 和下一提示符边界。
- WSL 集成会保留 guest 已配置的登录 shell。
- 现有 `WSLENV` 项会被保留，不再覆盖。
- 新增 Wayland 与 X11 的 Linux GPUI 平台接线，作为原生 Linux 移植地基。
- GPUI 产品壳已与旧版窗口、输入、渲染和事件适配器隔离，让 Linux 可沿用同一套产品架构。
- 将 GPUI 设为默认产品壳，旧壳保留在显式 feature 后，普通 Release 构建与打包应用使用同一条产品路径。

## 1.4.1 - 2026-08-30

### English

#### Added
- Added `agent.delegate` to the Runtime control API, allowing a managed Agent to delegate work to a verified target and receive the bounded result back in the originating session.

#### Fixed
- Fixed Claude hooks failing under PowerShell on Windows by installing them in exec form and repairing older shell-form entries in place.
- Fixed user-defined PowerShell and Git Bash prompts being overwritten while preserving Nebula's shell-integration markers.
- Fixed Windows environment expansion leaving literal `%USERPROFILE%` paths that could create a `%USERPROFILE%` folder in the launch directory. Contributed by [@Mikachu2333](https://github.com/Mikachu2333) in [#86](https://github.com/Kuddev/nebula/pull/86).
- Fixed GPUI confirmation and cancellation buttons not responding to mouse clicks, and stopped terminal right-click paste from propagating into overlapping UI layers.
- Fixed `Ctrl+V` in a focused tab-rename field also pasting the clipboard into the terminal behind it.

#### Improved
- Improved tab and sidebar activity badges with OSC 9;4 running, paused, and error states without overriding managed-Agent hook state.
- Improved running activity spinners by scheduling them on GPUI display frames instead of an approximately 12.5 FPS timer, while retaining visibility and window-activity gating.

### 简体中文

#### 新增
- Runtime 控制 API 新增 `agent.delegate`，托管 Agent 可以把任务委派给经过核实的目标，并在原会话中收到长度受限的执行结果。

#### 修复
- 修复 Windows PowerShell 下 Claude hook 执行失败；hook 改用 exec 形式安装，并会就地修复旧的 shell 形式配置。
- 修复用户自定义 PowerShell 与 Git Bash 提示符被覆盖，同时继续保留 Nebula 的 shell 集成标记。
- 修复 Windows 环境展开留下字面量 `%USERPROFILE%` 路径、进而可能在启动目录创建 `%USERPROFILE%` 文件夹。由 [@Mikachu2333](https://github.com/Mikachu2333) 在 [#86](https://github.com/Kuddev/nebula/pull/86) 中贡献。
- 修复 GPUI 确认框的确认与取消按钮无法响应鼠标点击，并阻止终端右键粘贴继续传播到重叠的界面层。
- 修复标签重命名输入框聚焦时按 `Ctrl+V` 还会把剪贴板内容粘贴到背后终端的问题。

#### 改进
- 改进标签与侧栏活动徽章，支持 OSC 9;4 的运行、暂停和错误状态，同时不覆盖托管 Agent hook 的权威状态。
- 改进运行态转圈动画，改由 GPUI 显示帧调度，不再使用约 12.5 FPS 的定时器，同时保留可见性与窗口激活门控。

## 1.4.0 - 2026-08-29

### English

#### Added
- **The terminal card's geometry is now one configurable group — corner radius, gutter, shadow, and the divider between the tab sidebar and the terminal — and it travels with the theme.** "A rounded card floating on the shell" and "the terminal filling the entire right-hand area, separated by a single hairline" are no longer two layouts or two code paths; they are two sets of values on one path. `Nord` is the first theme to take the second form: radius and gutter both drop to zero and a 1px divider carries the structural boundary, because once the gutter is gone the sidebar and the terminal read as one undivided slab. Every other theme keeps the floating card and deliberately draws *no* divider — with a gutter present, a line would be a second boundary saying the same thing. Explicit `pane_card_radius` / `pane_card_gutter` / `pane_card_shadow` / `pane_card_divider` keys in `nebula_settings.txt` override whatever the theme asks for, so switching themes changes the shape while anyone who has tuned it keeps control. The radius default also became a single source of truth: the old shell's `UI_SHELL_RADIUS_LOGICAL` and the new shell's card now reference the same constant instead of each carrying its own literal — two copies of one number is exactly what produced the white seam around the card.
- **GPUI settings now ship with embedded English and Simplified Chinese catalogs.** The language preference can follow the system or explicitly select either catalog, persists across launches, and updates the settings navigation, controls, placeholders, provider tests, network tests, SSH status, and other migrated settings messages without reading translation files at runtime. English is the fallback for missing localized messages, configuration values remain separate from display text, and the build rejects catalogs whose message-ID sets drift apart. Addresses [#71](https://github.com/Kuddev/nebula/issues/71) for the GPUI Settings portion only; other untranslated surfaces remain outside this release.
- **Windows now has a dedicated GPUI quick terminal window.** The configured global shortcut opens a process-wide singleton across the full width and 40% height of the display containing the most recently active regular workspace, slides it from the top edge, and preserves its PTY, scrollback, and terminal state while hidden. Changing the shortcut re-registers it with Windows, and repeated key events are coalesced into one show or hide action.
- **A versioned Runtime control API and resource-style CLI now give automation one discoverable interface to Nebula.** `nebula env` reports the current pane identity, control executable, supported commands, and runtime reachability; `nebula window`, `nebula tab`, `nebula pane`, and `nebula agent` expose read, send, wait, close, rename, move, split, zoom, resize, run, paste, and execution operations through the same JSON protocol. Bash, Fish, and Zsh completions describe the expanded command surface.
- **Typed multi-step orchestration can build an Agent workspace in one request.** A workflow may create tabs, split panes, start verified AI providers in parallel, wait for the requested generation to become ready, and submit initial prompts. Backward step references, bounded templates, explicit partial-failure receipts, and no implicit rollback keep completed actions visible and resumable when a later step fails.
- **Managed Agents now have stable names, generation identities, and transactional Git worktree forks.** Cold-start, resume, and fork syntax is limited to verified `claude`, `codex`, `opencode`, `cursor`, `pi`, `omp`, and `kimi` commands; fork results retain branch, path, source, and base-commit provenance, reject dirty or conflicting sources, and clean up failed transactions. Native SSH panes report worktree forking as unsupported instead of pretending to create a remote checkout.
- **Local terminal panes expose a stable automation environment contract.** `TERM_PROGRAM`, `TERM_PROGRAM_VERSION`, `NEBULA_PANE_ID`, `NEBULA_CLI`, and `NEBULA_BIN_DIR` identify the pane and control plane; WSL receives idempotent `PATH` / `WSLENV` forwarding with translated paths, while native SSH receives only a remote marker and never advertises a host-local control executable.
- **`pane.exec` runs a direct argv as an independent non-TTY child in the pane's local or WSL working context.** It leaves shell history, the interactive terminal, cwd, and pane environment untouched; captures stdout and stderr concurrently; reports exit status, encoding, truncation, and timeout metadata; and bounds retained output. Timeouts and early parent exits clean up descendants through a Windows Job Object or Unix process group. Native SSH returns `remote_exec_unsupported` rather than silently running elsewhere.
- **Panes and live managed Agents support bounded bracketed paste from literal text, stdin, or a file.** Prompt, paste, control-key, argv, and multiline validation prevent arbitrary terminal bytes from bypassing the public API, and generation-aware targeting keeps delayed input from reaching a restarted Agent with the same display name.
- **Quick Jump searches tabs, panes, frequent directories, configured SSH hosts, and recoverable AI sessions in one palette.** Results retain tab and pane location, cwd, remote destination, provider, and session metadata, then expose direct focus, open, launch, or resume actions. File and Recipe scopes remain hidden because those backends are not yet end-to-end.
- **Selected terminal or document text can be sent to an explicitly chosen live Agent.** The searchable Send to Chat dialog previews the selection as a quote, preserves blank lines, accepts an optional comment, can copy the composed message, labels remote SSH targets, and reports the destination after sending. If no live Agent exists, the action is disabled instead of guessing a pane.
- **Windows terminal tabs can be dragged between Nebula windows.** A tab may become a new tab in the target or dock its complete split tree to the left, right, top, or bottom; terminal panes, runtime ownership, subscriptions, focus, and session metadata move together. Moving the last tab closes the empty source window, while a target that disappears before commit leaves the source tab attached.
- **Windows taskbar progress reflects OSC 9;4 emitted by terminal applications.** Normal, indeterminate, error, paused, and cleared states map to the native taskbar indicator; unsupported or unknown states clear stale progress instead of leaving the taskbar stuck.
- **The GPUI file tree now offers complete context actions for local and WSL entries.** Users can open files or directories, reveal them in Explorer, copy host or Linux paths, launch a terminal in a directory, and send deletions to the recycle bin after confirmation. WSL routes retain their distribution and guest path instead of being flattened into an invalid host path.

#### Fixed
- **Output no longer stops mid-frame with the CLI's input box missing.** The 1.2.0 fix for the lost `piper` wakeup only closed half the race. It re-posted the readable event when the pipe still held bytes *at the moment the read returned* — but `drain_inner` takes the read waker **before** it copies any data, so "this read got something" already means "no waker is registered", regardless of whether the pipe happens to look empty now. An AI CLI's trailing bytes land in exactly that window: a screenful of diff pushes `pty_read` past its 64 KB `MAX_LOCKED_READ` break just as the pipe drains, and the few hundred bytes of input box and status bar printed right after find an empty waker, so their `wake()` is a no-op. Those bytes then sit in the pipe until a keystroke (which flips write interest, re-registers, and re-posts) or a resize (which force-drains) lets them out; dragging a tab does neither and never recovered. Live evidence from a stuck pane: one no-op arrow key released 527 stranded lines at once. The repost now keys only on "this read got data", which is self-converging — the following read finds the pipe empty and piper registers the waker again.
- **Common formulas such as `$E=mc^2$` and `$n!$` are recognized without reinterpreting shell variables, prompts, prices, or `sed`/`grep` BRE groups as mathematics.** Inline delimiters may cross terminal soft wraps but no longer join unrelated hard lines.
- **Resetting a settings drop-down immediately restores its displayed default value** instead of updating the saved setting while leaving the old selection visible.
- **Rounded terminal panes no longer expose an outline or a gap along their top edge** in either light or dark themes; the shell paint now follows the pane's actual left, right, bottom, and zero-width top gutters.
- **New Windows terminal panes no longer inherit stale environment variables from the long-running Nebula process.** Each pane rebuilds its environment from the current machine and user registry values, merges `PATH` in Windows order, treats variable names case-insensitively, expands registry values against the refreshed snapshot, preserves process-private values when a registry entry disappears, and applies pane-specific overrides last. The same refreshed environment reaches GPUI panes, legacy panes, and WSL launches. Addresses [#68](https://github.com/Kuddev/nebula/issues/68).
- **The GPUI quick-terminal shortcut now works outside Nebula.** The configured combination is registered with Windows when the first workspace opens, follows later settings changes without discarding a still-valid old registration if the replacement fails, and targets the most recently active regular workspace instead of requiring Nebula to be focused.
- **The quick terminal no longer opens with a blank strip or settles at ordinary-window dimensions.** Its native size and terminal surface are synchronized once while the window is still above the screen, and its full-width, 40%-height geometry is confirmed after entry without repeatedly resizing the PTY or swapchain. It is also excluded from regular-window MRU routing, runtime command targeting, session restoration, reveal-all behavior, and ordinary workspace persistence, so hiding or closing it cannot rewrite the normal workspace session.
- **Display-math delimiters no longer consume unrelated prose or later formulas.** Multiline `$$...$$` and `\[...\]` pairing now requires a real block-opening context and stops at blank-line presentation boundaries, so a delimiter mentioned in prose or an orphaned closer at the top of scrollback cannot pair with a later block.
- **Presentation-only `\boxed{...}` and `\tag{...}` commands no longer make an otherwise valid formula fall back to raw TeX.** Boxed content remains renderable even when the current math backend cannot draw the frame, and equation tags are retained as inline numbers.
- **Spaces and other valid ink-free glyphs inside `\text{...}` no longer make an entire formula disappear.** Glyphs with advance width but no outline produce an empty bitmap instead of a missing-glyph error; if a formula still cannot be composed, its original terminal cells remain visible rather than leaving a blank hole.
- **Sidebar resizing follows the theme's actual boundary.** Nord's flush terminal layout centers the resize cursor and width calculation on its 1px divider, while floating-card themes continue to use their gutter, so the visible line no longer lags behind the pointer.
- **Tab badges no longer let fallback signals overwrite authoritative state.** Agent authorization and interactive prompts use the hand indicator, a non-zero command exit keeps a warning triangle until the next command begins, and a successful background completion briefly shows a check before settling into the unread-completion mark. A trailing BEL can request attention only when no agent rule has already decided the pane state, so a completed agent turn is not mislabeled as waiting for input.
- **Runtime completion edges no longer leak into a command that is still being submitted, and an unselected completion popup no longer captures shell history keys.** `Up` and `Down` remain available to the shell until `Tab` or a mouse action selects a candidate; once selected, the same keys navigate the popup as expected.
- **GPUI cursor blinking now agrees with its setting and with the existing shell.** An unset `cursor_blink` value enables blinking consistently in both Settings and the terminal; blinking pauses while the window or pane is unfocused and during IME preedit, then restarts from a visible cursor when focus or composition changes. The interval is aligned at 750ms, and stale timer generations cannot hide the cursor after blinking is no longer allowed.
- **Entering non-ASCII proxy text, including full-width punctuation, no longer crashes Settings.** Case-insensitive `socks5://`, `socks5h://`, `socks://`, and `http://` matching now respects UTF-8 character boundaries while preserving bare `host:port` compatibility. Addresses [#67](https://github.com/Kuddev/nebula/issues/67).
- **GPUI title-bar File Tree and Git controls no longer arm a window drag.** Their left mouse-down events stop before reaching the draggable title bar, while the real button click still opens the requested panel. Addresses [#62](https://github.com/Kuddev/nebula/issues/62).
- **GPUI diagnostics and Windows panic reporting no longer turn a recoverable diagnostic failure into a second panic.** GUI logging uses a fallible stderr sink and direct `println!` / `eprintln!` calls are rejected inside GPUI modules. On Windows, panic text is persisted to `nebula-panic.log` before a dedicated thread shows the error dialog, preventing its modal message loop from re-entering GPUI during the original panic. Addresses [#70](https://github.com/Kuddev/nebula/issues/70).
- **Selecting a wide terminal character highlights its complete two-column footprint.** Whether the pointer lands on the leading cell or spacer, CJK characters, full-width punctuation, and emoji produce one non-overlapping selection run; simple, block, semantic, and line selections copy the complete character instead of showing or copying half a glyph.
- **Runtime commands no longer show or activate their routed window as a side effect.** Snapshot, read, prompt, paste, run, exec, layout, and Agent operations preserve the current foreground window; runtime-created windows use non-activating show semantics, and only an explicit `focus` command activates a hidden or background workspace.
- **Top-tab and sidebar actions are clickable without leaking into parent drag or collapse handlers.** Terminal tabs, the top-bar new-tab and overflow controls, and the sidebar new-tab control stop their mouse-down event at the action boundary, retain a shared 34px alignment, and no longer start a title-bar drag or toggle the `TABS` group at the same time.
- **The sidebar divider is now one pixel-aligned segment from the top of the window to the pane card bottom.** It no longer stops below the title bar or forms a seam from two independently painted pieces, and it is omitted when the layout has no real sidebar boundary.
- **Runtime Agent notifications are associated with the correct pane and process.** On Windows, process-ancestry evidence can find hook helpers through intermediate shells, distinguish nested Agents by PID, and reject another pane's event instead of trusting a short-lived session id; unavailable remote process evidence is reported as such rather than fabricated.
- **Runtime waits no longer treat a pane that was already idle as completion of a command that has just been submitted.** Submission creates a fresh running/state-change edge before waiting, Agent waits stay bound to the requested generation, and a command without reliable OSC exit evidence is not reported as a verified success.
- **Runtime layout mutations no longer guess when a target is ambiguous or unavailable.** Explicit ambiguity, busy-pane, stale lifecycle, unsupported-remote, and invalid-target errors replace silent fallback to whichever window or pane happened to be current.
- **Local selection content is not silently forwarded to an SSH Agent by programmatic routing.** Automated sends return `remote_target_refused`; a user may still deliberately choose a labeled remote target in the Send to Chat dialog.
- **The GPUI file tree keeps long lists and status messages inside the rounded drawer.** Row density, clipping, scrolling, and bottom padding no longer fight each other, and empty states distinguish loading, enumeration failure, missing cwd, and a genuinely empty directory with an appropriate retry or navigation action.
- **WSL file-tree entries preserve their guest paths and distribution.** Files open through the corresponding `\\wsl.localhost` route, directories launch a terminal at the Linux cwd without forcing Bash over the distribution's configured default shell, and Explorer actions use the matching host-visible path.
- **Applications using DECSET 2031 receive consistent terminal color-scheme notifications.** The subscription uses the same light/dark predicate as OSC 11, reports the current scheme immediately, avoids duplicate notifications, and updates after a live theme change.
- **ConPTY cursor realignment now handles resize drift in both directions.** Nebula corrects content whether conhost collapsed rows above the local cursor or wrapped farther below it, and resize bursts are coalesced without dropping the final geometry or a required full-PTY notification.

#### Improved
- **Terminal TeX rendering now detects formulas from terminal content instead of a recognized AI process name**, so block formulas remain available through WSL, SSH, and full-screen agent interfaces. Standard `$...$`, `$$...$$`, `\(...\)`, and `\[...\]` forms, Markdown-damaged bare delimiters, compact fraction arguments such as `\frac13`, dimmed reasoning output, cell backgrounds, and overlapping persisted formulas are handled more reliably. Includes the terminal-math work contributed by [@liu-ws](https://github.com/liu-ws) in [#55](https://github.com/Kuddev/nebula/pull/55).
- **GPUI now uses the legacy compact projection contract for inline terminal math.** A rendered formula occupies its measured visual width and following text closes the unused source-cell gap; ANSI backgrounds, selections, box-drawing glyphs, cursor shapes, ghost text, completion popups, IME anchors, and link previews all use the same projected columns. Mouse selection and link hit-testing map back to unchanged terminal source coordinates, including non-zero scrollback origins. Active selections and VI mode reveal the original TeX, alternate-screen applications keep fixed TUI columns, overlapping streamed formulas share one survivor set, and font or bitmap failures fall back to source text without stale shifts or hidden cells.
- **Quick-terminal motion now matches the established slide behavior.** Entry uses a 120ms swift-out transition, exit uses a 90ms ease-in transition, and reversing direction mid-animation continues from the current position instead of jumping. Frame callbacks move only the native window's Y coordinate with no activation, resize, or z-order churn; user-triggered recall focuses once before entry, and the native window is hidden only after the exit finishes.
- **Nord is now the factory-default dark theme, paired with Paper when the system uses a light appearance.** Fresh installs and missing or invalid theme values use the same default in the legacy and GPUI shells, while an explicit user-selected theme remains untouched.
- **Runtime snapshots are now suitable for automation and auditing.** They expose revisions, semantic pane lifecycle and task state, zoom state, process evidence, stable Agent identity, generation, and worktree provenance; provider actions use verified cold-start, resume, and fork syntax instead of inferring commands from display names.
- **GPUI Settings navigation is now a flat list with stable routes.** Group headings are removed and the visible order is Application, Appearance, Profiles, Interaction, Key Bindings, SSH, Network, Advanced. AI Providers and Backup retain their implementations and route indices but are temporarily absent from the default sidebar.
- **GPUI session residency and tray actions now respect live-pane ownership.** When configured, closing a regular workspace hides it without killing PTYs; tray actions can reveal the correct workspace and focus a selected Agent pane, while an explicit tray quit saves session state before shutdown. Quick-terminal windows remain outside this ordinary-session lifecycle.
- **GPUI terminal rendering shares the existing terminal color resolver.** Application-owned truecolor and background values follow theme and terminal overrides consistently, while fixed ANSI and graphic colors retain their intended ownership.

### 简体中文

#### 新增
- **终端卡的几何收成了一组可配的键——圆角、卡缝、投影，以及左侧 Tab 栏与终端之间的竖线——并且跟随主题走。** 「浮在壳上的圆角卡」与「终端铺满整个右侧区域、靠一条细线分界」不再是两套布局、也不是两条渲染路径，而是同一条路径上的两组取值。`Nord` 是第一个采用后者的主题：圆角与卡缝双双归零，由一条 1px 竖线承担结构分界——卡缝一旦消失，侧栏与终端就会糊成一整块。其余主题保持浮起的卡片，并且刻意**不画**竖线：已经有卡缝了，再加一条线是在重复同一件事。`nebula_settings.txt` 里的 `pane_card_radius` / `pane_card_gutter` / `pane_card_shadow` / `pane_card_divider` 会覆盖主题的要求，于是切主题能换形态，而手调过的人不会被主题夺回控制权。圆角默认值同时收敛为单一真源：旧壳的 `UI_SHELL_RADIUS_LOGICAL` 与新壳的卡现在引用同一个常量，不再各自持有一份字面量——同一个数字存两份，正是那圈白边的来源。
- **GPUI 设置现已内置 English 与简体中文语言 catalog。** 语言偏好可以跟随系统，也可以显式选择任一语言；选择会跨启动保留，并在运行时更新设置导航、控件、占位文案、Provider 测试、网络测试、SSH 状态及其他已迁移的设置消息，无需从磁盘读取翻译文件。缺少本地化消息时回退 English，配置值与显示文案保持分离，构建阶段会拒绝消息 ID 集合不一致的 catalog。此项对应 [#71](https://github.com/Kuddev/nebula/issues/71) 的 GPUI 设置页范围；其他尚未翻译的界面不在本次发布范围内。
- **Windows 新增 GPUI 独立快速终端窗口。** 已配置的全局快捷键会打开一个进程级单例窗口，定位到最近活跃普通工作区所在的显示器，使用显示器全宽和 40% 高度从顶部滑入；窗口隐藏期间继续保留 PTY、滚屏和终端状态。修改快捷键后会向 Windows 重新注册，同一轮重复按键会合并成一次显示或隐藏操作。
- **新增版本化 Runtime 控制 API 与资源式 CLI，为自动化提供统一、可发现的 Nebula 操作入口。** `nebula env` 会报告当前 pane 身份、控制面可执行文件、支持的命令和运行时连通性；`nebula window`、`nebula tab`、`nebula pane`、`nebula agent` 通过同一 JSON 协议提供读取、发送、等待、关闭、重命名、移动、分屏、缩放、调整比例、运行、粘贴和执行操作。Bash、Fish、Zsh 补全同步覆盖新的命令面。
- **新增强类型多步编排，可在一次请求中建立 Agent 工作区。** 工作流可以创建标签、分屏、并行启动经过核实的 AI Provider、等待指定 generation ready，再投递首个 Prompt。后向步骤引用、受限模板、显式 partial 失败回执以及不隐式回滚，让后续步骤失败时已经完成的动作仍然可见、可续作。
- **托管 Agent 新增稳定名称、generation 身份和事务化 Git worktree 分叉。** 冷启动、恢复和分叉只开放已核实的 `claude`、`codex`、`opencode`、`cursor`、`pi`、`omp`、`kimi` 命令；fork 结果保留 branch、路径、来源和 base commit provenance，拒绝脏源或冲突，并清理失败的事务。原生 SSH pane 会明确报告不支持 worktree 分叉，不会伪装成已创建远端 checkout。
- **本地终端 pane 新增稳定自动化环境契约。** `TERM_PROGRAM`、`TERM_PROGRAM_VERSION`、`NEBULA_PANE_ID`、`NEBULA_CLI`、`NEBULA_BIN_DIR` 用于识别 pane 和控制面；WSL 通过幂等的 `PATH` / `WSLENV` 与路径转换获得入口，原生 SSH 只收到远端标记，不会宣称拥有 host 本地控制面可执行文件。
- **`pane.exec` 可以在 pane 的本地或 WSL 工作上下文中直接运行 argv，作为独立非 TTY 子进程执行。** 它不会改变 shell 历史、交互终端、cwd 或 pane 环境；会并发捕获 stdout/stderr，返回退出状态、编码、截断和超时元数据，并限制输出保留量。超时或父进程提前退出时通过 Windows Job Object 或 Unix process group 清理后代。原生 SSH 会返回 `remote_exec_unsupported`，不会静默改在别处执行。
- **pane 与 live 托管 Agent 支持从字面量、stdin 或文件进行受限 bracketed paste。** Prompt、paste、控制键、argv 和多行输入校验会阻止任意终端字节绕过公开 API；generation-aware 目标绑定也会避免延迟输入落入同名但已经重启的 Agent。
- **Quick Jump 可以在同一面板中搜索标签、pane、常用目录、已配置 SSH 主机和可恢复 AI 会话。** 结果保留标签与 pane 位置、cwd、远端目标、Provider 和会话元数据，并提供直接聚焦、打开、启动或恢复操作。Files 与 Recipes scope 因后端尚未端到端完成而继续隐藏。
- **终端或文档选区可以发送给用户明确选择的 live Agent。** 可搜索的 Send to Chat 对话框会把选区预览为引用、保留空行、接受可选评论、支持复制组合消息、标识 SSH 远端目标，并在发送后报告目标。没有 live Agent 时该操作会禁用，不会猜测 pane。
- **Windows 支持在 Nebula 窗口之间拖动终端标签。** 标签既可成为目标窗口的新标签，也可把完整 split tree 停靠到左、右、上、下区域；终端 pane、Runtime 所有权、订阅、焦点与会话元数据会一起迁移。移动最后一个标签时会关闭空源窗口；如果目标在提交前消失，源标签继续留在原窗口。
- **Windows 任务栏进度会反映终端应用发出的 OSC 9;4。** 普通、不确定、错误、暂停和清除状态映射到原生任务栏指示器；不支持或未知状态会清除旧进度，不会让任务栏卡在过期状态。
- **GPUI 文件树为本地和 WSL 条目提供完整右键操作。** 用户可以打开文件或目录、在资源管理器中显示、复制 host 或 Linux 路径、在目录中启动终端，并在确认后把文件送入回收站。WSL 路由保留发行版和 guest path，不会被压成无效 host 路径。

#### 修复
- **输出不再停在半帧、缺 CLI 输入框。** 1.2.0 那次修 `piper` 丢唤醒只补了一半：它在「读取返回的那一刻管道仍有货」时补投 readable，但 `drain_inner` 是**在拷贝数据之前**就摘掉 read waker 的——因此「这次读到过数据」本身就意味着此刻没有 waker，与管道当下看起来空不空无关。AI CLI 的收尾字节恰好落在这个窗口里：先打完一屏 diff 把 `pty_read` 顶过 64 KB 的 `MAX_LOCKED_READ` 提前 break、管道刚好排空，紧随其后那几百字节的输入框与状态栏就撞上空 waker，它们的 `wake()` 成了空操作。这批字节于是滞留在管道里，直到一次按键（翻转写意图、触发 reregister 并补投）或一次 resize（走强制排空）才被放出来；而拖动标签两者都不做，所以永远不会自行恢复。卡住 pane 的活体取证：一个 no-op 方向键一次性放出了 527 行滞留输出。现在补投只以「这次读到过数据」为判据，并且自收敛——紧接着的那次读取会遇到空管道，piper 借此重新注册 waker。
- **`$E=mc^2$`、`$n!$` 等常见公式可以被正确识别，同时不会把 shell 变量、提示符、价格或 `sed`/`grep` 的 BRE 分组误当成数学公式。** 行内定界符可以跨终端软换行，但不会再连接互不相关的真实换行。
- **还原设置下拉框后会立即显示默认选项**，不再出现设置值已经写回、界面却仍停留在旧选项的情况。
- **浅色与深色主题下，圆角终端 pane 四周不再露出线框或顶部缝隙**；壳层绘制现在严格采用 pane 真实的左、右、下卡缝以及零宽度上卡缝。
- **Windows 新建终端 pane 不再继承 Nebula 常驻进程里的过期环境变量。** 每个 pane 会从当前机器和用户注册表重建环境，按 Windows 顺序合并 `PATH`，以大小写不敏感方式处理变量名，基于刷新后的快照展开注册表值，在注册表项消失时保留进程私有值，并最后应用 pane 专属覆盖项。GPUI pane、旧壳 pane 和 WSL 启动都会使用同一份刷新结果。对应 [#68](https://github.com/Kuddev/nebula/issues/68)。
- **GPUI 快速终端快捷键在 Nebula 之外也能正常响应。** 第一个工作区打开时会向 Windows 注册已配置组合键，设置变化后会同步更新；如果新组合键注册失败，仍会保留有效的旧注册。快捷键会定位最近活跃的普通工作区，不要求 Nebula 已经处于焦点。
- **快速终端首次打开不再出现空白条，也不会停在普通窗口尺寸。** 原生窗口尺寸与终端表面会在窗口仍位于屏幕上方时完成一次同步，入场结束后再确认全宽、40% 高度的几何，但不会反复 resize PTY 或 swapchain。快速终端同时从普通窗口 MRU、runtime 命令定位、会话恢复、显示全部窗口和普通工作区持久化中隔离，隐藏或关闭它不会改写正常工作区会话。
- **显示数学定界符不再吞掉无关正文或后续公式。** 跨行 `$$...$$` 与 `\[...\]` 只有在真正的块起始上下文中才会配对，并会在空行展示边界停止，因此正文里提到的定界符、或滚屏顶部孤立的结束符不会再错误连接后面的公式块。
- **只负责展示的 `\boxed{...}` 与 `\tag{...}` 不再让原本有效的公式回退成原始 TeX。** 当前数学后端无法绘制边框时仍会渲染盒内内容，公式编号也会作为行内编号保留下来。
- **`\text{...}` 中的空格等合法无墨字形不再导致整条公式消失。** 有前进宽度但没有轮廓的字形会生成空位图，而不是触发缺字错误；如果公式仍然无法合成，终端原始单元格会继续显示，不会留下一块空洞。
- **侧栏拖拽现在跟随主题真正的边界。** Nord 的铺满终端布局会把 resize 光标和宽度计算对准 1px 分隔线，浮起卡片主题则继续使用卡缝，因此可见竖线不会再落后于鼠标指针。
- **Tab 徽章不再让兜底信号覆盖权威状态。** Agent 授权和交互提示使用手掌标记，命令非零退出后会保留警示三角直到下一条命令开始，后台成功完成时会短暂显示对勾，再沉降为未读完成标记。只有尚无 Agent 规则判定 pane 状态时，末尾 BEL 才能请求关注，因此已经完成的 Agent 回合不会再被误标成等待输入。
- **Runtime 完成边沿不再串入仍在提交的新命令，补全弹窗尚未选中时也不会抢走 shell 历史方向键。** 在按 `Tab` 或用鼠标选择候选前，`Up` / `Down` 仍交给 shell；建立选中后，同样的方向键会按预期导航候选列表。
- **GPUI 光标闪烁现在与设置页和旧壳保持一致。** 未设置 `cursor_blink` 时，设置页与终端都会统一启用闪烁；窗口或 pane 失焦、以及输入法预编辑期间会暂停，焦点或组合状态变化时会先显示光标再重新开始。间隔统一为 750ms，过期 timer generation 不会在已不允许闪烁时再次隐藏光标。
- **设置页输入非 ASCII 代理文本、包括全角标点时不再崩溃。** 大小写不敏感的 `socks5://`、`socks5h://`、`socks://`、`http://` 匹配现在遵守 UTF-8 字符边界，同时继续兼容裸 `host:port`。对应 [#67](https://github.com/Kuddev/nebula/issues/67)。
- **GPUI 标题栏的文件树与 Git 控件不再触发窗口拖动。** 左键按下事件会在到达可拖拽标题栏前停止，同时真实按钮点击仍会打开对应面板。对应 [#62](https://github.com/Kuddev/nebula/issues/62)。
- **GPUI 诊断与 Windows panic 报告不再把一次可恢复的诊断失败升级成二次 panic。** GUI 日志使用可失败 stderr sink，GPUI 模块内直接调用 `println!` / `eprintln!` 会在编译期被拒绝。Windows 会先把 panic 文本保存到 `nebula-panic.log`，再由专用线程显示错误对话框，避免模态消息循环在原始 panic 期间重新进入 GPUI。对应 [#70](https://github.com/Kuddev/nebula/issues/70)。
- **选中终端宽字符时会完整高亮它占用的两列。** 无论命中首格还是 spacer，CJK 字符、全角标点与 emoji 都只生成一个不重叠的选区 run；简单、块、语义和整行选区会复制完整字符，不再只显示或复制半个字形。
- **Runtime 命令不再因为路由而显示或激活目标窗口。** Snapshot、读取、Prompt、paste、run、exec、布局和 Agent 操作都会保留当前前台窗口；Runtime 新建窗口采用非激活显示语义，只有显式 `focus` 命令会激活隐藏或后台工作区。
- **顶栏标签与侧栏操作可以正常点击，不再把事件泄漏给父级拖动或折叠处理。** 终端标签、顶栏新建与更多按钮、侧栏新建按钮会在操作边界停止 mouse-down，保持统一的 34px 对齐，也不会同时启动标题栏拖动或切换 `TABS` 分组。
- **侧栏分隔线现在是一条从窗口顶部延伸到 pane 卡底部、按物理像素对齐的连续线。** 它不再停在标题栏下方，也不会因两段独立绘制造成接缝；没有真实侧栏边界的布局不会绘制该线。
- **Runtime Agent 通知现在会关联到正确的 pane 和进程。** Windows 进程祖先证据可以穿过中间 shell 找到 hook helper、按 PID 区分嵌套 Agent，并拒绝其他 pane 的事件，而不是信任短命 session id；无法取得远端进程证据时会明确报告，不会伪造。
- **Runtime wait 不再把提交前已经 idle 的 pane 误判成刚提交的命令已经完成。** 提交操作会先建立新的 running / 状态变化边沿，Agent wait 始终绑定请求的 generation；没有可靠 OSC 退出证据的命令不会被报告为已验证成功。
- **Runtime 布局变更在目标歧义或不可用时不再猜测。** 目标歧义、pane 忙碌、生命周期过期、远端不支持和目标无效都会返回明确错误，不会静默回退到碰巧处于当前状态的窗口或 pane。
- **本地选区不会再被程序路由静默发送给 SSH Agent。** 自动发送会返回 `remote_target_refused`；用户仍可以在 Send to Chat 对话框中主动选择带远端标识的目标。
- **GPUI 文件树会把长列表和状态消息限制在圆角抽屉内部。** 行密度、裁剪、滚动和底部 padding 不再互相冲突；空态会区分读取中、枚举失败、缺少 cwd 与真正空目录，并提供对应的重试或导航操作。
- **WSL 文件树条目会保留 guest path 和发行版。** 文件通过对应的 `\\wsl.localhost` 路由打开，目录在 Linux cwd 处启动终端且不会强制 Bash 覆盖发行版配置的默认 shell，资源管理器操作则使用匹配的 host 可见路径。
- **使用 DECSET 2031 的应用现在会收到一致的终端色彩方案通知。** 订阅与 OSC 11 使用同一个明暗判据，会立即报告当前方案、避免重复通知，并在运行中切换主题后更新。
- **ConPTY 光标对账现在能处理 resize 漂移的两个方向。** 无论 conhost 在本地光标上方折叠了行，还是在下方产生更多换行，Nebula 都会校正内容；resize 爆发会合并处理，同时保留最终几何和必要的完整 PTY 通知。

#### 改进
- **终端 TeX 渲染改为根据终端内容识别公式，不再依赖识别 AI 进程名称**，因此块级公式可继续用于 WSL、SSH 和全屏 Agent 界面。标准 `$...$`、`$$...$$`、`\(...\)`、`\[...\]`、Markdown 损坏后的裸定界符、`\frac13` 等紧凑分式参数、DIM 推理输出、单元格背景和持久化公式重叠都能得到更可靠的处理。包含 [@liu-ws](https://github.com/liu-ws) 在 [#55](https://github.com/Kuddev/nebula/pull/55) 中贡献的终端数学工作。
- **GPUI 现在沿用旧壳的行内终端数学紧凑投影合同。** 公式按实际测得的视觉宽度占位，后续文本会收拢源单元格留下的空隙；ANSI 背景、选区、框线字形、各种光标、幽灵补全、补全弹窗、输入法锚点和链接预览全部使用同一套投影列。鼠标选择与链接命中仍会映射回未改动的终端源坐标，也覆盖非零滚屏起点。活动选区与 VI 模式会显示原始 TeX，备用屏应用保持 TUI 固定列，流式重叠公式共享同一存活集合，字体或位图失败时回退源文本，不会遗留旧位移或隐藏单元格。
- **快速终端运动现在与既有滑入体验一致。** 入场使用 120ms swift-out，退场使用 90ms ease-in，中途反向切换会从当前位置继续，不会跳到错误端点。帧回调只移动原生窗口 Y 坐标，不重复激活、不 resize、不改 z-order；用户主动召回时只在入场前聚焦一次，退场动画结束后才真正隐藏原生窗口。
- **Nord 现在是出厂默认深色主题，系统使用浅色外观时配对 Paper。** 新安装、缺少主题值或主题值无效时，旧壳与 GPUI 壳会采用同一个默认来源；用户显式选择的主题不会被覆盖。
- **Runtime snapshot 现在可直接用于自动化与审计。** 它会暴露 revision、语义化 pane 生命周期和任务状态、缩放状态、进程证据、稳定 Agent 身份、generation 和 worktree provenance；Provider 操作使用已核实的冷启动、恢复和分叉语法，不再根据显示名称猜命令。
- **GPUI 设置导航改为保持稳定路由的扁平列表。** 左侧不再显示分组标题，默认顺序为“应用、外观、配置文件、交互、按键映射、SSH、网络、高级”。“AI 供应商”和“备份”仍保留实现及路由索引，但暂时不进入默认侧栏。
- **GPUI 会话驻留与托盘操作现在遵守 live pane 所有权。** 配置启用时关闭普通工作区会隐藏窗口而不杀 PTY；托盘操作可显示正确工作区并聚焦指定 Agent pane，显式从托盘退出则会先保存 session 再关闭。快速终端窗口不参与普通 session 生命周期。
- **GPUI 终端渲染与旧壳共享 Terminal color resolver。** 应用拥有的 truecolor 和背景值会一致遵循主题与终端覆盖设置，固定 ANSI 和图形颜色则保持原有归属。

## 1.3.1 - 2026-08-25

### English

#### Added
- **Verified in-app Windows updates** — Nebula can download the exact official Windows x64 installer in the background, show progress, validate its size, PE header, and SHA-256, then wait for explicit confirmation before launching setup.

#### Fixed
- **Automatic update checks use the real GitHub latest release** and compare it with the running package version before showing a reminder; skipped versions and three-day reminders remain respected.
- **Update notifications remain visible in the bottom-right corner** until handled, and download completion or failure replaces the original reminder with the appropriate next action.
- **Update and confirmation dialog actions, geometry, and typography are restored**: required confirmation actions are available and execute correctly, the dialogs match the established width, vertical position, and font sizing, and overlay clicks, Escape, and cancel buttons consistently run the cancel action. Addresses [#64](https://github.com/Kuddev/nebula/issues/64).
- **Setting terminal opacity to 100% no longer returns to 82%** when legacy blur settings are normalized and reloaded. Addresses [#60](https://github.com/Kuddev/nebula/issues/60).
- **The keymap page no longer leaves a large empty gap** above its shortcut rows.
- **The font group popup stays anchored to its selector**, and hovering a truncated font family reveals its complete name.
- **The sidebar resize cursor and width calculation align with the visible divider**, so dragging starts directly on the baseline instead of several pixels to its left. Addresses [#61](https://github.com/Kuddev/nebula/issues/61).
- **PowerShell reports an absolute current directory to Files and Git integrations** while retaining `~` as the compact prompt display, restoring repository detection beneath the Windows home directory.

#### Improved
- **The update flow exposes download, retry, verified, install, and failure states** and revalidates a downloaded installer immediately before it is launched.
- **Modal cancellation is consistent across workspace, pane, tab, file-tree, and settings confirmations** without removing the new component's click-outside behavior.

### 简体中文

#### 新增
- **新增经过校验的应用内 Windows 更新** — Nebula 可在后台下载名称精确匹配的官方 Windows x64 安装包，显示进度，校验大小、PE 文件头与 SHA-256，并在明确确认后才启动安装。

#### 修复
- **自动更新改为读取真实的 GitHub 最新 Release**，与当前运行版本比较后才显示提醒，同时继续遵守跳过版本与三天后提醒设置。
- **更新通知会持续显示在右下角**，直到用户处理；下载完成或失败后，原通知会被带有下一步操作的新状态替换。
- **更新与确认弹窗恢复必要操作以及既有的宽度、垂直位置和字体大小**；确认操作会正常显示并执行，同时让遮罩点击、Esc 与取消按钮一致执行取消逻辑。对应 [#64](https://github.com/Kuddev/nebula/issues/64)。
- **终端不透明度拉到 100% 后不再回到 82%**，旧版模糊设置在规范化和重载时不会再次覆盖用户选择。对应 [#60](https://github.com/Kuddev/nebula/issues/60)。
- **按键映射页不再在快捷键列表上方留下大块空白。**
- **字体组弹层会持续贴住下拉框**，鼠标悬停在被截断的字体名称上时可查看完整名称。
- **侧边栏拖拽光标和宽度计算与可见分隔线严格对齐**，无需把鼠标偏到基线左侧才能开始拖动。对应 [#61](https://github.com/Kuddev/nebula/issues/61)。
- **PowerShell 向文件与 Git 集成上报绝对当前目录**，提示符仍可用 `~` 紧凑显示，从而恢复 Windows 用户主目录下的仓库识别。

#### 改进
- **更新流程提供下载、重试、已校验、安装和失败状态**，并在启动安装包前立即执行二次校验。
- **工作区、分栏、标签、文件树与设置确认框采用一致的取消行为**，同时保留新组件点击外部区域取消的特性。

## 1.3.0 - 2026-08-24

### English

#### Added
- **Paged title-bar tabs and pane headers** — overflowing top tabs expose previous/next controls with edge auto-scroll, while split panes receive their own draggable headers and can be detached back into tabs.
- **Per-tab broadcast input** — one action can send text and control keys to every pane in the active tab, re-encoding the input for each pane's terminal mode.
- **A semantic completion popup** — history, commands, directories, Rust, TOML, Markdown, and other files use distinct vector icons, source tags, and colors; the popup flips above the cursor near the bottom edge and shares one pixel geometry for painting, clicking, hovering, and scrolling.

#### Fixed
- **Finished AI commands no longer remain stuck in the running state** — process-tree evidence now disproves stale shell state after Codex or Claude has returned to an idle prompt. Addresses [#42](https://github.com/Kuddev/nebula/issues/42) and [#54](https://github.com/Kuddev/nebula/issues/54).
- **Windows task switching uses the Nebula icon** — GPUI windows now set both native icon sizes, fixing the default white placeholder in taskbar previews and Alt+Tab. Addresses [#51](https://github.com/Kuddev/nebula/issues/51).
- **The SSH host editor renders and scrolls its complete form** — the modal now has a measured responsive viewport, and its icon picker restores a bounded scrollbar without leaving empty space for short filtered lists.
- **Native backdrop transparency stays synchronized** with wallpaper, terminal opacity, update dialogs, and the Windows material state.
- **Settings selectors and font profiles refresh after changes**, including imported/private font installation and the SSH/sidebar/top-tab surfaces that consume those settings.
- **WSL launches preserve the guest account's default shell**, and WSL file-tree paths map correctly between guest and UNC forms.
- **AI completion notifications expire after 90 seconds** instead of accumulating indefinitely.

#### Improved
- **The product shell now targets GPUI v1.16.1** while retaining Nebula's resize, split-tree, mathematical layout, and PTY ownership contracts.
- **Settings was rebuilt around a consistent two-column layout** with fixed control alignment, clearer descriptions, non-default markers, a copyable About view, and dedicated font and keymap pages.
- **Nebula theme tokens now drive current component states** for buttons, sliders, switches, window chrome, transparency, and drag-time fast paths.
- **Native Windows backdrop materials and split-pane chrome are integrated directly** instead of relying on a parallel legacy backdrop path.

### 简体中文

#### 新增
- **可分页的标题栏标签与 pane 标题栏** — 顶部标签溢出后显示前后翻页控件并支持边缘自动滚动；分屏 pane 拥有独立的可拖动标题栏，也可以重新拖出为标签。
- **按标签广播输入** — 一次操作可把文字与控制键发送到当前标签的全部 pane，并按每个 pane 自己的终端模式重新编码。
- **语义化补齐弹窗** — 历史、命令、目录、Rust、TOML、Markdown 与普通文件使用各自的矢量图标、来源标签和颜色；光标靠近窗口底部时弹窗向上翻转，绘制、点击、悬停与滚动共用同一份像素几何。

#### 修复
- **AI 命令结束后不再错误保持“运行中”** — Codex 或 Claude 回到空闲提示符后，进程树证据会推翻过期的 shell 状态。对应 [#42](https://github.com/Kuddev/nebula/issues/42) 与 [#54](https://github.com/Kuddev/nebula/issues/54)。
- **Windows 任务切换显示 Nebula 图标** — GPUI 窗口现在设置两种原生图标尺寸，修复任务栏悬停预览与 Alt+Tab 中的默认白框占位图。对应 [#51](https://github.com/Kuddev/nebula/issues/51)。
- **SSH 主机编辑器完整显示并可滚动** — 弹窗使用经过测量的响应式视口；图标选择器恢复有界滚动条，过滤结果较少时也不会留下多余空白。
- **原生背景材质透明度保持同步**，覆盖壁纸、终端不透明度、更新弹窗与 Windows material 状态。
- **设置选择器与字体 profile 会在修改后刷新**，包括导入/私有字体安装，以及消费这些设置的 SSH、侧栏与顶部标签界面。
- **WSL 启动保留来宾账号的默认 shell**，文件树路径也能在来宾路径与 UNC 形式之间正确映射。
- **AI 完成通知会在 90 秒后自动消失**，不再无限堆积。

#### 改进
- **产品壳升级到 GPUI v1.16.1**，同时保留 Nebula 自有的 resize、split-tree、数学布局与 PTY 所属关系。
- **设置页重建为一致的两列布局**，统一控件对齐、说明文字和非默认值标记，并提供可复制的 About 页面及独立的字体、按键映射页面。
- **Nebula 主题 token 驱动当前组件状态**，覆盖按钮、滑块、开关、窗口 chrome、透明度与拖动快速路径。
- **原生 Windows 背景材质与分屏边框直接整合**，不再维护一条平行的旧 backdrop 路径。

## 1.2.0 - 2026-08-21

### English

#### Added
- **Deterministic Runtime orchestration** — `nebula ctl orchestrate` accepts one typed request and executes bounded local control steps with workflow-specific `stop`/`continue` failure handling.
- **Expanded Runtime control APIs** — named-agent operations, process and control-key commands, real command exit status, pane lifecycle events, and a stabilized transport/discovery path are available to automation clients.
- **Transactional managed-agent worktrees** — agent forks create isolated branches and worktrees, retain stable agent identity, and roll back resources only when failure is confirmed.
- **Packaged Runtime contract** — the versioned schema, control API documentation, and `nebula-runtime` Skill ship in both the ZIP and installer.
- **Horizontal title-bar tabs** — top-mode tabs support selection, drag reordering, docking and splitting, plus horizontal auto-scroll while selecting or dragging. Settings now lives in the three-dot menu, and the first tab aligns with the terminal/powerline edge.
- **SVN integration** — the GPUI repository workflow and status panel now support SVN working copies alongside Git.
- **SSH failure retry** — both shells show Retry immediately to the left of Close only after a connection failure and replace that failed pane in place.
- **The viewport auto-scrolls while drag-selecting** — dragging a selection past the top or bottom edge of the grid used to stop dead; it now keeps scrolling the scrollback with the legacy shell's ramp (1 line at the edge, +1 per 20px, 15ms cadence). The timer is armed on mouse-down rather than on the first out-of-grid move, so a fast flick past the edge still triggers it, and releasing over the tab strip or off-window no longer leaves the selection stuck on.

#### Fixed
- **Output could stop mid-frame until you pressed a key** — `piper` only registers its read waker on the read that finds the pipe empty, and drops it as soon as a read returns data, while the writer's `wake()` is a no-op against an empty waker. So when the reader stopped early (`pty_read` returns at its 64KB cap) and the source then went quiet — an AI CLI finishing its answer — the remaining bytes stayed in the pipe forever: the screen froze on a half-painted frame with the scrollbar still at the bottom, and only a keystroke or a resize could recover it. The level-triggered contract is now honored by re-posting the wakeup while data remains.
- **Second launches reach the resident window** — opening Nebula again hands the request to the running instance and creates the requested tab/window instead of exiting silently. Addresses [#39](https://github.com/Kuddev/nebula/issues/39).
- **Sogou and other IMEs keep ownership of unmodified text input** — ordinary letters and Space remain on the text-input path so IME composition can be confirmed. Addresses [#43](https://github.com/Kuddev/nebula/issues/43).
- **WSL keeps the guest's configured default shell** — Nebula no longer forces Bash when launching `wsl.exe`. Addresses [#44](https://github.com/Kuddev/nebula/issues/44).
- **Ctrl+C is conditional copy again** — with no selection it reaches the PTY; after an explicit copy, the selection is cleared before the next key event. Addresses [#48](https://github.com/Kuddev/nebula/issues/48).
- **Legacy `user@host:port` SSH targets no longer fail the Windows configuration probe with `11001`** — only the `ssh -G` probe target is normalized to `ssh://user@host:port`; the actual connection destination is preserved.
- **Codex Shift+Enter is encoded as a Win32 input record** — ConPTY Win32 input mode and DECSET 9001 tracking preserve the modified Enter key.
- **Background blur can be toggled at runtime** and hidden windows recover correctly when tray support is disabled.
- **Opacity sliders no longer rebuild fonts, themes, wallpaper textures, or DWM state on every drag event.**
- **WSL file trees no longer report cold or timed-out reads as empty folders** and the Files panel follows the active local/WSL/SSH pane after switches and manual navigation.
- **PowerShell startup injection is present before the first prompt** for three-dot launches, powerline-disabled sessions, and alternate profile paths.
- **Checked-in bash, fish, and zsh completions match the CLI** for `ctl orchestrate`, `--gpui`, and `--legacy-shell`.
- **Installer fallback and numeric version metadata now follow the application release version.**

#### Improved
- **Completion is routed through each pane's filesystem** — local, WSL, and SSH panes fetch candidates from their own environment instead of the host filesystem.
- **WSL tree enumeration batches starting points into one `find` call** and uses a realistic cold-start timeout; refreshes are event-driven instead of running every four seconds.
- **Shell injection is selected by shell family** so PowerShell, cmd, Nushell, and fallback shells keep their compatible startup/completion behavior.
- **Mixed-DPI window dragging coalesces intermediate work** and separates the native move transaction from the final DPI/size commit.
- **GPUI split-pane chrome and title-bar integration were aligned**, while the sidebar mode and its settings control remain unchanged.
- **The shell-injected `fmt` command was removed** and the expanded sidebar toggle now uses the legacy shell's softer selected surface.
- **README Star History rendering was restored** by [@FaintFlower](https://github.com/FaintFlower) in [#49](https://github.com/Kuddev/nebula/pull/49).

### 简体中文

#### 新增
- **确定性的 Runtime 编排** — `nebula ctl orchestrate` 接收一次强类型请求，在本地执行受步数、字节上限和就绪超时约束的控制步骤，并支持按工作流选择 `stop`/`continue` 错误处理。
- **扩展 Runtime 控制 API** — 新增命名 agent 操作、进程与控制键命令、真实命令退出码、pane 生命周期事件，并稳定了传输与发现路径。
- **事务式托管 agent worktree** — agent fork 会创建隔离分支与 worktree，保存稳定 agent 身份，并且只在确认失败时回滚本次创建的资源。
- **随包提供 Runtime 契约** — ZIP 与安装器都包含带版本的 schema、控制 API 文档和 `nebula-runtime` Skill。
- **水平标题栏标签页** — 顶部模式支持选择、拖拽排序、停靠与分屏，选择或拖拽时可水平自动滚动；设置入口已收进三个点菜单，首个标签与终端/powerline 左边缘严格对齐。
- **SVN 集成** — GPUI 仓库工作流与状态面板现在除 Git 外也支持 SVN 工作副本。
- **SSH 失败重试** — 两套界面都只在连接失败后显示“重试”，位置紧邻“关闭”左侧，并在原位替换失败的 pane。
- **拖动选区时视口自动滚动** — 以前把选区拖过网格上下边缘就停住了，现在会继续滚动回滚缓冲，斜率与旧界面一致（贴边 1 行，每多 20px 加 1 行，15ms 一拍）。计时器在按下时就武装，而不是等第一次「移出网格」的移动事件，所以快速甩到边缘也能触发；在 tab 栏上或窗口外松手也不会再让选区状态卡住。

#### 修复
- **输出会停在半帧，要按一下键才继续** — `piper` 只在「读到管道为空」的那一次注册 read waker，一旦读到数据就立刻把它摘掉，而写入侧的 `wake()` 对空 waker 是空操作。于是当读取方提前停下（`pty_read` 到 64KB 上限就返回）而源头随即安静下来——比如 AI CLI 答完就不再输出——剩下的字节便永久滞留在管道里：画面冻在一个画到一半的残帧上，滚动条却仍在底部，只有按键或 resize 能救回来。现在只要管道里还有数据就补投一次唤醒，兑现电平触发的承诺。
- **第二次启动会交给驻留窗口处理** — 再次打开 Nebula 会由现有实例创建所需标签/窗口，不再静默退出。对应 [#39](https://github.com/Kuddev/nebula/issues/39)。
- **搜狗等输入法继续接管未修饰文本输入** — 普通字母与空格保留在 text-input 路径，输入法组合文本可以正常确认。对应 [#43](https://github.com/Kuddev/nebula/issues/43)。
- **WSL 保留来宾系统配置的默认 shell** — 启动 `wsl.exe` 时不再强制使用 Bash。对应 [#44](https://github.com/Kuddev/nebula/issues/44)。
- **Ctrl+C 恢复条件复制语义** — 没有选区时按键传给 PTY；明确复制后，在下一个按键事件前清除选区。对应 [#48](https://github.com/Kuddev/nebula/issues/48)。
- **旧格式 `user@host:port` 不再让 Windows SSH 配置探测报 `11001`** — 只把 `ssh -G` 探测目标规范化为 `ssh://user@host:port`，实际连接目标保持不变。
- **Codex 的 Shift+Enter 会编码为 Win32 输入记录** — ConPTY Win32 输入模式与 DECSET 9001 跟踪会保留带修饰键的 Enter。
- **背景模糊开关可在运行时生效**，关闭托盘时也能正确找回之前隐藏的窗口。
- **拖动不透明度滑块不再逐事件重建字体、主题、壁纸纹理或 DWM 状态。**
- **WSL 文件树不再把冷启动或超时误报为空目录**；切换 pane 或手动导航后，Files 面板会继续跟随当前本地/WSL/SSH pane。
- **PowerShell 在第一条提示符前完成启动注入**，覆盖三个点启动、关闭 powerline 和备用 profile 路径。
- **签入的 bash、fish、zsh 补全脚本与 CLI 重新同步**，补齐 `ctl orchestrate`、`--gpui` 与 `--legacy-shell`。
- **安装器兜底版本与数字文件版本会跟随应用发布版本。**

#### 改进
- **补全按 pane 使用各自的文件系统** — 本地、WSL 与 SSH pane 都从自己的环境获取候选，不再错误读取宿主文件系统。
- **WSL 文件树把多个起点合并成一次 `find`**，采用符合冷启动实际的超时，并改为事件驱动刷新，不再每四秒全量扫描。
- **按 shell 家族选择注入逻辑**，让 PowerShell、cmd、Nushell 和通用 fallback 保持各自兼容的启动/补全行为。
- **混合 DPI 窗口拖动会合并中间更新**，原生移动事务与最终 DPI/尺寸提交彼此分离。
- **GPUI 分屏边框与标题栏整合已对齐**，侧边栏模式及其设置按钮保持不变。
- **移除 shell 注入的 `fmt` 命令**，展开的侧栏切换按钮改用旧界面更柔和的选中色。
- **README Star History 图表已恢复**，由 [@FaintFlower](https://github.com/FaintFlower) 在 [#49](https://github.com/Kuddev/nebula/pull/49) 中贡献。

## 1.1.0 - 2026-08-18

### English

#### Added
- **A rebuilt interface as the default window** — 1.1.0 packages open a new GPU-accelerated shell: the terminal grid, sidebar, settings, launcher and document views are all rendered by it. `--legacy-shell` still starts the previous winit UI, which stays in the codebase.
- **AI session forking from the sidebar** — right-clicking a claude or codex tab offers "Fork AI session", opening a new tab that resumes the same conversation through the CLI's own fork syntax. Eligibility is recomputed when the menu opens, so a session that has just reported its id is immediately forkable.
- **AI turn-state detection** — sidebar tabs show running, finished, waiting-for-input and failed states for claude and codex, driven by the CLIs' own hook payloads plus screen-state evidence. A watchdog releases zombie "working" states so the spinner cannot spin forever.
- **A full settings page in the new shell** — every key-value setting from the legacy UI is migrated with the same section navigation, and Settings opens as one reusable tab instead of a floating window.
- **Tab rename, color labels and single-tab export** — rename a tab in place, tag it with one of seven brand colors, or export just that tab as a workspace file.
- **Split panes, drag-to-split and workspace session restore** — dragging a tab onto a pane edge splits it, and the split tree with per-pane launch identity comes back on the next launch.
- **AI provider settings and a runtime control CLI** — provider credentials and models are configurable in Settings, and a runtime API lets scripts open tabs, run commands and query state.
- **A real GPUI agent-control loop** — the default shell now publishes its live workspace into the same RuntimeHub used by CLI waits and subscriptions. `nebula ctl agents` returns canonical agent identity, lifecycle state and evidence source; `nebula ctl read` reads the real terminal Grid/scrollback tail without moving the user's viewport; focus, prompt and wait act on the exact window/pane pair. SSH panes reject reads and prompts until their shell is ready, and unavailable GPUI multi-window creation now returns an explicit error instead of a synthetic success.
- **Isolated Git-worktree agents** — `nebula ctl agent-fork` creates a new branch and worktree on a Runtime worker thread, opens a real Tab/PTY in that checkout, and starts a named Codex or Claude generation. The stable Agent record keeps its branch, base commit and paths for later `agent-get`/prompt/read/wait calls. Dirty sources, existing branches/paths and SSH sources fail explicitly; a confirmed UI launch failure rolls back only resources created by that request, while an uncertain UI timeout keeps the checkout instead of invalidating a late-created pane.
- **A packaged Nebula Runtime skill and protocol contract** — release ZIPs and installers include the versioned Runtime API documentation/schema plus a `nebula-runtime` Skill that drives the list → observe → read → focus → prompt → wait workflow and treats terminal output as untrusted data.
- **Built-in box-drawing geometry** — frames, blocks and separators are drawn geometrically in the render contract instead of relying on font glyphs, so they align in any font.
- **Terminal mouse semantics parity** — application mouse reporting, right-click behavior and copy-on-select match the legacy shell.
- **A Markdown and image reader** — documents render with rich text, native math and images, and PNG/JPEG/WebP/BMP open in their own viewer tab.

- **Two completion styles with a switch** — Settings → Terminal → Completion adds a "Completion style" choice between the existing inline ghost text and a new popup candidate list. The popup gathers history, frecency directories, PATH commands, and filesystem matches (up to 8, with source tags), navigates with Up/Down, accepts with the configured accept key (Tab/Right), and dismisses with Esc without re-opening until the line changes. A command-palette action toggles the style, and the choice persists as `completion_style`.
- **A real color picker for the custom background** — the background-color popup now leads with a saturation/value plane and a hue bar; dragging picks continuously with live terminal preview and persists on release. The preset swatches and the hex field remain, and all three inputs stay in sync (including hue retention across gray/black/white picks).
- **Clipboard screenshot paste, locally and over SSH** — pasting with an image-only clipboard (e.g. right after Win+Shift+S) now converts the bitmap to PNG and pastes a file path instead of nothing. Local panes write a temp file; SSH panes upload via the existing SFTP stack to `/tmp/nebula-paste-<ts>.png` in the background and then type the remote path into the pane — so image-accepting CLIs like codex and claude receive a usable path on both sides.
- **Configurable new-tab placement** — Settings → Interaction can place newly created tabs immediately after the active tab or at the end of the tab list. The choice persists, while restored tabs keep their original ordering semantics.
- **Configurable terminal cell width** — Settings → Appearance offers Compact and Relaxed cell-width modes. Changing the mode immediately rebuilds font metrics, the terminal grid, pane layout, and PTY dimensions, and the choice persists across launches.
- **Ordered font fallback chains** — `font_family` accepts comma-separated families such as `JetBrains Mono, LXGW WenKai, Cascadia Code`. The first family remains the primary face, later families fill missing glyphs in order for regular, bold, italic, and bold-italic text, and the embedded Maple font remains the final fallback. System and imported/private fonts now use the same family lookup path for validation, preview, and rendering. (#33)
- **Middle-click to close tabs** — middle-clicking a tab or its close control follows the existing close flow, including confirmation when required.
- **Audible terminal bell** — BEL (`\a`) now plays the system notification sound in addition to the visual bell, so an AI CLI finishing a turn is audible even from another tab. Throttled so a bell-happy program cannot machine-gun it; disable with `bell.audible = false`. (#37)
- **AI conversations resume across restarts** — panes that had a claude/codex conversation open when Nebula closed (or crashed) now type the exact resume command (`claude --resume <id>` / `codex resume <id>`) into the restored shell automatically. Session identity comes from the CLIs' own hook payloads; a claude detected without an id falls back to `claude --continue` in the restored directory. Injection only targets plain restored shells (never SSH/profile seeds), ids are validated before anything reaches the terminal, and Settings → Advanced offers the "resume AI conversations on restore" switch (`resume_ai`, on by default).
- **System tray icon with agent attention** — a resident tray icon flips to an amber-dot state whenever any AI CLI stops and waits for input, even with every window buried or minimized. Right-click lists all agent panes with their state (running / waiting) and jumps straight to the source pane through the same focus path as toast clicks; left-click goes to the most urgent pane. The tray mirrors the sidebar badges (one source of truth) and can be turned off in Settings → Advanced (`tray`, on by default).
- **Line numbers for plain-text files (GPUI shell)** — double-clicking `.txt` / `.log` / `.json` / `.jsonl` files in the file tree now opens the code viewer with line numbers and per-line virtualization (same as source files) instead of the markdown document view; Markdown keeps the rich reader.
- **Clickable file and URL links in the GPUI shell** — OSC 8 hyperlinks and matched URLs draw a dashed underline in the cell's own foreground color. Hovering shows a preview (`decoded path · Ctrl+click`); Ctrl+click opens local `file://` paths in Explorer and other URIs with the default handler.
- **Configurable terminal bell in the GPUI shell** — Settings → Profiles → Terminal bell chooses Off, Flash, Sound, or Flash + sound. A BEL still plays the throttled system beep, briefly flashes the pane, toasts when the window is unfocused, and dots background tabs. The choice persists as `bell` (`none` / `visual` / `audible` / `both`, default both).
- **WYSIWYG font picker and folder-picker startup directory (GPUI shell)** — the font dropdown renders each family in its own face, strips junk extensions from display names, and the import button is just "Import font". Startup directory is a folder picker with "inherit current" / Clear, matching the legacy shell.

#### Fixed

- **Explorer context-menu launches join the resident instance** — "Open in Nebula" used to be treated as explicit intent and always started an independent window, so tabs detached in the resident process looked lost. A directory launch without `-e` now attaches the existing instance first (restoring its tabs) and opens the directory as a new tab in that window, brought to the foreground; without a resident instance it still starts standalone. The runtime API's `tab.new` gained an optional `cwd` parameter to carry this.
- **Imported terminals become selectable in the default-shell dropdown** — the dropdown's hit test counted only detected shells while the list also renders imported quick-launch profiles, leaving the trailing rows visible but unclickable. Both sides now share one count.
- **Git panel no longer blanks on repositories owned by another user** — `git status`/`diff` in the drawer run with a per-invocation `safe.directory` exemption, fixing the silently empty Git view on `\\wsl$\…` roots and elevated-owner checkouts where the same commands work in the user's own shell. Failures now leave a debug-log trace instead of a silent blank.
- **Visible Windows windows recover from stuck render gates** — startup-time occlusion misreports and missing frame callbacks can no longer leave an already visible window accepting input without repainting. Occlusion is ignored unless the window is actually minimized, the existing 1 Hz window heartbeat idempotently releases stale `occluded` and `has_frame` gates, and the watchdog is now armed at window creation so even a freeze before the first frame self-heals within a second. (#21, #32)
- **Drive-root context menu launches work** — right-clicking the background of `D:\` (any drive root) and choosing "Open in Nebula" failed with "invalid working directory": Explorer expands `%V` to `D:\`, whose trailing backslash escapes the closing quote on the command line. The mangled path is now repaired, and the error's log hint uses PowerShell syntax (`$env:NEBULA_LOG`) so copy-paste resolves. (#36)
- **Multi-line paste stops interrupting codex** — the multi-line paste confirmation now only fires when newlines are actually headed to a shell that would execute them line by line. Applications in bracketed-paste mode (codex, vim, modern PSReadLine) receive the paste as one atomic chunk, so the warning no longer blocks them. The dark-theme "Enter" keycap on the confirm dialog's accent button also derives from the button's own ink instead of the near-black panel color. (#35)
- **Math overlays survive markdown-unescaped delimiters (GPUI shell)** — some AI CLIs render their markdown before printing and eat the backslashes of `\[ \]` / `\( \)` (markdown punctuation escapes), leaving bare `[ … ]` blocks and `(\sqrt{…})` spans that never overlaid. The shared scanner now recognizes those bare forms when the content carries a known TeX command (`\int`, `\frac`, …) and the brackets sit alone on their line edges; JSON arrays, `[INFO]` logs and regex `(\d+)` stay literal. Explicit TeX lengths (`\\[6pt]`, `\kern`) also regained their point-to-pixel scale in the GPUI shell, matching the legacy renderer.
- **Markdown reader fills the reading column (GPUI shell)** — soft line breaks inside a paragraph (hand-wrapped README prose) rendered as hard line breaks, leaving the right side of the column empty and ragged. They now merge into spaces per CommonMark, with CJK neighbours joining directly.
- **Markdown images actually load (GPUI shell)** — the document tab now resolves relative image paths against the document's own directory, loads absolute local paths from disk, and fetches `http(s)` images through a ureq HTTP client on the background executor (gpui ships a null client by default), so README logos, shields badges (SVG included) and screenshots finally display. Animated GIF/WebP (local or remote) are flattened to a still PNG first, because gpui panics when a multi-frame image's frame index is reused on a static image.
- **Raw HTML in Markdown renders like GitHub (GPUI shell)** — README-style HTML now matches GitHub's semantics: `align="center"` on `<p>` / `<div>` / headings centers the block, consecutive `<img>` badges inside one paragraph flow on a single centered row instead of stacking one per line, inline `<br/>` breaks in document order (it used to be hoisted above the text it followed), and `<a><img/></a>` linked badges are no longer dropped — the image renders and carries its link.
- **Codex `notify` no longer wraps itself into an unusable config** — when another notify wrapper (codex-computer-use) re-registered without recognizing Nebula's helper, it serialized the whole existing array into its own `--previous-notify` argument; Nebula wrapped that again, and the escaped backslashes doubled every round until `config.toml` reached 130 MB and Codex refused to start. The helper mark now counts as "already wired" wherever it appears — including inside a JSON string — so Nebula only heals its own path and never re-wraps. A serialized `notify` over 8 KB is refused outright as a backstop against any other form of inflation, and `nebula setup-ai --remove` persists `ai_hooks=0` so auto-wiring stays off across launches. (#38)
- **Tab context-menu shadow no longer thickens with the tab count (GPUI shell)** — the component library's context-menu extension keys its state by a fixed element id, so every tab row resolved to the same state and re-rendered the same open menu at the same anchor. The menu panel is opaque, but its shadow is not: with eight tabs the drop shadow was composited eight times. The tab menu now draws exactly once from the workspace root, like the file-tree menu.
- **The terminal cursor no longer drifts after a resize** — conhost collapses the screen buffer non-deterministically on resize and repaints nothing, so the cursor could end up rows away from where the shell thinks it is. Nebula now reconciles the real position through an AttachConsole probe after each resize and re-aligns the grid.
- **The user PowerShell profile loads in the default shell** — the default launch skipped `$PROFILE`, so prompts, aliases and PSReadLine settings defined there never applied. (#30)
- **Reflow survives a shrink-then-grow round trip** — narrowing and then widening the window no longer drops wrapped content, and the window now has a real minimum size.
- **Cursor focus and blink, and a startup render-gate freeze** — the cursor state machine follows focus correctly, and a watchdog releases a startup gate that could leave the window visible but unpainted. (#21)
- **Esc reaches Claude Code in the new shell** — the win32-input encoder filled the control-key `uChar` with 0, which OpenConsole drops, so Esc never arrived at CLIs reading the byte stream.
- **The window-close confirmation accounts for busy processes** — closing while a build or an AI CLI is running now names the program instead of closing silently.
- **Notifications anchor to the bottom-right corner.**
- **The acrylic background composites at the configured opacity.**
- **Shell picker brand icons are no longer blurry** — they are pre-scaled to the integer device pixel size instead of being stretched by the GPU.
- **Built-in glyphs fill the cell in Relaxed width mode.**
- **Tab launch identity and full-row hit areas** — a restored tab keeps the program it was launched with, and the whole row is clickable rather than just the label text.

#### Improved

- **File/Git side-panel refreshes no longer block rendering** — directory traversal and Git status subprocesses now build a snapshot on a worker thread while the existing view stays usable. Completed snapshots are swapped in atomically; stale-root results are discarded and active search results are not overwritten.
- **Tab disclosure and file-tree menus in the GPUI shell** — the TABS collapse control uses the shell's linear chevrons instead of Nerd Font glyphs. File-tree context menus no longer pick up the drawer drop shadow, so they match the tab menus.
- **The font picker previews candidates in their own glyphs** — each family is drawn in its own face before you commit to it.

### 简体中文

#### 新增
- **重写的界面成为默认窗口** — 1.1.0 安装包启动全新的 GPU 加速界面：终端网格、侧栏、设置、启动器与文档视图全部由它渲染。旧的 winit 界面仍保留在代码中，用 `--legacy-shell` 启动。
- **侧栏 AI 会话分叉** — 右键 claude 或 codex 标签会出现「分叉 AI 会话」，用 CLI 自己的 fork 语法开一个新标签接续同一段对话。资格在菜单打开当下重算，刚上报 session id 的会话立刻就能分叉。
- **AI 回合状态识别** — 侧栏标签会显示 claude 与 codex 的运行中、已完成、等待输入与失败四种状态，依据来自 CLI 自身的 hook 载荷加屏幕状态证据。看门狗会释放僵尸「运行中」状态，转圈不会一直转下去。
- **新界面的完整设置页** — 旧界面的所有设置项与分区导航全部迁移，设置以一个可复用的标签打开，不再是浮动窗口。
- **标签重命名、颜色标记与单标签导出** — 可以就地重命名标签、用七种品牌色之一标记，或只把这一个标签导出为工作区文件。
- **分屏、拖拽分屏与工作区会话恢复** — 把标签拖到窗格边缘即分屏，分屏树与每个窗格的启动身份会在下次启动时恢复。
- **AI 服务商设置与运行时控制 CLI** — 服务商凭据与模型可在设置中配置，运行时 API 让脚本可以开标签、执行命令与查询状态。
- **默认 GPUI 壳的真实 Agent 控制闭环** — 默认界面把正在运行的 workspace 发布到 CLI wait/subscribe 共用的 RuntimeHub。`nebula ctl agents` 返回权威的 Agent 身份、生命周期状态与证据来源；`nebula ctl read` 从真实终端 Grid/scrollback 尾部读取且不移动用户视口；focus、prompt、wait 始终操作精确的 window/pane 组合。SSH pane 在 Shell Ready 前拒绝读写；GPUI 尚不可用的多窗口创建明确报错，不再伪造成功。
- **Git worktree 隔离 Agent** — `nebula ctl agent-fork` 在 Runtime 工作线程创建新分支与 worktree，在该 checkout 中打开真实 Tab/PTY，并启动带稳定 generation 的命名 Codex/Claude。Agent 记录持续携带 branch、base commit 与路径，后续 `agent-get`/prompt/read/wait 都能核验同一工位。dirty source、既存分支/路径与 SSH source 明确报错；UI 明确启动失败只回滚本请求创建的资源，UI 超时状态不确定时保留 checkout，不让晚到 Pane 的 cwd 突然失效。
- **随包 Nebula Runtime Skill 与协议合同** — ZIP 和安装器同时携带版本化 Runtime API 文档、Schema 与 `nebula-runtime` Skill，固化“列出 → 观察 → 读取 → 聚焦 → 派活 → 等待”流程，并把终端输出按不可信数据处理。
- **内置制表符几何绘制** — 边框、方块与分隔线在渲染契约中按几何绘制，不再依赖字体字形，换任何字体都能对齐。
- **终端鼠标语义对齐** — 应用级鼠标上报、右键行为与选中即复制均与旧壳一致。
- **Markdown 与图片阅读器** — 文档带富文本、原生公式与图片渲染，PNG/JPEG/WebP/BMP 在独立的查看标签中打开。

- **两种补全样式可切换** — “设置 → 终端 → 补全”新增「补全样式」：在既有的行内灰字与新的弹窗候选列表之间选择。弹窗汇总历史命令、常用目录、PATH 命令与文件系统匹配（至多 8 项，带来源标签），↑/↓ 选行、按补全接受键（Tab/→）接受、Esc 关闭且同一行不再重弹；命令面板提供切换动作，选择以 `completion_style` 持久化。
- **自定义背景色改用真调色盘** — 背景色浮层顶部新增饱和度/明度取色面与色相条，按住拖动即连续取色、终端实时预览、松手落盘。预设色板与 16 进制输入保留，三种输入互相同步（灰/黑/白取色时保留既有色相）。
- **剪贴板截图粘贴（本地与 SSH）** — 剪贴板只有位图没有文本时（如 Win+Shift+S 之后），粘贴会把位图转成 PNG 并粘出文件路径：本地 pane 写入临时文件；SSH pane 经既有 SFTP 栈后台上传到远端 `/tmp/nebula-paste-<时间戳>.png` 再把远端路径敲进会话——codex/claude 这类接受图片路径的 CLI 在两侧都能直接用。
- **可配置新标签页插入位置** — “设置 → 交互”可选择把新建标签页插在当前标签页之后，或放到标签列表末尾。选择会持久化；恢复会话中的标签页仍保持原有顺序语义。
- **可配置终端单元格宽度** — “设置 → 外观”新增“紧凑”和“宽松”两种单元格宽度模式。切换后会立即重算字体度量、终端网格、分屏布局和 PTY 尺寸，并在下次启动时保留选择。
- **有序字体 fallback 链** — `font_family` 支持逗号分隔的字体族，例如 `JetBrains Mono, 霞鹜文楷, Cascadia Code`。第一个字体族仍是主字体，后续字体族按顺序为常规、粗体、斜体和粗斜体补齐缺失字形，内置 Maple 字体始终作为最后兜底。系统字体与导入/私有字体现在统一通过同一族名查找路径完成校验、预览和渲染。（#33）
- **中键关闭标签页** — 在标签页或其关闭控件上单击鼠标中键，会进入既有的关闭流程；需要确认时仍会正常弹出确认。
- **终端铃声** — BEL（`\a`）现在在视觉铃声之外播放系统提示音：AI CLI 在别的标签页里完成回合也听得见。内置节流，刷铃声的程序不会连成机关枪；`bell.audible = false` 可关闭。（#37）
- **AI 对话跨重启接续** — 关闭（或崩溃）时某个 pane 里还开着 claude/codex 对话的，冷恢复后会自动把 resume 命令（`claude --resume <id>` / `codex resume <id>`）敲进恢复出来的 shell。会话身份来自 CLI 自己的 hook 载荷；识别到 claude 但没有 id 时退化为在恢复目录里 `claude --continue`。注入只针对恢复出的裸 shell（绝不注入 SSH/Profile 首格），id 上屏前先做字符集校验；“设置 → 高级”提供「恢复时自动接续 AI 对话」开关（`resume_ai`，默认开）。
- **托盘图标 agent 提醒** — 常驻系统托盘图标：任一 AI CLI 停下来等输入时翻转为橙点 attention 态，窗口全被压住或最小化也看得见。右键列出所有 agent pane 及状态（运行中/等待输入），点击经 toast 同一条聚焦路径直达来源 pane；左键直达最需要人的那个。托盘与侧栏徽章同一事实源；“设置 → 高级”可关（`tray`，默认开）。
- **纯文本文件带行号打开（GPUI 壳）** — 文件树双击 `.txt` / `.log` / `.json` / `.jsonl` 现在进代码查看器：行号 + 按可视行虚拟化（与源码文件同款），不再进 markdown 文档视图；Markdown 仍走富文本阅读器。
- **GPUI 壳可点击文件与 URL** — OSC 8 超链接和匹配到的 URL 用格子自身前景色画虚线下划线。悬停显示预览（`解码后的路径 · Ctrl+点击`）；Ctrl+点击用资源管理器打开本地 `file://`，其余 URI 走默认打开方式。
- **GPUI 壳可配置终端铃声** — “设置 → 配置文件 → 终端铃声”可选关 / 闪烁 / 声音 / 闪烁 + 声音。BEL 仍播放节流后的系统提示音、短暂闪一下 pane、窗口失焦时出 toast、后台 tab 打点。选择以 `bell` 持久化（`none` / `visual` / `audible` / `both`，默认两者都开）。
- **GPUI 壳所见即所得字体选择器与启动目录** — 字体下拉用各族自己的字形渲染，展示名剥掉多余扩展名，导入按钮改为「导入字体」。启动目录改为选文件夹，「继承当前目录」/「清除」，与旧壳一致。

#### 修复

- **资源管理器右键打开并入驻留实例** — 「在 Nebula 中打开」此前被当作显式意图而总是启动独立窗口，驻留进程里 detached 的标签看起来就“丢了”。带目录、无 `-e` 命令的启动现在会先 ATTACH 既有实例（找回原有标签），再在该窗口新开定目录标签并置前；没有驻留实例时仍独立启动。runtime API 的 `tab.new` 为此新增可选 `cwd` 参数。
- **导入的终端在默认 Shell 下拉中可以选中** — 下拉的命中测试此前只统计检测到的 shell，而列表还渲染了导入的快速启动配置，末尾几行看得见点不中。绘制与命中现在共用同一计数。
- **Git 面板在他人所有的仓库上不再空白** — 抽屉里的 `git status`/`diff` 以单次调用范围的 `safe.directory` 豁免运行，修复 `\\wsl$\…` 根目录与提升权限检出下“自己 shell 里 git status 正常、面板却空白”的问题；失败现在会留下调试日志而不是无声空白。
- **Windows 可见窗口可从卡死的渲染门控中恢复** — 启动期的遮挡误报或帧回调丢失不再让可见窗口陷入“能接收输入但不重绘”。窗口没有真正最小化时会忽略遮挡误报，既有的 1 Hz 窗口心跳也会以幂等方式释放卡住的 `occluded` 与 `has_frame` 门控；看门狗现在在建窗时即刻武装，首帧之前的冻结也能在一秒内自愈。（#21、#32）
- **盘符根目录右键启动可用** — 在 `D:\` 这类盘符根目录背景右键「在 Nebula 中打开」此前报「无效的工作目录」：资源管理器把 `%V` 展开成 `D:\`，末尾反斜杠在命令行上转义了收尾引号。被吃掉的路径现在会被修复；错误提示里的日志变量改用 PowerShell 语法（`$env:NEBULA_LOG`），复制即可用。（#36）
- **多行粘贴不再打断 codex** — 多行粘贴确认现在只在换行真的会被 shell 逐行执行时弹出。处于 bracketed paste 模式的应用（codex、vim、新版 PSReadLine）会把粘贴当作一个整体接收，警告不再拦路。确认框里深色主题下 accent 主按钮上的「Enter」键帽也改从按钮自身墨色派生，不再是一块近黑色。（#35）
- **公式覆盖层兼容被 markdown 反转义的定界符（GPUI 壳）** — 部分 AI CLI 先渲染 markdown 再上屏，`\[ \]` / `\( \)` 的反斜杠被当作 markdown 标点转义吃掉，屏幕上只剩裸 `[ … ]` 块与 `(\sqrt{…})`，公式从不渲染。共享扫描器现在识别这些裸形态：内容须携带已知 TeX 命令（`\int`、`\frac`……）且方括号独占行首/行尾；JSON 数组、`[INFO]` 日志、正则 `(\d+)` 保持原样。GPUI 壳里 TeX 显式长度（`\\[6pt]`、`\kern`）的 pt→px 换算也已对齐旧壳。
- **Markdown 阅读器铺满阅读列（GPUI 壳）** — 段落内的软换行（README 手工折行的正文）此前渲染成硬换行，右侧留出一大片参差空白。现按 CommonMark 合并为空格，中日韩相邻行直接相连不插空格。
- **Markdown 图片真正能加载（GPUI 壳）** — 文档 tab 现在把相对图片路径按文档所在目录解析、绝对路径直接读盘、`http(s)` 图源经后台 ureq 客户端拉取（gpui 默认装的是空客户端），README 的 logo、shields 徽章（含 SVG）与截图终于都能显示。本地和网络的 GIF / 动画 WebP 会先压成单帧 PNG：gpui 在多帧图的帧下标被静态图复用时会直接 panic。
- **Markdown 里的原生 HTML 按 GitHub 语义渲染（GPUI 壳）** — README 常用的 HTML 写法现在与 GitHub 渲染一致：`<p>` / `<div>` / 标题上的 `align="center"` 让整块居中；同一段落里连续的 `<img>` 徽章在同一行居中横排（此前一枚一行竖着摞）；行内 `<br/>` 按文档顺序断行（此前会被提到所跟文本之前）；`<a><img/></a>` 链接徽章不再整个丢失——图片正常显示且带链接。
- **Codex `notify` 不再自我包装成起不来的配置** — 另一个 notify 包装器（codex-computer-use）重新注册时若认不出 Nebula 的 helper，会把整个旧数组序列化进自己的 `--previous-notify` 参数；Nebula 再包一层，转义反斜杠每轮翻倍，最终把 `config.toml` 撑到 130 MB，Codex 直接起不来。现在只要 helper 标记出现在**任何位置**（含 JSON 字符串内部）就算已接线，Nebula 只自愈自己的路径、绝不再包。序列化后超过 8 KB 的 `notify` 一律拒写，兜住其他形态的膨胀；`nebula setup-ai --remove` 会持久化 `ai_hooks=0`，重启后也不再自动装回。（#38）
- **Tab 右键菜单阴影不再随标签数变厚（GPUI 壳）** — 组件库的右键菜单扩展用固定元素 id 存状态，于是每个标签行都解析到同一份状态、把同一个已打开的菜单重复画在同一锚点上。菜单面板不透明，阴影不是：8 个标签就叠 8 层投影。现在 Tab 菜单与文件树菜单一样，由 workspace 根只画一次。
- **窗口缩放后终端光标不再漂移** — conhost 在 resize 后会以非确定的方式塌缩屏幕缓冲区且零重绘，光标可能停在离 shell 认知好几行的位置。Nebula 现在在每次 resize 后通过 AttachConsole 探针对账真实光标位置并重新对齐网格。
- **默认 Shell 会加载用户 PowerShell 配置文件** — 默认启动此前跳过了 `$PROFILE`，其中定义的提示符、别名与 PSReadLine 设置从未生效。（#30）
- **先缩小再放大后 reflow 不再丢内容** — 窗口变窄再变宽不会丢掉折行内容，窗口也有了真正的最小尺寸。
- **光标焦点与闪烁，以及启动期渲染门控冻结** — 光标状态机正确跟随焦点，看门狗会释放可能让窗口可见却不重绘的启动门控。（#21）
- **新界面里 Esc 能送到 Claude Code** — win32-input 编码器把控制键的 `uChar` 硬填成 0，而 OpenConsole 会丢弃这种事件，读字节流的 CLI 因此收不到 Esc。
- **关闭窗口确认会把忙碌进程算进去** — 构建或 AI CLI 正在运行时关闭窗口，会指名是哪个程序而不是直接关掉。
- **通知贴到右下角。**
- **背景模糊按配置的不透明度合成。**
- **Shell 选择器品牌图标不再发虚** — 图标按整数物理像素预缩放，不再由 GPU 拉伸。
- **“宽松”宽度模式下内置字形填满单元格。**
- **标签启动身份与整行命中区** — 恢复出的标签保留它启动时的程序，整行都可点击而不只是标签文字。

#### 改进

- **文件/Git 侧栏刷新不再阻塞渲染** — 目录遍历与 Git 状态子进程改为在工作线程中生成快照，旧内容在刷新期间仍可正常使用。完成后的快照会整体替换；根目录已变化的过期结果会被丢弃，正在显示的搜索结果也不会被覆盖。
- **GPUI 壳 Tab 折叠箭头与文件树菜单** — TABS 折叠控件改用壳自带的线性 Chevron，不再用 Nerd Font 字形。文件树右键菜单不再叠上抽屉阴影，观感与 Tab 菜单一致。
- **字体选择器用候选字体自己的字形预览** — 每个字体族在确认前就以自己的字面呈现。

## 1.0.0 - 2026-08-10

### English

#### Updated

- **Updated network settings** — the page now has three clear choices: No proxy, Follow system, and Use proxy. The network test is at the top of the page, and changing the choice or address takes effect immediately without restarting Nebula.
- **Updated proxy routing** — Follow system reads the operating system proxy only; terminal variables such as `ALL_PROXY`, `HTTP_PROXY`, and `HTTPS_PROXY` no longer change SSH or SFTP routes. The old per-host proxy field is removed from `ssh_profiles.json`, while OpenSSH `ProxyJump` remains supported.

#### Fixed

- **Fixed inline formulas split by terminal wrapping** — a formula such as `$e^{i\\pi}+1=0$` still renders when the terminal wraps it onto the next screen row, while a real line break still ends the inline formula.
- **Fixed the network page showing an obsolete direct-host row** — all three modes now avoid displaying the unused bypass-host input.

#### Improved

- **Improved settings color feedback** — selected and hovered controls now use one softly tinted version of the active theme color in both light and dark themes, avoiding abrupt green-to-blue transitions.
- **Improved saved-host rows** — host cards use the same pale theme-tinted surface as the selected navigation item, so settings sections read as one consistent interface.

#### Added

- **Encrypted backup and restore** — Settings can export selected Nebula-owned data into an AES-256-GCM archive protected by an Argon2id-derived passphrase, then authenticate and restore it with path-traversal and symlink-parent checks. Appearance, configuration, sanitized SSH profiles, sync, assistant data, sessions, directory and command history, and imported fonts can be selected independently.
- **A dedicated image viewer tab** — double-clicking PNG, JPEG, WebP, or BMP files in the file tree opens an independent image tab that scales the decoded image to the content area. Markdown images reuse the same decoder and renderer instead of maintaining a second image path.
- **A dedicated SSH settings page** — the sidebar's ordered SSH hosts now have a two-line card view with connect, edit, hide, add-host, and immediate `~/.ssh/config` import actions under Settings → SSH. The global network mode and proxy address live on their own Network page; the existing sidebar remains a fast connection entry point.
- **Shared overlay scrollbars** — Markdown, tab lists, and SSH hosts now use the same unobtrusive scrollbar: a 3 px thumb appears only for overflowing hovered content, with a forgiving 12 px pointer target plus drag and track-click navigation.
- **A unified Files/Git drawer header** — Files and Git are two centered slots in one segmented control. The file tools row keeps the current root on one line and provides follow-current-terminal (`Alt+R`), new-terminal-here (`Alt+T`), and reveal-in-file-manager (`Alt+O`) actions with matching hover tips.
- **Input latency probes** — launching with `NEBULA_INPUT_LATENCY=1` logs per-segment timings (key event → PTY write, PTY wakeup → frame, key → frame) through the debug log, turning "typing feels slow" into a number that names the slow segment. Disabled probes cost one initialized-`OnceLock` read.
- **SSH connections can go through a jump host or the system proxy** — `ProxyJump` from `~/.ssh/config` remains supported, and SSH/SFTP connections use the shared global network setting. The system mode reads the Windows system proxy instead of terminal environment variables; multi-hop chains are rejected with an explanatory error.
- **The font picker lists installed system fonts** — the font selector merges Windows system font families with imported ones: monospaced families (as reported by DirectWrite, not guessed from glyph widths) are shown by default, the full list is one click away, and a real search box — caret, selection, and clipboard shortcuts included — filters by display name. Enumeration is lazy and stays off the startup path. Applying a font is transactional: validated first, rolled back on failure; proportional families are marked, and a startup fallback shows a notice. (Thanks to @Sakyvo.)
- **A compact appearance preset** — Settings → Appearance gains an interface density choice: Standard keeps the current look, while Compact trims padding, steps every corner radius one rung down the existing ladder, and forgoes decorative glows across the title bar, sidebar, settings, command palette, file drawer, and dialogs. No new visual values are introduced — compact takes the next smaller step of the existing spacing and radius ladders, expressed as relations (`radius::overlay(density)`, `control::row(density)`) rather than a parallel constant table. (Thanks to @Sakyvo.)

#### Fixed

- **Codex can distinguish Shift+Enter from Enter on Windows** — Nebula now enables ConPTY's Win32 input mode when creating a pseudo console, tracks the application's DECSET 9001 request, and emits the Win32 key records expected by console applications. Codex can therefore insert a newline with Shift+Enter instead of receiving an ordinary Enter submission.
- **Esc reaches Claude Code and other byte-stream readers under Win32 input mode** — the Win32 key records emitted for control keys carried `UnicodeChar=0`, while a real keyboard reports Esc=27, Enter=13, Tab=9 and Backspace=8; the bundled OpenConsole drops an Esc record without its character, so programs that consume the translated byte stream (node/Ink, hence Claude Code) never saw Esc at all, while VK-based readers (Codex) were unaffected. Control keys now carry their true `KEY_EVENT_RECORD` character (the OS text with modifiers applied when available, e.g. Ctrl+Enter→LF), and `scripts/win32_input_matrix.ps1` replays the key matrix against a recorded baseline to keep it that way. Root-cause walkthrough in `docs/hard_lessons.md`.
- **ConPTY sessions receive window focus reports** — focus in/out (`CSI I`/`CSI O`) was only sent when an application subscribed via DECSET 1004, so ConPTY could not synthesize the `FOCUS_EVENT_RECORD`s Win32 console programs read. Following the ConPTY keyboard spec, focus reports are now also sent whenever Win32 input mode is active; the host consumes them itself and still forwards the VT form only to 1004 subscribers, verified for both zero leakage and correct delivery.
- **Held modifiers no longer dangle across Alt+Tab** — switching windows mid-chord delivers the modifier's real key-up to the newly focused window, so any protocol stream that had reported the key-down (Win32 records or extended keyboard events) left the application believing the modifier was held forever; worst case, the first plain keystroke after returning was read as a Ctrl-chord and could kill a running task. Nebula now synthesizes protocol-correct key-ups for every held modifier at the moment focus is lost.
- **Live window drags no longer flood ConPTY with resizes** — every queued intermediate size used to reach `ResizePseudoConsole`, and the console host performs a full viewport reflow per call. The PTY event loop now applies only the newest size per channel drain: intermediate sizes are superseded, the final size always lands, and the slower the host reflows the harder the coalescing works.
- **Nebula starts even when the in-box ConPTY rejects Win32 input mode** — `CreatePseudoConsole` was called unconditionally with the Win32-input flag and a failure hit a process-killing assert. The call now retries without the flag and returns a real error instead of dying; a flagless host never requests DECSET 9001, so the input stack self-gates down to legacy VT end to end.
- **A crashed console host no longer leaves a zombie tab** — when the PTY transport died without the shell exiting (host crash, pipe or poller failure), the I/O thread quit silently and the tab kept accepting input into the void. The failure now surfaces its reason on the message bar, lands in the debug log, and runs the normal session teardown.
- **Occasional stalls while typing Chinese in PowerShell** — the renderer pushed the IME caret rectangle to the input method every frame, cursor blink and output scrolling included, and on Windows each push is a chain of synchronous cross-process IMM32 calls into the input-method host; whenever that host was busy, the render thread waited on it. The rectangle is now deduplicated and only pushed when it actually changes, and the cache is invalidated on focus changes, IME enabling, and IME re-association so the first composition after returning never lands at a stale position.
- **Missing AI hook warnings no longer repeat every frame** — the notice is emitted only when hook availability changes into the missing state, preventing an unavailable optional hook from producing an unbounded warning loop.
- **Installer version metadata matches the application** — the Inno Setup fallback version and numeric file version now match Nebula 0.9.0; release builds continue to inject the package version automatically.
- **A specified private key no longer triggers the system passphrase dialog** — the russh dependency had dropped its default `rsa` feature while switching to the ring backend, so every RSA private key — including the classic PKCS#1 `.pem` downloaded from cloud consoles — failed to parse locally; the failure was misclassified as "the key has a passphrase", popped the Windows credential dialog, and finally reported "the server rejected the key" although the server never saw it. The `rsa` feature is restored, a parse failure is reported as the local problem it is (only genuinely encrypted keys enter the passphrase flow), and the footer connection test never prompts — it reports "key is passphrase-protected" instead, honouring its no-interaction contract.
- **The terminal card's corners no longer show a pale fringe** — the rounded card and the shell's concave corner patches were two independently anti-aliased arcs blended in sequence, which mathematically leaks a sliver of the clear color along the arc (and the desktop behind the window once transparency is on). The corner patches now render underneath the card inside the backdrop pass, making the card's own arc the only visible seam, and the clear fallback uses the composited shell tone shared with the chrome strips.

#### Improved

- **The launcher palette now follows the grouped Shell/SSH design** — the three filters are reduced to All, SSH, and Shell; quiet chips remain text-only until hover or selection, while the selected state gets a restrained pill and hairline ring. Chip geometry uses real UI-font metrics so multi-column labels and double-digit counts stay inside the pill. Recommended shells, all shells, and SSH hosts use smaller section captions, full-width hairlines, equal-height rows, 28 px neutral icon tiles, stable panel height, a softer outer radius, and shared search/filter/list geometry. Opening it also dims the surrounding workspace so the active surface is unambiguous.
- **Settings navigation is compact, icon-led, and quieter** — the rail now matches the 196 px reference shell with 32 px rows, 2 px gaps, dedicated vector icons, wider internal label spacing, and a neutral low-contrast selected state. The backup surface has also been reorganized around an automatic-backup summary, an export/restore segmented action, and a grouped manifest with descriptions and sizes; its sidebar entry remains hidden until restore preview, conflict handling, and rollback are ready.
- **The Windows installer can register Nebula as a command** — "Add Nebula Terminal to the user PATH" is selected by default, `nebula.exe` is also registered through Windows App Paths for Win+R, and uninstall removes only the PATH entry that the installer added. Existing Explorer directory and directory-background context-menu registrations remain included in the installer.
- **File-tree status is quieter and stable** — directories use neutral theme colors, ignored paths detected through batched `git check-ignore` are dimmed without changing sorting or filtering, and hovering a row no longer shifts its contents.
- **Saved hosts in Settings → SSH show their OS icon** — each row now draws the same per-host icon as the sidebar (real ink width measured, then optically scaled to one target size), replacing the earlier neutral status ring; `auto` and unrecognized ids fall back to the generic terminal shape. The icon says which machine this is — it does not invent an online state.
- **Overlay panels draw from one component set** — the command palette, the Ctrl+K launcher, the Ctrl+Shift+O session picker, and combobox dropdowns now share one `overlay_list` module for option rows, icon tiles, identity chips, footer hints, and the query caret, and the SSH settings row actions share one outline-button widget. Selection and hover treatments are now identical everywhere; the migration also removed a hidden asymmetry where selected pills were 2 px taller than hover pills.
- **The Network page shows only what the selected mode needs** — the page keeps the mode selector, a manual proxy address when needed, and the network test. Obsolete local proxy scans, per-host overrides, and direct-host input are no longer presented; saved SSH hosts use the global network setting while OpenSSH `ProxyJump` remains a backend capability.
- **One selection tint everywhere** — the settings navigation, the sidebar's active tab, and the Network page's choice rows now share the design prototype's accent-soft selection wash (≈ rgb(52,71,99) composited on the dark theme); light themes stay neutral automatically because the token derives from each theme's accent.
- **The key-bindings page gains groups, search, and clash warnings** — actions are grouped under Global / Tabs / Panes / Side panels / Terminal with quiet section headers only (no frames), a search box filters actions and keys as you type and folds empty groups away, duplicate bindings paint both keycaps in the danger tint plus a warning naming which action stops firing, keycaps follow the prototype's tiering (surface base, thick bottom lip, dim ink lifted on hover), and hovering a row reveals the Rebind affordance.

### 简体中文

#### 更新

- **更新网络设置** — 网络页现在只保留三个直观选项：“不使用代理”“跟随系统”“使用代理”。“测试网络”放在页面最上方，切换选项或修改地址会立即生效，不需要重启 Nebula。
- **更新代理读取方式** — “跟随系统”只读取操作系统代理，不再受 `ALL_PROXY`、`HTTP_PROXY`、`HTTPS_PROXY` 等终端环境变量影响。`ssh_profiles.json` 中旧的主机代理字段已移除，但 OpenSSH 的 `ProxyJump` 仍然支持。

#### 修复

- **修复行内公式被终端换行拆开后不显示** — `$e^{i\\pi}+1=0$` 即使跨到下一屏幕行也会正常渲染；真正的换行仍会结束行内公式。
- **修复网络页残留“直连目标主机”输入行** — 三种模式都不再显示这条已经不用的设置。

#### 改进

- **改进设置页的颜色反馈** — 选中和悬停使用当前主题的同一套主题色，浅色与深色模式都保持柔和、干净的过渡，不再出现绿色跳成蓝色的突兀变化。
- **改进设置页主机行** — 已保存主机使用淡色主题底，和导航选中状态保持一致，减少不同区域之间的颜色跳变。

#### 新增

- **加密备份与恢复** — 设置页可将选中的 Nebula 数据导出为 AES-256-GCM 加密归档，密钥由 Argon2id 从口令派生；恢复前会完整认证，并防止路径穿越和符号链接父目录写入。外观、配置、已脱敏的 SSH 配置、同步、助手数据、会话、目录与命令历史以及导入字体均可独立选择。
- **独立图片查看器标签页** — 在文件树中双击 PNG、JPEG、WebP 或 BMP 会打开独立图片标签页，并按内容区域等比适配。Markdown 图片复用同一套解码与渲染路径，不再维护重复实现。
- **独立 SSH 设置页** — 侧栏同源、保持原顺序的 SSH 主机现在以双行卡片呈现，并提供连接、编辑、隐藏、添加主机和立即导入 `~/.ssh/config`；全局网络模式和代理地址集中在“设置 → 网络”。侧栏仍保留为快速连接入口。
- **共享 OverlayScrollbar** — Markdown、标签列表和 SSH 主机列表统一使用克制的浮层滚动条：内容溢出且鼠标进入时才显示 3px thumb，实际命中区为 12px，并支持拖动和点击轨道跳转。
- **文件/Git 一体化抽屉头部** — 文件与 Git 成为同一分段控件内居中的两个槽位；文件工具行单行显示当前根目录，并提供跟随当前终端（`Alt+R`）、在此新建终端（`Alt+T`）和在资源管理器中打开（`Alt+O`），hover 提示与快捷键保持一致。
- **输入延迟打点** — 以 `NEBULA_INPUT_LATENCY=1` 启动后，调试日志会记录分段耗时（按键事件→PTY 写入、PTY 唤醒→帧、按键→帧），把“打字感觉慢”变成能指出慢在哪一段的数字。探针关闭时的开销仅为一次已初始化 `OnceLock` 读取。
- **SSH 可经跳板机或系统代理连接** — `~/.ssh/config` 的 `ProxyJump` 仍然支持，SSH/SFTP 连接统一使用网络页的全局设置；“跟随系统”读取 Windows 系统代理，不读取终端环境变量。多级跳板链会得到明确报错。
- **字体选择器列出系统已安装字体** — 字体选择器把 Windows 系统字体族与导入字体合并去重：默认只显示等宽家族（由 DirectWrite 权威判定，不用字形宽度启发式），一键展开全部；弹层顶部是正经搜索框（光标、选区、剪贴板快捷键俱全），按显示名过滤。枚举惰性执行，不进启动路径。应用字体是事务性的：先验证再持久化，失败回滚到当前字体；非等宽家族有标记，启动回退时也会给出提示。（感谢 @Sakyvo。）
- **紧凑外观预设** — 设置 → 外观新增界面密度选项：标准保持现状；紧凑减少留白、把所有圆角在既有阶梯上整体下移一档，并去除顶栏、侧栏、设置、命令面板、文件抽屉与对话框的装饰性光晕。不引入任何新的视觉数值——紧凑取的是既有间距与圆角阶梯的更小一档，以关系函数（`radius::overlay(density)`、`control::row(density)` 等）表达，而非平行常量表。（感谢 @Sakyvo。）

#### 修复

- **Windows 下 Codex 现在能区分 Shift+Enter 与 Enter** — Nebula 创建伪控制台时启用 ConPTY Win32 输入模式，跟踪应用发出的 DECSET 9001 请求，并发送 Windows 控制台程序所需的 Win32 按键记录。因此 Codex 中 Shift+Enter 会插入换行，不再被当作普通 Enter 提交。
- **Win32 输入模式下 Esc 能到达 Claude Code 等字节流读取方** — 控制键的 Win32 按键记录此前带着 `UnicodeChar=0`，而真实键盘上 Esc=27、Enter=13、Tab=9、Backspace=8；随附的 OpenConsole 会丢弃不带字符的 Esc 记录，导致按翻译后字节流消费输入的程序（node/Ink，即 Claude Code）完全收不到 Esc，按虚拟键码识别的程序（Codex）则不受影响。现在控制键携带真实 `KEY_EVENT_RECORD` 字符值（可用时取操作系统带修饰的文本，如 Ctrl+Enter→LF），并新增 `scripts/win32_input_matrix.ps1` 按基线回放键位矩阵防止回归。完整根因见 `docs/hard_lessons.md`。
- **ConPTY 会话现在能收到窗口焦点报告** — 焦点进出（`CSI I`/`CSI O`）此前只在应用通过 DECSET 1004 订阅时发送，ConPTY 因此无法合成 Windows 控制台程序读取的 `FOCUS_EVENT_RECORD`。按 ConPTY 键盘规范，Win32 输入模式激活时也发送焦点报告；宿主自行消费并仅向 1004 订阅者透传 VT 形式，零泄漏与正确送达均已验证。
- **修饰键不再因 Alt+Tab 悬挂** — 按住修饰键切窗时，真实的 key-up 发给了新聚焦的窗口，导致所有上报过 key-down 的协议流（Win32 记录或扩展键盘事件）让应用永远认为修饰键仍被按住；最坏情况下，切回后按下的第一个普通键会被解读为 Ctrl 组合并可能误杀正在运行的任务。Nebula 现在在失焦瞬间为所有按住的修饰键合成符合协议的 key-up。
- **拖动窗口不再向 ConPTY 倾泻 resize** — 此前队列中每个中间尺寸都会到达 `ResizePseudoConsole`，而控制台宿主每次调用都做整个视口的 reflow。PTY 事件循环现在每轮排空只应用最新尺寸：中间尺寸被后来者取代、最终尺寸必达，且宿主 reflow 越慢合并力度越大。
- **内置 ConPTY 拒绝 Win32 输入模式时 Nebula 仍能启动** — `CreatePseudoConsole` 此前无条件携带 Win32 输入标志，失败即触发进程级 assert。现在会去掉标志重试并返回真实错误而非直接崩溃；未带标志创建的宿主不会请求 DECSET 9001，输入栈端到端自动降级到传统 VT。
- **控制台宿主崩溃不再留下僵尸标签页** — PTY 传输在 shell 未退出时死亡（宿主崩溃、管道或轮询器故障）此前会让 I/O 线程静默退出，标签页继续把输入吞进黑洞。现在故障原因会显示在消息栏、写入调试日志，并走正常的会话收尾流程。
- **PowerShell 中文输入偶发卡顿** — 渲染层此前每帧都把 IME 光标矩形推给输入法（光标闪烁、输出滚动都算帧），而 Windows 上每次推送是一串同步跨进程的 IMM32 调用，直达输入法宿主进程；宿主一忙，渲染线程就跟着等。现在推送前做值去重，矩形没变就不再打扰输入法；焦点切换、输入法启用与重新关联时作废缓存强制重推，回焦后的第一次组词不会落在陈旧位置。
- **AI hook 缺失警告不再逐帧重复** — 只有 hook 可用性切换到“缺失”状态时才提示一次，避免可选 hook 不存在时产生无上限的警告循环。
- **安装器版本信息与应用一致** — Inno Setup 的兜底版本和数字文件版本改为 Nebula 0.9.0；正式构建仍会自动注入包版本。
- **终端卡四角不再泛白边** — 圆角卡片与壳层凹角补片是两条各自抗锯齿的弧按先后顺序混合，弧线上必然漏出一丝清屏色（开启透明度后直接漏出窗口后面的桌面）。凹角补片改为在 backdrop 里画在卡片**之下**，卡片自己的圆弧成为唯一可见接缝；清屏兜底色也换成与 chrome 条带同源的壳层合成色。
- **指定私钥不再触发系统口令弹窗** — russh 依赖在切换 ring 后端时把默认的 `rsa` feature 一并关掉了，导致所有 RSA 私钥（包括云厂商控制台下载的经典 PKCS#1 `.pem`）本地解析失败；这个失败被误判成「密钥有口令」，弹出 Windows 凭据对话框，最终还报成「服务器拒绝了指定的私钥」——其实服务器根本没见过这把钥匙。现在补回 `rsa` feature，解析失败按本地问题如实报错（只有真正加密的密钥才进入口令流程），页脚的「测试连接」也绝不弹框——改为报告「私钥受口令保护」，兑现无人值守承诺。

#### 改进

- **启动器面板改为 Shell / SSH 分组设计** — 筛选项收敛为“全部、SSH、Shell”；未选中 chip 保持纯文字，悬停或选中才出现克制的胶囊底，选中态带很弱的发丝描边。chip 宽度按 UI 字体真实度量计算，双列文字和两位数计数都不会越出圆角底。推荐 Shell、所有 Shell 与 SSH 主机使用更小的分组标题、贯穿内容宽度的细分隔线、等高条目、28px 中性图标底块、更柔和的面板圆角和固定高度；搜索、筛选、列表继续共用同一套几何。打开面板时同时压暗周围工作区，当前操作层级更明确。
- **设置导航更紧凑、图标化且更安静** — 左侧栏对齐 196px 原型，使用 32px 条目、2px 间隔、独立矢量图标、更宽松的内部文字间距，以及低对比度的中性选中态。备份页也重排为自动备份摘要、导出/恢复分段操作和带描述/大小的分组清单；在恢复预览、冲突策略与回滚流程完成前，左侧入口暂时隐藏。
- **Windows 安装器可以把 Nebula 注册为命令** — “将 Nebula Terminal 添加到当前用户 PATH”默认勾选，同时通过 Windows App Paths 注册 `nebula.exe`，因此 Win+R 可以直接启动；卸载时只移除安装器自己加入的 PATH 条目。资源管理器目录及目录背景的右键菜单注册仍随安装包提供。
- **文件树状态更安静且不抖动** — 目录使用主题中性灰，批量 `git check-ignore` 识别出的忽略项只降低显示强度、不参与排序或过滤，鼠标悬浮也不再移动行内容。
- **设置 → SSH 的已保存主机显示各自的系统图标** — 每行左缘画与侧栏同源的 per-host 图标（按真实墨迹宽度光学缩放到统一尺寸），替代此前的中性状态环；`auto` 与未识别的 id 回落通用终端形状。图标只回答“这是哪台机器”，不发明在线状态。
- **浮层面板共用一套组件** — 命令面板、Ctrl+K 启动器、Ctrl+Shift+O 会话快跳与下拉框的选项行、图标底块、身份 chip、页脚提示和查询光标统一收进 `overlay_list` 组件，SSH 设置页的行内动作也共用一个 outline 按钮组件。各处的选中/悬停形态从此完全一致——迁移顺带消除了一个隐藏不对称（选中药丸此前比悬停药丸高 2px）。
- **网络页按所选模式显示内容** — 页面保留模式选择、需要时的代理地址和网络测试；旧的本机代理扫描、每主机覆盖和“直连目标主机”输入不再显示。已保存主机统一使用网络页的全局设置，OpenSSH `ProxyJump` 仍由后端支持。
- **选中色全局统一为一个 token** — 设置导航、侧栏活动标签与网络页单选行共用原型的 accent-soft 选中底（深色主题合成后 ≈ rgb(52,71,99)）；浅色主题的这个 token 本身就是中性灰，无需另设分支。
- **按键映射页新增分组、搜索与冲突提示** — 动作按 全局 / 标签页 / 窗格 / 侧栏面板 / 终端 分组，只用段标题与间距分隔（无框保持干净）；搜索框即输入即过滤动作名与按键，被滤空的组连标题一起收起；重复绑定会把两行键帽标成 danger 色，并配一条写明「谁不生效」的警告；键帽按原型分级（surface 底、厚底边、弱墨 hover 提亮）；悬停行浮现「改键」入口。

## 0.9.0 - 2026-08-02

### English

#### Added

- **Idle tabs show their shell** — a quiet tab row (no badge, not hovered) shows its shell as a small dim tag at the right edge: `pwsh`, `cmd`, `bash`, `ubuntu` (WSL distributions by name), and `ssh` for SSH tabs. Any badge (spinner, dot, attention…) outranks it, and the tag steps aside when a long name reaches the right edge.
- **Section headers count what is in them** — the sidebar's TABS and SSH HOSTS headers carry a small count chip, and the disclosure chevron moved to the front of the title so several sections line their chevrons into one column. The count stays visible while a section is collapsed: it is the only information left there, and "42 hosts" versus "3 hosts" is what decides whether you scroll or search.
- **WSL distributions in the folder picker** — "Browse folder" dialogs now pin every registered WSL distribution (`\\wsl.localhost\<distro>`) to the top of their sidebar, so picking a Linux directory no longer depends on the system's "Linux" navigation node being present. (#12)
- **Drag to resize the panels** — Settings → Interaction gains "Drag to resize panel widths". With it on, the sidebar's right edge and the file/git drawer's left edge become drag handles (the pointer turns into a resize cursor over the ±4 px grip), and dragging either panel all the way to its window edge closes it rather than stopping at a minimum width. Off by default, and turning it on asks for confirmation first, because a width drag reflows the terminal live. The drag itself is double-throttled — the panel edge tracks the pointer every frame, but the grid is only re-laid when the width has crossed a whole cell and at most once every 80 ms, so a fast drag cannot turn into a reflow storm. The SSH HOSTS divider is always draggable regardless of the switch — it only redistributes height inside the sidebar and never touches the grid, and the height you drag it to is honoured even when you have fewer hosts than fit. All three sizes persist.
- **Restore last tabs on launch is now a setting** — Settings → Advanced → Sessions gains "Restore last tabs on launch (also after a crash)", on by default. Turning it off always starts clean; the snapshot keeps being written either way, so workspace export and crash diagnostics still work.
- **Window background blur (Windows 11)** — Settings → Appearance gains "Background blur". With it on, whatever is behind the window is blurred through the terminal background (Acrylic), and the window-opacity slider becomes the strength of the tint laid over that blur, so one control still governs "how much shows through". Mica was tried and dropped: it samples the desktop wallpaper rather than the actual window behind, which reads as a flat colour wash instead of a blur. Note that a translucent background forces text anti-aliasing from subpixel to grayscale — that is a system-level constraint of transparent windows, not a rendering regression.
- **The command palette is grouped** — rows now sit under quiet section headers with a hairline rule running to the panel edge: Working directory, Tabs, Jump, View, Workspace, Appearance, Settings, and the shell/profile list. The **Working directory** group carries the focused pane's path on the header itself — written once instead of repeated on every row — and holds "Copy path" and "Reveal in file manager"; "New tab" joins it only when a new tab would actually open there (a configured startup directory takes precedence, and then the row stays under Tabs rather than sitting under a heading that lies). Toggle commands ("Show tab sidebar", "Drag to resize panels") carry a check mark for their current state, so a switch that is already on reads differently from an action that would turn it on. Grouping is applied after ranking, so a group's internal order is still most-recently-used first (or best fuzzy match while typing), and the headers track the scrolled window rather than the top of the list.
- **SSH connection progress** — connecting to a saved host now shows the four real stages (resolve, TCP, authenticate, open session) instead of a blank pane. The stages are call sites in Nebula's own SSH implementation rather than guesses parsed from a client's output, so the progress is truthful. The page waits 350 ms before appearing: connections that finish faster than that never flash a progress screen, and hovering a host in the sidebar shows the state inline instead of taking over the pane.

#### Fixed

- **A crash is now told apart from a clean exit** — the session snapshot records whether the process reached its teardown. After an unclean exit (crash, force-kill, power loss) the restored window says so in a transient success toast instead of restoring silently; and when the crash-loop breaker trips after three failed launches, the offending session is moved to `session.crashed.json` instead of being overwritten by the next autosave a second later — previously the only evidence of a restore-crash loop destroyed itself.
- **Recovery notices use the right layer** — a completed recovery is a short success toast, while a crash-loop breaker notice remains in the message bar with the quarantined session path available for follow-up.
- **The message-bar close button stays visible with CJK text** — wide characters are measured in terminal columns and the close control is drawn as a reusable UI widget, so long multilingual notices no longer push the button off-screen; its plate and hover state make the control visibly clickable.
- **Sharp text and icons in the SSH host editor** — the identity strip's name, the avatar shape and the icon list previously scaled up bitmaps rasterized at the terminal font size, visibly blurry next to the design; they now re-rasterize at their true size. Caret placement in the enlarged name field follows the true glyph advance, so clicking the tenth character no longer lands the caret a column off.
- **The icon picker no longer shows form text through its panel** — overlay backgrounds were submitted in the same batch as the form's fills, which all paint beneath text; overlays (the icon list, the test-status tooltip) now paint after the form's text, so nothing bleeds through.
- **Test-connection failures show the whole reason** — the status line used to flatten the error into one truncated line with the full text hidden behind a hover tooltip. Failures now wrap up to four lines right in the dialog; the tooltip remains only for over-long tails.
- **Spinner rings render as smooth rings** — the beaded look came from translucent dots doubling their alpha wherever they overlap, and the active tab's ring composited against the sidebar background instead of the light pill it actually sits on, which read as a dark circle. Ring colors now pre-compose against the row's real background.
- **The "needs your answer" badge shows up for codex** — codex's notify pipe only reports "turn complete", so interactive question prompts could only ever show the unread dot. On turn completion Nebula now checks the visible tail of the screen for prompt markers ("enter to submit", "(y/N)"…) and upgrades the badge to the attention marker.

#### Improved

- **The icon picker's search box is a real input field** — click to place the caret, drag to select, Shift+arrows / Home / End, Ctrl+A/C/V, with the same caret-blink rhythm as every other field; the hint still shows while it is empty.
- **New SSH hosts default to password authentication** — "Auto" needs a configured key to succeed, which made the old default a guaranteed first failure for most people. Existing hosts keep whatever they saved.
- **The raised-hand badge is withdrawn for now** — even with wider fingers and tighter gaps the shape did not read as a hand at badge size. "Needs your answer" is an amber dot until the icon is redrawn; it still outranks the blue unread dot, and the two are told apart by colour rather than by shape.
- **Sidebar rows breathe wider** — the row inset narrowed from 8 px to 4 px, giving the width to names, badges and shell tags; the sidebar's own width is unchanged.
- **Every text field shares one caret** — caret position, selection, click-to-place and the blink rhythm moved out of the individual dialogs and into the input component itself. Previously each field carried its own copy, which is why the SSH editor's enlarged name field and the icon search box each drifted from the others in small ways; they now behave identically, and new fields inherit the behaviour instead of re-implementing it.

### 简体中文

#### 新增

- **静默标签页显示 shell 类型** — 没有任何徽章、未悬浮的标签行，右侧以最淡的小字显示该标签的 shell 短标：`pwsh`、`cmd`、`bash`、`ubuntu`（WSL 按发行版名），SSH 标签显示 `ssh`。任何徽章（转圈、圆点、「等你批准」…）都比它优先；名字长到顶着右缘时短标自动让位。
- **分组标题显示数量** — 侧栏的 TABS 与 SSH HOSTS 标题带一颗数量 chip，折叠箭头移到标题前面，多个分组的箭头因此对齐成一条竖线。折叠之后数量仍然常驻：那时它是这一段仅剩的信息量，而「42 台」和「3 台」直接决定人是滚动找还是直接搜。
- **文件夹选择器可直达 WSL** — 「浏览文件夹」对话框把每个已注册的 WSL 发行版（`\\wsl.localhost\<发行版>`）钉进侧栏顶部，不再依赖系统资源管理器是否显示「Linux」节点。（#12）
- **侧栏可拖拽调节** — 设置→交互新增「拖拽调节侧栏宽度」。开启后左侧栏右缘与文件/Git 抽屉左缘成为拖拽把手（±4px 热区上光标变为调整宽度形态）；把左侧栏一路拖到最左、或把抽屉一路拖到最右，就直接收起该面板，而不是卡在最小宽度上。默认关闭，且开启前会先确认——宽度拖动会实时重排终端内容。拖动本身走双重节流：面板边缘每帧跟手，但网格只在宽度跨过一整个单元格时才重排，且最快 80ms 一次，快速拖动不会演变成 resize 风暴。SSH HOSTS 分界线不受该开关约束、始终可拖：它只在侧栏内部重新分配高度，碰不到终端网格；主机数量撑不满时，拖出来的高度也照样生效。三处尺寸都会保存。
- **启动恢复会话可开关** — 设置→高级→会话新增「启动时恢复上次的标签（异常退出后同样恢复）」，默认开启。关掉就永远干净启动；快照照写不误，工作区导出与崩溃诊断仍然可用。
- **窗口背景模糊（Windows 11）** — 设置→外观新增「背景模糊」。开启后窗口背后的内容透过终端底色被模糊（Acrylic），窗口不透明度滑杆随之变成盖在模糊层上的着色强度——「透出多少」仍然只由一个控件管。Mica 试过后放弃：它取的是桌面壁纸而不是窗口背后的真实内容，看起来是一层平涂色调而非模糊。另外，半透明背景会把文字抗锯齿从次像素降为灰度，这是透明窗口的系统级约束，不是渲染退化。
- **命令面板分组了** — 行归到安静的分组标题下，标题右侧一条发丝线拉到面板右缘：工作目录、标签页、跳转、视图、工作区、外观、设置，以及 shell / profile 列表。**工作目录**组把聚焦终端的路径挂在标题上——写一次，而不是每行重复一遍——组里是「复制路径」和「在资源管理器中显示」；「新建标签页」只在它确实会开在那个目录时才并入（配了启动目录时启动目录优先，那时这行留在标签页组，而不是挂在一个说谎的标题下）。开关类命令（「显示标签侧栏」「拖拽调节侧栏宽度」）带勾选态，已经开着的开关与「点了会开」的动作因此读起来不一样。分组是在排序**之后**做的稳定划分，组内仍然是最近用过的优先（打字时是模糊得分优先）；表头跟着滚动窗口走，而不是钉在列表开头。
- **SSH 连接进度** — 连接已保存的主机时不再是一片空白，而是显示四个真实阶段（解析、TCP、认证、建立会话）。这四个阶段是 Nebula 自己的 SSH 实现里的调用点，不是从某个客户端的输出里猜出来的，所以进度是可信的。连接页有 350ms 门槛：比这更快连上的连接不会闪一下进度屏；在侧栏悬浮某台主机时状态就地显示，不接管整个终端面板。

#### 修复

- **崩溃与正常退出现在分得清了** — 会话快照记录进程有没有走完收尾。上次是异常退出（崩溃、强杀、断电）时，恢复后会用自动消失的 success toast 明说，而不是悄悄恢复；连续三次启动失败触发断路器时，那份会话被挪到 `session.crashed.json` 而不是被一秒后的自动保存盖掉——此前「一恢复就崩」的唯一现场会自己销毁。
- **恢复提示分到正确的层** — 已完成的恢复成功提示走短暂的 success toast；断路器提示仍留在消息栏，并保留隔离会话路径供后续处理。
- **中文消息栏的关闭按钮始终可见** — 按终端显示列宽处理宽字符，关闭控件改为可复用 UI 组件自绘，中文长消息不会再把按钮挤出屏幕；常态底板与悬停反馈让它明显可点击。
- **SSH 主机编辑器的大字与图标不再发糊** — 身份条的名字、头像形状与图标列表此前是把按终端字号栅格化的位图硬放大，对着原型一眼可见的糊；现在按真实字号重新栅格化。放大后名字框的光标换算按真实字形步进，点第十个字不再偏一格。
- **图标选择器不再透出表单文字** — 浮层的底与表单同批提交时全部沉在文字之下；现在浮层（图标列表、测试状态提示）在表单文字之后单独提交，什么都透不上来。
- **「测试连接」失败显示完整原因** — 此前被压成单行截断，全文藏在悬浮层里；现在失败原因直接折行铺开（最多四行），只有超长的尾巴才收进悬浮层。
- **转圈指示器是平滑的圆环** — 珠链感来自半透明圆点在重叠处 alpha 翻倍；活动标签上的环还拿侧栏深底做合成，画在浅色药丸上就成了一圈黑。环的颜色现在按所在行的真实底色预合成。
- **codex 的「等你回答」徽章能亮了** — codex 的 notify 只报「回合完成」，交互式提问也只能落成未读圆点。现在回合结束时检查屏幕尾部的提问特征（"enter to submit"、"(y/N)"…），命中即把徽章升级为「等你批准」。

#### 改进

- **图标搜索框是一个正经输入框** — 点击定位光标、拖选、Shift+方向键 / Home / End、Ctrl+A/C/V，光标闪烁节律与其它输入框一致；空着时仍显示提示语。
- **新建 SSH 主机默认密码认证** — 「自动」要先配好私钥才走得通，拿它当默认等于让多数新手先撞一次失败。已保存的主机保持原样。
- **手掌徽章暂时下线** — 指再粗、缝再窄，徽章尺寸下还是读不成一只手。「等你批准」改用琥珀色圆点，等图标重画后再回来；它依然压过蓝色未读点，两者靠颜色而不是形状区分。
- **侧栏行更宽** — 行的左右内缩从 8px 收到 4px，宽度让给名字、徽章和 shell 短标；侧栏总宽不变。
- **所有输入框共用同一套光标** — 光标位置、选区、点击定位与闪烁节律从各个对话框里下沉到输入组件本身。此前每个输入框各存一份，SSH 编辑器放大后的名字框、图标搜索框于是各自在细节上跑偏；现在行为完全一致，新加的输入框直接继承，不必再实现一遍。

## 0.8.0 - 2026-07-29

### English

#### Added

- **Shell picker shortcut (Ctrl+K)** — opens the same shell/profile list the "+" chevron does, and closes it on a second press. The binding appears in Settings → Key bindings and can be rebound; by default it takes over the shell's `kill-line` (Ctrl+K → `\x0b`).
- **SSH connection testing** — the SSH host editor can now test its unsaved destination, password, authentication mode, and private keys before saving, with a 12-second timeout and inline connecting, success-time, or failure status.
- **Configurable quick-terminal shortcut** — Settings → Key bindings now exposes the global quick-terminal shortcut. Captured changes are applied immediately and remembered; if the operating system rejects a conflicting shortcut, Nebula keeps the previous working binding and shows the registration failure in the row.
- **Workspace export/import** — the command palette gains "Export workspace…" and "Open workspace…", and a tab's right-click menu gains "Export as workspace…". A workspace file (`.nebula-workspace.json`) records the tab list, each tab's full split layout (axis, ratio, per-pane working directory), tab names and colors, and each tab's launch identity: WSL/custom shells reopen with their shell, SSH tabs reconnect to their saved destination automatically. Files are portable across machines and platforms — a directory that does not exist on the importing machine falls back to the default one, and a shell program the machine lacks (e.g. `wsl.exe` on Linux) falls back to the default shell in the saved directory instead of dropping the tab.
- **Crash recovery now restores split layouts** — the continuously written session snapshot (1 Hz) uses the same schema as workspace files, so after a crash or force-kill Nebula reopens with every tab's split tree, ratios, per-pane directories and SSH/WSL launch identities — not just one pane per tab as before.
- **Grok is recognised as a running program** — a tab running `grok` or `grok-cli` shows the official xAI mark in its icon slot, the same way `claude` and `codex` already do. The light or dark mark is picked to match the chrome ink, and it is downsampled to the exact physical pixel size of the slot and optically centred on its ink, so it stays sharp at every DPI. (Thanks to @Sakyvo.)
- **Tab reveal motion is configurable** — Settings → Interaction gains a "Tab reveal" choice. `Slide` (default, unchanged behaviour) eases a tab into place when it becomes visible; `Instant` puts it there immediately. Drag-to-reorder keeps its spring in both modes. (Thanks to @Sakyvo.)

#### Fixed

- Nebula now always starts at its standard size — the configured `window.dimensions`, or the default 116×30 grid **priced at the config base font size**. Two regressions could previously blow the first window up to near-fullscreen: the launch replayed the saved window size from the session file (stale or wrong-unit values included), and the new font-size persistence fed the Ctrl+wheel zoom into the startup sizing formula, so 116 columns of an enlarged cell filled the screen. Startup now ignores both; a persisted zoom still renders after launch, it just shows fewer columns in the standard-sized window.
- Fixed the sidebar and other interface text zooming together with Ctrl+wheel, plus the hairline seams that appeared once the terminal font left its base size. Root cause was architectural: chrome shares the terminal's single font system, and several chrome paths (sidebar captions, tab/host labels, settings headings, the SSH editor, the message-queue entry) drew through the document-text path that follows the terminal zoom **by design**. All chrome typography now goes through a dedicated UI-anchored text path that rasterizes at the UI base size with its own baseline math, layout steps by the base font's actually-rasterized cell metrics, click targets read the same layout the pixels were drawn with, and the anchor state is correct from the very first frame of a restored session.
- Terminal font zoom is now clamped to a sane range (logical 4–64 px). Previously a stuck modifier key or trackpad burst could scroll the size past 180 px, where each wheel notch changes the size by under 1 % and zooming back out felt broken.
- Fixed the resize HUD ("80 × 24") drawing its label outside the centered box whenever the sidebar was open: the box was centered in window pixels while the text was centered on the terminal grid, whose origin carries the sidebar's asymmetric padding. Both now share one coordinate system, and the HUD no longer inflates with the terminal zoom it reports.
- Fixed the directory tree not following tab switches: an open SFTP panel no longer captures the drawer forever — the view routes by the focused pane (local tabs get the directory tree back, the SFTP connection stays warm), switching to a remote pane no longer blanks the tree, and WSL tabs launched as `wsl -d <distro>` now follow the shell's directory through `\\wsl$`.
- Fixed math rendering in WSL and remote SSH sessions: `$$ … $$` and `\[ … \]` display formulas printed by claude, codex, pi and similar tools running behind `wsl.exe` or `ssh.exe` now render natively — block detection no longer depends on spotting the AI CLI in the local process tree, which cannot see through WSL or SSH.
- Display blocks now accept physics-style implicit products such as `E = mc^2`; inline `$…$` keeps the stricter shape checks so shell text like `$foo^bar$` stays literal.
- The first rendered display formula in a pane now unlocks inline `$…$` rendering there, so remote AI sessions get inline math without local process detection.

#### Improved

- Reworked the SSH host editor into a compact content-sized dialog with consistent 32 px controls, helper text below the destination, a segmented authentication selector, deliberate label/control and section spacing, a lightweight private-key section, visibly blinking text carets, and clearly separated Test / Cancel / Save actions.
- Reworked the Shell / Profile picker into cards: content-sized height with a maximum ten-row viewport, a "Recommended" section carrying a taller hero card (name above, full program path below, Enter chip on the right) and an "All options" section, neutral initial selection, soft accent selection, a bordered default-shell badge, and a keyboard-hint footer. Paths keep their drive/root context and ellipsize at the tail. Hover and click hit-test against the layout's per-row rectangles, so section captions and the gaps between rows no longer highlight as if they were rows.
- Picker rows are no longer drawn as bordered cards. A row is transparent until you hover or select it, because the panel-to-row lightness difference already separates them — five ringed white rectangles in a column read as a form, not as a list. Selection uses a neutral wash rather than the accent: the accent budget is spent once per screen, on the current row.
- All option paths now start at one shared column, so they line up in a single vertical edge. Right-aligning them made every row's path *start* float with its own length (`cmd.exe` began nearly 300 px right of Nushell's), so scanning the list meant hunting left and right for each line.
- One keycap recipe everywhere — every place that shows a key combination (the picker's Ctrl+K badge, the command list's shortcut hints, Settings → Key bindings, confirm dialogs) draws the same chip: hairline ring, panel fill, chip radius. A combination is **one** chip carrying the whole string, matching the familiar Windows convention; the per-key split it replaces is a macOS form, where `⌘ ⇧ ⌥` are single-character glyphs that tile evenly — `Ctrl` `Shift` `Alt` do not.
- Chip backgrounds now mean "you can click this". The picker footer's ↑↓ / Enter / Esc hints are keyboard legends, not buttons, so they dropped their chips and separate the key from its description by ink weight instead.
- Text carets are visibly alive. The blink phase now starts from your last edit instead of running off the wall clock, so a caret is lit the instant a field takes focus (previously it had a 50 % chance of appearing up to half a second late, which read as "this field isn't focused"). The caret holds steady while you type and only breathes once you stop, and the cadence follows the system's `GetCaretBlinkTime`, including the accessibility setting that turns blinking off. The command palette was also missing from the fast-redraw list, so its caret froze once the open animation ended.
- The command palette's caret is a 1.5 px beam instead of a full-cell glyph, which lets the placeholder and your actual query start at the same x — typing the first character no longer shifts the line sideways.
- Confirm dialogs and the SSH host editor now cast a real drop shadow and share the app's corner radius. Both previously drew only a hairline and a fill, so a dialog that demands an answer floated lower than the command palette you can dismiss with Esc. The SSH editor's dim was also a hard-coded 67 % black that ignored the theme, veiling light themes more heavily than dark ones.
- Dropdown lists are fully opaque and cast a shadow instead of a glow. A glow spreads brightness outward without establishing height, which on light themes just grayed the area around the list.
- Light-theme overlays are white-based: the command palette, the SSH host editor and the settings surfaces no longer inherit a silver/slate fill from the theme family.
- The palette's right edge now lines up. The Ctrl+K badge, the hero card's Enter chip, the command list's shortcut chips and the footer's Esc hint previously measured from four different insets — the footer even measured from the panel instead of the card column — leaving a ragged right margin that drifted with font size.
- Aligned Settings with the new navigation-and-row system: equal-height hairline groups, consistent right-aligned controls, restrained section headings, theme previews, and editable keycaps share one layout and interaction model.

### 简体中文

#### 新增

- **Shell 选择器快捷键（Ctrl+K）** — 打开与 "+" 旁 chevron 相同的 shell/profile 列表，再按一次收起。该键位在“设置 → 按键映射”里列出、可改绑；默认占用了 shell 行编辑的 kill-line（Ctrl+K → `\x0b`）。
- **SSH 连接测试** — SSH 主机编辑器现在可以在保存前测试未保存的地址、密码、认证方式和私钥，带 12 秒超时，并在页脚显示连接中、成功耗时或失败状态。
- **快速终端快捷键可配置** — “设置 → 按键映射”新增快速终端全局快捷键行。捕获的新组合立即应用并持久化；若操作系统因冲突拒绝注册，Nebula 会保留原先可用的快捷键，并在该行显示注册失败。
- **工作区导出/导入** — 命令面板新增"导出工作区…"和"打开工作区…",标签页右键菜单新增"导出为工作区…"。工作区文件(`.nebula-workspace.json`)记录标签页列表、每个标签页的完整分屏布局(方向、比例、每个 pane 的工作目录)、标签名与颜色,以及每个标签页的启动身份:WSL/自定义 shell 按原 shell 重开,SSH 标签页自动重连保存的目标。文件跨机器、跨平台可移植——导入机器上不存在的目录回落默认目录;缺失的 shell 程序(如 Linux 上的 `wsl.exe`)回落为默认 shell 并保留目录,不会丢掉整个标签页。
- **崩溃恢复现在还原分屏布局** — 持续写入的会话快照(每秒)与工作区文件共用同一格式,崩溃或强杀后重开时,每个标签页的分屏树、比例、各 pane 目录以及 SSH/WSL 启动身份都会还原,不再是以前的"每个标签页只剩一个 pane"。
- **识别运行中的 Grok** — 跑 `grok` 或 `grok-cli` 的标签页会在图标位显示 xAI 官方标记,和现有的 `claude`、`codex` 一样。浅色/深色两版按 chrome 墨色自动选,并且是按图标槽的**物理像素尺寸**重采样、再按墨迹重心居中的,因此在任何 DPI 下都不发虚。(感谢 @Sakyvo。)
- **标签展开动效可配置** — 设置 → 交互新增"标签展开"选项。`滑动`(默认,与原行为一致)让标签页在变可见时缓动到位,`立即`则直接落位。两种模式下拖拽换序的弹簧手感都不变。(感谢 @Sakyvo。)

#### 修复

- Nebula 现在始终以标准尺寸启动——配置的 `window.dimensions`,或默认的 116×30 网格,且**按配置基准字号计算**。此前有两处回归会把首窗口撑到接近全屏:启动会重放会话文件里保存的窗口尺寸(包括过期或单位错误的值);新增的字号持久化又把 Ctrl+滚轮缩放喂进了启动尺寸公式,116 列放大后的 cell 本身就是一个全屏宽度。启动现在对两者都免疫;持久化的缩放字号启动后照常渲染,只是在标准尺寸的窗口里显示更少的列数。
- 修复界面文字随 Ctrl+滚轮一起缩放、以及终端字号离开基准后出现的发丝级脏线。根因是架构性的:chrome 与终端共用同一套字体系统,而侧栏标题、tab/主机标签、设置页大标题、SSH 编辑浮层、消息队列条目等多处 chrome 文字走的是**设计上就跟随终端缩放**的文档文字路径。现在全部 chrome 排版统一走专用的 UI 锚定文字路径(按 UI 基准字号栅格化、独立的基线数学),布局按基准字号真实栅格化的 cell 步进,点击目标与绘制读取同一布局,恢复持久化缩放的第一帧锚定即正确。
- 终端字号缩放现在有硬边界(逻辑 4–64 px)。此前卡住的修饰键或触控板会把字号滚过 180px,那里每档滚轮变化不足 1%,缩回去的手感如同失灵。
- 修复调整窗口大小的 HUD("80 × 24")在侧栏打开时文字画到居中框外:框按窗口像素居中,文字却按终端网格居中,而网格原点带着侧栏的非对称边距。两者现在共用同一坐标系,HUD 也不再随它所汇报的终端缩放一起变大。
- 修复目录树不跟随标签页切换:打开过的 SFTP 面板不再永久霸占抽屉——视图按聚焦 pane 路由(切回本地标签页恢复目录树,SFTP 连接保持热连接),切到远程 pane 不再把树清空;`wsl -d <发行版>` 启动的 WSL 标签页现在通过 `\\wsl$` 跟随 shell 目录。
- 修复 WSL 与远程 SSH 会话中的公式渲染:隔着 `wsl.exe` / `ssh.exe` 运行的 claude、codex、pi 等工具输出的 `$$ … $$`、`\[ … \]` 块级公式现在原生渲染——块级检测不再依赖本机进程树里能否找到 AI CLI(进程探测无法穿透 WSL 和 SSH)。
- 块级公式接受 `E = mc^2` 这类隐式乘积;行内 `$…$` 保持更严格的形状检查,`$foo^bar$` 之类的 shell 文本仍按字面显示。
- pane 内首个渲染成功的块级公式会解锁该 pane 的行内 `$…$` 渲染,远程 AI 会话无需本机进程探测即可获得行内公式。

#### 改进

- SSH 主机编辑器重排为紧凑的内容自适应弹窗：控件统一为 32px 高，地址提示移到输入框下方，认证方式改为分段选择，标签/控件与分组间距遵循统一节奏，私钥区降级为轻量小节，文本光标真实闪烁，并清晰分隔“测试连接 / 取消 / 保存”。
- Shell / Profile 选择器改为卡片式：按条目数量决定高度，最多显示十行；分为“推荐”区（一张更高的大卡片，名称在上、完整程序路径在下、右侧回车键帽）和“所有选项”区；初次打开保持中性，选中态使用柔和 accent，保留默认 Shell 发丝描边徽标和键盘提示页脚。路径保留盘符/根路径上下文、尾部省略。悬停与点击改为按布局的逐行矩形命中，分区标题和行之间的缝隙不再被当成行高亮。
- 选择器的行不再画成带框卡片。默认完全透明，只有悬停/选中才上底色——面板与行之间的明度差已经足够把它们分开，而五个带圈的白矩形排成一列读起来是表单，不是列表。选中态改用中性底色而不是 accent：强调色预算一屏只花一次，就花在“当前选中”上。
- 所有选项的路径改为从同一列起画，左缘对齐成一条竖线。此前是右对齐，每行路径的**起点**随它自身长度浮动（`cmd.exe` 的起点比 Nushell 靠右近 300px），眼睛沿列表往下扫时要不停地左右找落点。
- 键帽样式全局统一：所有展示键位的地方（选择器的 Ctrl+K 徽标、命令列表的快捷键提示、设置 → 按键映射、确认弹窗）画同一种 chip——发丝圈边、面板填充、统一圆角。一个组合键是**一颗**承载整串的 chip，符合 Windows 用户熟悉的输入习惯；被它替换掉的逐键拆分是 macOS 的形式，那里 `⌘ ⇧ ⌥` 是单字符图形、排起来宽度整齐，而 `Ctrl` `Shift` `Alt` 不是。
- chip 底色现在只表示“这里可以点”。选择器页脚的 ↑↓ / Enter / Esc 是键位说明而非按钮，因此去掉 chip 底，改用墨色深浅区分键名与释义。
- 文本光标有了活动感。闪烁相位改为从**最后一次编辑**起算，而不是跟着系统挂钟走：输入框一获得焦点光标立刻是亮的（此前有一半概率要等最多半秒才出现，读起来像“这个框没聚焦”）。打字期间光标保持常亮，停手后才开始呼吸，节律取自系统的 `GetCaretBlinkTime`，并尊重“关闭闪烁”这项无障碍设置。命令面板此前还漏在快速重绘名单外，入场动画一结束它的光标就不再翻转。
- 命令面板的光标改为 1.5px 细梁而不是占满一格的字形，于是提示文字与真实输入从同一个 x 起画——打下第一个字符时整行不再横向跳动。
- 确认弹窗与 SSH 主机编辑器现在有真正的外阴影，圆角也并入统一阶梯。此前两者只画发丝描边加填充，于是一个要求用户作答的弹窗，浮起高度还不如按 Esc 就能关掉的命令面板。SSH 编辑器的遮罩此前是写死的 67% 纯黑、不读主题，浅色主题下比深色主题罩得还重。
- 下拉列表改为完全不透明，并用阴影而不是辉光。辉光只是向外扩散亮度、不建立高度关系，在浅色主题下只会让列表四周发灰。
- 浅色主题的浮层统一白色打底：命令面板、SSH 主机编辑器与设置页表面不再从主题族继承银/岩灰底色。
- 命令面板右缘现在对齐成一条线。此前 Ctrl+K 徽标、大卡片的回车键帽、命令列表的快捷键 chip 与页脚的 Esc 提示各用一套内边距——页脚甚至是按面板而非卡片列计算的——右边缘参差，且会随字号漂移。
- 设置页统一到新的导航与等高行体系：hairline 分组、右对齐控件、克制的分节标题、主题预览和可编辑键帽共用一致的布局与交互模型。

## 0.7.0 - 2026-07-24

### English

#### Added

- **Background image** — Appearance settings can now set a wallpaper for the terminal, with stretch mode, position, and image opacity. The wallpaper stays inside the terminal area by default, and can optionally extend across the whole window.
- **Startup directory** — new terminal tabs can open in a directory of your choice, picked with the system folder dialog. The choice is remembered and can be cleared at any time.
- **Window size memory** — Nebula reopens with the same window size and maximized state as last time, and the size stays consistent across screens with different scaling.
- **Live appearance preview** — the Appearance page now starts with a preview card that immediately shows color, font, font size, and cursor changes as you make them.
- **Dropdown option lists** — every multi-choice setting (default shell, terminal font, wallpaper stretch and position, interface language, completion accept key, cursor shape) now opens a dropdown showing all options, instead of cycling through values on click.
- **Font size controls** — the terminal font size can be changed with a numeric control in settings or Ctrl + mouse wheel in the terminal, and is remembered. Interface text keeps its own size and is not affected by zooming.
- **Background color picker** — the background color row now opens a picker with preset colors and a hex color input, instead of cycling colors on click.
- **Cursor shape and blinking** — the default cursor shape (bar, underscore, filled box, hollow box) is selectable and cursor blinking is on by default. Programs like vim can still change the cursor themselves.
- **Copy on select** — a new Interaction section adds copy-on-select, on by default. With it off, right-click still copies the selection, or pastes when nothing is selected.
- **SSH keepalive** — remote sessions that sit idle for a long time no longer get disconnected by routers or firewalls.
- **Update check** — Nebula now checks GitHub Releases shortly after startup and shows a dismissible in-app banner when a newer version is available. It is a single anonymous version query; nothing else is sent.

#### Fixed

- Fixed wallpaper fading: lowering the image opacity now fades the picture into the theme background — lighter in light mode, darker in dark mode — instead of letting the desktop behind the window shine through as a harsh white.
- Fixed wallpaper formats: PNG, JPG, WebP, and BMP files chosen in the picker all display correctly now.
- Fixed the wallpaper covering the terminal's rounded corners, and made the appearance preview card show the actual wallpaper with its real stretch, position, and opacity.
- Fixed the opacity controls: both sliders now drag smoothly with live preview, and save when released.
- Fixed uneven window transparency: at any opacity the frame around the terminal now looks like one even surface, without patches that appear more solid than their neighbors.
- Fixed the maximize button not switching to the restore icon when the window is maximized, and hover highlights on the window buttons not reaching the top edge of the screen.
- Fixed background text showing through the command palette.
- Fixed math rendering while typing: formulas typed into Claude Code, Codex, and similar tools stay as editable text, and only finished output is displayed as math.
- Fixed rendered formulas reverting to raw source while scrolling, and long formulas losing their beginning after it scrolled out of view.
- Fixed matrices and multi-line formulas showing leftover text such as `&nbsp;`.
- Fixed the terminal and input boxes keeping colors from the previous theme after switching between light and dark.
- Fixed tab titles not following directory changes; tabs you renamed yourself keep their names.
- Fixed custom fonts causing hollow boxes and runs of broken symbols; icons and missing characters now fall back to the built-in font automatically.
- Fixed SSH warnings that could not be closed and spilled over the sidebar.
- Fixed the title-bar buttons: minimize, maximize, and close now form a seamless Windows-style group flush with the window edge, the close button shows the familiar red hover, and it stays clickable in the very corner when maximized.
- Fixed the sidebar "+" button and the three-dot menu changing size depending on window state.
- Fixed the working spinner pausing and jumping between rotations; it now spins smoothly.
- Fixed the opacity sliders showing a resize cursor and a gray hover box; they now look and feel like standard Windows sliders.
- Fixed icons sitting visibly off-center inside their hover highlights.
- Fixed command palette rows not highlighting under the mouse.
- Fixed the Ctrl+click link tooltip jumping around with the pointer and growing too long; it now stays in place and shortens long paths.
- Fixed cursor shape and blinking changes not taking effect until a restart.
- Fixed confirmation dialogs stretching into wide banners; long messages now wrap and the buttons are a simple Yes / No pair.
- Fixed harmless clipboard warnings that appeared when another program briefly held the clipboard.
- Fixed the cursor style chosen in Settings never applying when the shell had already touched cursor blinking (ConPTY does this on startup, so on Windows the setting effectively never worked): the choice now takes effect immediately, in every open tab and in new tabs.
- Fixed rendered formulas sitting on an opaque black (in light mode: white) slab that blocked the wallpaper and window transparency; formulas now draw directly over the real background.
- Fixed formula sizes depending on how many terminal rows the source text happened to occupy — a one-line `$$...$$` was squeezed tiny while multi-line sources rendered large. Formulas now borrow breathing room from surrounding blank lines and render at one consistent size, and tall inline fractions (`\dfrac`) become readable the same way.
- Fixed formulas flashing back to raw TeX while typing — including during Chinese IME composition — and sometimes never re-rendering after a TUI repaint; a formula whose closing `$$` is still visible now restores itself from scrollback.
- Fixed short inline formulas like `$\xi$` being centered inside the width of their source text, leaving them stranded between large gaps; narrow results now sit next to the preceding words.
- Fixed the background-color value in Settings overlapping the dropdown arrow.

#### Improved

- Improved long terminal sessions with many formulas: scrolling stays fast and memory use stays stable.
- Improved scrolling smoothness when a custom font is selected.
- Improved battery and CPU usage: animations only redraw while something is actually moving, and idle windows do almost no work.
- Improved consistency of settings controls: sliders, switches, dropdowns, and steppers now share one implementation, so they look and behave the same everywhere.

### 简体中文

#### 新增

- **背景图** — 外观设置现在可以为终端设置壁纸，支持拉伸方式、位置和图片不透明度调节。壁纸默认只显示在终端区域内，也可以选择铺满整个窗口。
- **启动目录** — 新建终端标签页可以在你指定的目录中打开，目录通过系统文件夹对话框选择，选择会被记住，也可以随时清除。
- **窗口大小记忆** — Nebula 会以上次关闭时的窗口大小和最大化状态重新打开，在不同缩放比例的屏幕之间大小也保持一致。
- **外观实时预览** — 外观页顶部新增预览卡片，颜色、字体、字号和光标的改动立刻能在预览中看到。
- **下拉选项列表** — 所有多选项设置（默认 Shell、终端字体、壁纸拉伸方式与位置、界面语言、补全接受键、光标形状）都改为打开下拉列表直接选择，不再是点一下换一个。
- **字号调节** — 终端字号可以在设置中用数字控件调整，也可以在终端里按住 Ctrl 滚动鼠标滚轮缩放，并会被记住。界面文字保持自己的大小，不受缩放影响。
- **背景色选择器** — 背景色一行改为打开选择器，提供预设颜色和十六进制色值输入，不再点击循环切换颜色。
- **光标形状与闪烁** — 默认光标形状可选（竖线、下划线、实心方块、空心方块），光标闪烁默认开启；vim 等程序仍然可以自己改变光标。
- **选中即复制** — 新增"交互"设置，提供选中即复制开关，默认开启；关闭后右键仍可复制选中内容，没有选中时则执行粘贴。
- **SSH 保活** — 长时间没有操作的远程会话不会再被路由器或防火墙断开。
- **检查更新** — Nebula 启动后会静默检查 GitHub Releases，有新版本时在应用内显示一条可关闭的横幅提示。整个过程只有一次匿名的版本查询，不上传任何数据。

#### 修复

- 修复壁纸变淡的方向：降低图片不透明度时，画面会淡入主题背景色——浅色模式越来越浅、深色模式越来越深，不再透出窗口后面的桌面形成刺眼的白色。
- 修复壁纸格式：选择器允许的 PNG、JPG、WebP 和 BMP 文件现在都能正常显示。
- 修复壁纸盖住终端圆角的问题，并让外观预览卡按真实的拉伸、位置和不透明度显示当前壁纸。
- 修复不透明度控件：两个滑块都可以流畅拖动、实时预览，松手后保存。
- 修复窗口透明度不均：任何透明度下，终端四周的边框看起来都是均匀的一整块，不再出现某一块比旁边更实的拼接感。
- 修复窗口最大化后按钮没有换成还原图标，以及窗口按钮的悬停高亮没有延伸到屏幕顶边的问题。
- 修复命令面板背后的文字透进面板形成重影的问题。
- 修复输入时的公式渲染：在 Claude Code、Codex 等工具里输入的公式保持为可编辑文字，只有已经输出完成的内容才会显示为数学公式。
- 修复滚动时已渲染的公式变回原始文字，以及长公式开头滚出屏幕后无法完整显示的问题。
- 修复矩阵和多行公式中出现 `&nbsp;` 之类残留文字的问题。
- 修复切换明暗主题后，终端和输入框残留上一个主题颜色的问题。
- 修复标签页标题不跟随目录变化的问题；你手动重命名过的标签页仍保持自定义名称。
- 修复自定义字体导致空心方框和成片乱码的问题；图标和缺失的字符会自动使用内置字体显示。
- 修复 SSH 警告无法关闭、背景延伸到侧边栏的问题。
- 修复标题栏按钮：最小化、最大化和关闭现在是连续无缝的 Windows 风格按钮组并完全贴齐窗口边缘，关闭按钮悬停显示熟悉的红色，最大化时屏幕最角落也能点到。
- 修复侧栏"+"按钮和三点菜单随窗口状态忽大忽小的问题。
- 修复工作状态转圈动画停顿、跳动的问题，现在旋转是连续平滑的。
- 修复不透明度滑块显示双向箭头光标和灰色悬停底块的问题，现在的外观和手感与标准 Windows 滑块一致。
- 修复图标在悬停高亮中明显偏离中心的问题。
- 修复命令面板候选行不跟随鼠标高亮的问题。
- 修复 Ctrl+点击 链接提示跟着指针乱跳、内容过长的问题，提示现在固定显示并会缩短过长的路径。
- 修复光标形状和闪烁设置需要重启才生效的问题。
- 修复确认对话框被拉成横幅的问题：长文字自动换行，按钮就是简单的"是 / 否"。
- 修复其他程序短暂占用剪贴板时弹出无意义警告的问题。
- 修复设置里选择的光标样式始终不生效的问题（Windows 上 shell 启动时会设置光标闪烁，旧逻辑会连形状一起"钉死"）：现在选择立即生效，对所有已打开和新建的标签页都有效。
- 修复公式渲染带一块不透明底色（深色模式黑块、浅色模式白块）、挡住壁纸和窗口透明效果的问题；公式现在直接绘制在真实背景上。
- 修复公式大小取决于源码占了几行终端的问题——单行 `$$...$$` 被压得极小、多行的又显得很大。公式现在会向上下空行借用空间，以统一大小渲染；行内的高分数（`\dfrac`）也因此变得可读。
- 修复输入时（包括中文输入法组词过程中）公式闪回原始文字、TUI 重绘后偶尔再也不渲染的问题；闭合 `$$` 仍在屏幕上的公式现在能从回滚缓冲区自动恢复。
- 修复 `$\xi$` 这类短公式在源码宽度内居中、两侧留出大片空隙的问题；渲染结果较窄时现在紧贴前面的文字。
- 修复设置中背景色的当前值文字与下拉箭头重叠的问题。

#### 改进

- 改进含大量公式的长时间终端会话：滚动保持流畅，内存占用保持稳定。
- 改进选择自定义字体后的滚动流畅度。
- 改进耗电和 CPU 占用：动画只在真正有东西变化时刷新，空闲窗口几乎不做工作。
- 改进设置控件的一致性：滑块、开关、下拉框和步进器共用同一套实现，各处外观和行为完全一致。
## 0.6.0 - 2026-07-19

### English

#### Added

- **Native math formula support** — Nebula can now display inline $...$ and display $$...$$ formulas directly in Markdown. Fractions, roots, scripts, limits, matrices, scalable brackets, Greek letters, common operators, and Unicode text are supported. Formulas are rendered locally in Rust with the bundled math font, without a web component, formula images, or an external TeX program.
- **Math formula test document** — the README now includes a verified screenshot, and `docs/math-rendering-test.md` provides a reusable test page covering common symbols, complex formulas, long formulas, blank rows, Unicode text, and dollar-fence boundaries.
- **Windows installer support** — Nebula now provides a guided per-user installer with English and Simplified Chinese interfaces, optional font installation, desktop and startup shortcuts, and structured cleanup during uninstall.
- **File-drawer directory actions** — the Files drawer can move to its parent directory, open a new terminal at the displayed directory, and drag a file or folder into the terminal to insert its safely quoted full path without executing it.
- **Frequent-directory workflows** — Nebula remembers directories that the shell actually entered. Frequently used locations are promoted in path completion and inline suggestions, and the command palette can open a new terminal directly in a visited directory.

#### Fixed

- **Markdown wrapping and formula containment fix** — paragraphs, Chinese text, long unbroken content, failed formula source, and oversized formulas now remain inside the reading column instead of overflowing or being cut off.
- **Multiline math and recognition boundary fix** — display formulas can contain blank rows and Unicode explanations, while only paired $...$ and $$...$$ ranges are treated as math. Bare TeX and quoted code remain ordinary Markdown.
- **Formula geometry and clarity fix** — arrows are converted only inside formulas, radical bars connect cleanly to the root symbol, Markdown headings use consistent weight, and small mathematical symbols have clearer edges.
- **Split-pane paste routing fix** — right-click paste and Ctrl+V always send text to the pane where the paste started, even after the pointer or focus moves across a split.
- **Enter penetration fix** — while multiline-paste confirmation is visible, Enter handles that confirmation only and never reaches the terminal behind it or a neighboring split. Approved text remains bound to the pane that opened the confirmation.
- **Split Markdown input penetration fix** — keyboard, pointer, and scrolling input used by a Markdown document no longer reaches a neighboring or background terminal in a split window.
- **Numpad Enter routing fix** — the numeric keypad Enter key now submits commands in the same way as the main Enter key instead of triggering paste behavior.
- **SFTP split-session routing fix** — opening the file panel from a split SSH terminal now uses that pane's authenticated destination, so the panel does not connect to a different host after titles, commands, or focus change.
- **Shell prompt lifecycle fix** — Nebula now preserves existing PowerShell and Bash prompt hooks, command exit status, pipeline status, and prompt behavior while still reporting directory changes and command completion.
- **Default-shell picker fix** — confirming a shell in the default-shell picker now saves it as the default instead of opening it as a new terminal.
- **System appearance following fix** — enabling automatic appearance now reads the operating system theme directly instead of reusing a stale manual window theme, so switching from a light theme follows an already-dark system immediately and continues tracking later changes.
- **AI integration removal fix** — removing integrations now continues through every supported tool even when one user configuration is damaged, avoiding stale hooks that point to an uninstalled Nebula executable.

#### Improved

- **Large Markdown math document improvements** — Markdown files containing many formulas load quickly and remain responsive while scrolling. Nebula processes the visible area, reuses repeated formulas, and limits unusually complex input so memory use stays stable during long reading sessions.
- **SFTP workflow improvements** — the SFTP panel now supports parent-directory navigation, drag-and-drop upload, and a background context menu for refresh, uploading files or folders, and creating a directory. Multi-file drops are grouped into one transfer instead of cancelling one another.

### 简体中文

#### 新增

- **Markdown 数学公式支持** — Nebula 现在能够直接显示行内 $...$ 和块级 $$...$$ 公式，支持分数、根式、上下标、极限、矩阵、伸缩括号、希腊字母、常用运算符和 Unicode 文字。公式由 Rust 和内置数学字体在本地完成显示，不需要网页组件、公式图片或外部 TeX 程序。
- **数学公式测试文档** — README 已加入经过验证的效果截图，`docs/math-rendering-test.md` 提供可重复使用的测试页面，覆盖常用符号、复杂公式、长公式、空行、Unicode 文字和美元围栏边界。
- **Windows 安装程序支持** — Nebula 现在提供中英文安装向导，支持按当前用户安装、可选字体安装、桌面与开机启动快捷方式，并在卸载时完成应用配置清理。
- **文件目录快捷操作支持** — 文件抽屉可以返回上级目录、在当前显示目录中新建终端，也可以把文件或目录拖入终端，插入经过安全引用的完整路径而不会自动执行。
- **常用目录支持** — Nebula 会记录 Shell 实际进入过的目录，让常用位置优先出现在路径补全和行内建议中；也可以从命令面板直接在访问过的目录中新建终端。

#### 修复

- **Markdown 换行与公式越界修复** — 普通段落、中文、连续长文本、解析失败的公式源码和过宽公式都会留在阅读列内，不再越界或从右侧被裁掉。
- **多行公式与识别边界修复** — 块级公式可以包含空行和 Unicode 说明文字，同时只有成对的 $...$ 和 $$...$$ 才会识别为数学公式；裸露 TeX 和引用代码仍按普通 Markdown 显示。
- **公式几何与清晰度修复** — 箭头只在公式内部转换，根号横线能够与根号主体完整连接，Markdown 标题字重保持统一，小字号数学符号的边缘也更加清楚。
- **分屏粘贴路由修复** — 右键粘贴和 Ctrl+V 始终把内容发送到发起粘贴的分屏，即使鼠标或焦点随后移动到其他分屏也不会改错目标。
- **Enter 穿透修复** — 多行粘贴确认框显示时，Enter 只处理当前确认，不会再发送到后方终端或相邻分屏；确认后的内容仍然只进入发起粘贴的分屏。
- **分屏 Markdown 输入穿透修复** — 在分屏窗口中查看 Markdown 文档时，文档使用的键盘、鼠标和滚动操作不再发送到相邻或后方终端。
- **数字小键盘 Enter 修复** — 小键盘最右侧 Enter 现在与主 Enter 一样提交命令，不再触发粘贴行为。
- **SFTP 分屏连接修复** — 从 SSH 分屏打开文件面板时，会使用该分屏已经认证的连接目标，不再因标题、命令或焦点变化连接到其他主机。
- **Shell 提示符生命周期修复** — Nebula 在报告目录变化和命令完成状态时，会保留已有的 PowerShell、Bash 提示符 Hook、命令退出状态和管道状态，不再破坏用户原有提示符工具。
- **默认 Shell 选择修复** — 在默认 Shell 选择器中确认后会正确保存设置，不再把所选 Shell 当成新终端直接打开。
- **系统明暗模式跟随修复** — 开启自动跟随后会直接读取操作系统主题，不再沿用窗口中残留的手动浅色状态；即使系统已经处于深色，也能立即切换并继续响应之后的明暗变化。
- **AI 集成移除修复** — 移除集成时，即使某一项用户配置损坏，Nebula 也会继续清理其他工具，避免残留指向已卸载程序的 Hook。

#### 改进

- **大型 Markdown 数学文档加载改进** — 包含大量公式的 Markdown 文档能够快速打开并保持流畅滚动。Nebula 只处理当前可见区域、复用重复公式，并限制异常复杂的输入，让长时间阅读时的内存占用保持稳定。
- **SFTP 操作改进** — SFTP 面板新增返回上级目录、拖放上传，以及包含刷新、上传文件、上传目录和新建目录的空白区域右键菜单；一次拖入多个文件时会合并为同一批传输，不会互相取消。

## 0.4.0 - 2026-07-14

### Terminal Rendering And Interaction / 终端渲染与交互

- **No more missing rows at the bottom** — the terminal now makes proper room for both the top bar and the bottom edge. The last prompt, cursor, selection, and full-screen terminal content stay inside the visible card, including in split views.
  **中文：** 终端底部不再凭空少一截啦。顶部栏和底部边距现在各占各的空间，即使使用分屏，最后一行命令、光标、选区和全屏程序内容也都能完整显示在卡片内。
- **Selection stays clean in transparent windows** — selected text no longer shows ghosted content from apps behind Nebula or leaves visual residue behind.
  **中文：** 透明窗口里的选区不再透出后方应用，也不会留下残影，选中文字时看起来更干净。
- **Softer cursor and selection colors** — the cursor and selection now follow the current theme with lower-saturation colors, so they feel more balanced and are no longer harsh on the eyes. A color chosen by the user still takes priority.
  **中文：** 光标和选区现在会跟随主题色啦，并使用低饱和度的颜色，看起来更协调、不再刺眼；如果用户自己设置了光标颜色，仍会优先使用用户的选择。
- **Links are easier to recognize** — clickable file paths and terminal links now keep a dashed underline. The underline follows the original text color, so folders, executables, and multicolored filenames remain easy to tell apart.
  **中文：** 可点击的文件路径和终端链接现在会一直带有虚线下划线，而且下划线会跟随文字原本的颜色，目录、可执行文件和彩色文件名依然一眼就能分清。
- **Mouse selection feels like other desktop apps** — double- and triple-click selection now follows the system's timing and movement rules, while `Shift`+click extends the current selection. A normal click will no longer unexpectedly select a whole word or line.
  **中文：** 鼠标选中文字现在更符合系统习惯：双击、三击会遵循系统的速度和移动范围，`Shift`+点击可以继续扩展选区，普通单击也不会再莫名选中整词或整行。
- **Multiline shortcuts no longer look like pasted text** — key bindings that send an `Esc`-prefixed sequence, such as `Shift`+`Enter` for multiline input in Claude Code, now go straight to the terminal instead of opening the multiline paste confirmation.
  **中文：** `Shift`+`Enter` 这类发送 `Esc` 组合序列的多行输入快捷键，现在会直接交给终端，不会再被误认为粘贴内容并弹出多行粘贴确认。
- **Text sizing looks normal again** — headings and copy in the sidebar, SSH view, and document view no longer appear stretched, crowded, or stuck together when display scaling is enabled.
  **中文：** 开启系统缩放后，侧栏、SSH 页面和文档里的标题与说明文字不再被异常放大，也不会显得拉长、拥挤或粘在一起。

### SSH Safety And Feedback / SSH 安全与反馈

- **Right-click menus for SSH hosts and tabs** — SSH hosts can be connected, copied, edited, or removed from a right-click menu. Tabs can be duplicated, split, renamed, closed, or given a custom color. The menu closes naturally when clicking elsewhere, pressing `Esc`, typing, or switching away from the window.
  **中文：** SSH 主机和标签页都补上了顺手的右键菜单。SSH 主机可以连接、复制地址、编辑或删除；标签页可以复制、左右/上下分屏、重命名、关闭或设置颜色。点击其他地方、按 `Esc`、继续输入或切走窗口时，菜单都会自然收起。
- **Deleted hosts can be recovered** — removing a host now asks for confirmation and provides an eight-second Undo button plus `Ctrl+Z`. Hosts read from `~/.ssh/config` are only hidden in Nebula, never deleted from that file, and hidden hosts can be brought back from Settings. Saved order is restored on Undo, and credentials are not erased until the Undo period ends.
  **中文：** 删除 SSH 主机前现在会先确认，删除后还有 8 秒撤销时间，也可以直接按 `Ctrl+Z`。从 `~/.ssh/config` 读取的主机只会在 Nebula 里隐藏，不会改动原文件；之后也能从设置页的隐藏主机入口找回来。撤销时会恢复原来的顺序，保存的密码也会等撤销时间结束后再清理。
- **SSH errors are shown where you can see them** — an invalid address keeps the text you entered, returns focus to the address box, and explains what needs fixing. If a terminal pane cannot be created, Nebula now shows the host, the reason, and what to try next instead of leaving the details only in the log.
  **中文：** SSH 地址填错时不会再悄悄失败：已经输入的内容会保留，光标会回到地址框，并直接告诉你哪里需要修改。终端面板创建失败时，界面也会显示目标主机、失败原因和下一步建议，不用再去日志里猜。
- **SSH fields now use familiar editing shortcuts** — address and password boxes support `Ctrl+A`, `Ctrl+C`, `Ctrl+V`, replacing selected text, Chinese IME input, and visible selection. Hidden passwords can be selected and pasted, but can only be copied after being revealed.
  **中文：** SSH 地址和密码框现在可以正常使用 `Ctrl+A`、`Ctrl+C`、`Ctrl+V`，也支持中文输入法、全选后直接替换和清晰的选中效果。隐藏状态下的密码可以选择和粘贴，但只有点开显示后才能复制。

### UI Hierarchy And Control Consistency / UI 层级与控件一致性

- **A more consistent interface** — spacing now follows a 4px rhythm, while type sizes, row heights, icon buttons, corners, borders, shadows, animations, and control states share the same visual rules across the app.
  **中文：** 界面的间距现在统一按 4px 节奏排布，字号、行高、图标按钮、圆角、描边、阴影、动画和各种操作状态也都使用同一套视觉规则，页面之间看起来更整齐、更一致。
- **Themes can follow the system** — Appearance now includes “Follow system light/dark mode”. Nebula switches between the matching light and dark themes while preserving the selected theme family. Choosing a theme card manually turns automatic switching off, so an explicit choice is never overwritten.
  **中文：** 新增跟随系统明暗模式。在“外观”里开启后，Nebula 会切换到同系列的浅色或深色主题，同时保留用户选择的主题系列；手动点选主题卡会退出自动跟随，不会覆盖用户明确选择的主题。
- **Text boxes behave the same everywhere** — renaming tabs, filtering files, entering Git commit messages, editing SSH hosts, and searching commands now all support the same copy, paste, select-all, replacement, IME, and selection behavior.
  **中文：** 各处输入框终于用起来一致了：无论是重命名标签页、筛选文件、填写 Git 提交信息、编辑 SSH 主机还是搜索命令，都能用同样的复制、粘贴、全选、替换和中文输入法操作。
- **A calmer sidebar** — `TABS` and `SSH HOSTS` now have clearer heading sizes, weights, and shades. The two `+` buttons only appear when the pointer is over their section title, the tab menu uses a vertical three-dot icon, and the empty SSH message is easier to read.
  **中文：** 侧栏现在更清爽了：`TABS` 和 `SSH HOSTS` 的字号、字重与灰度层级更清楚；两个 `+` 只会在鼠标移到对应标题时出现，标签页菜单改成竖向三点，SSH 为空时的提示也更容易看清。
- **Tab colors are now optional** — tabs no longer show a color strip by default. The strip appears only after you choose a color, and custom tab names and colors are restored with the session.
  **中文：** 标签页默认不再显示色条，只有用户主动设置颜色后才会出现；自定义名称和颜色也会跟随会话保存，下次打开仍然保留。
- **The `+` buttons are properly centered** — the icon, hover background, and clickable area now share the same center, so the button looks and feels aligned. Menu icons are also limited to shapes that the bundled Maple Mono Nerd Font can display reliably.
  **中文：** `+` 图标、悬停背景和实际可点击区域现在共用同一个中心，看起来不会再歪，点起来也更准确；菜单图标也只使用内置 Maple Mono Nerd Font 能稳定显示的字形，避免出现方框或错位。
- **Shell and profile search is back** — the picker can once again search and filter shells or profiles, with Chinese IME and familiar editing shortcuts. Search boxes and results use a compact 38px height, while SSH hints are brighter and easier to read.
  **中文：** Shell 和 Profile 选择器的搜索回来了，支持中文输入法、常用编辑快捷键和模糊筛选。搜索框与结果行统一收紧到 38px，SSH 提示文字也调亮了一些，不再灰得看不清。
- **Right-click menus feel lighter** — menus now use a soft theme-aware shadow, a subtle border, and a short open/close animation. Tab color labels and swatches also have more natural spacing.
  **中文：** 右键菜单加上了跟随主题的柔和阴影、细边框和短促的开合动画，层次更自然；标签页颜色名称和色块之间也留出了更舒服的间距。

### Architecture, Reliability, And Verification / 架构、可靠性与验证

- **Cleaner internal structure** — context menus, text editing, SSH UI state, and shared visual values now live in separate modules, making later changes easier to understand and less likely to affect unrelated parts of the app.
  **中文：** 右键菜单、文本输入、SSH 界面状态和通用视觉配置已经拆到各自的模块里，后续修改更容易看懂，也更不容易误伤其他功能。
- **Product experience verification** — normal, empty, and error states were reviewed together with destructive-action recovery, focus behavior, and font and icon reliability, making common workflows easier to understand and recover.
  **中文：** 完成正常、空白和错误状态的产品体验检查，并覆盖误操作恢复、焦点行为、字体和图标可靠性，让常用流程更容易理解，也更容易从错误中恢复。
- **More regression tests** — new tests cover the terminal bottom edge, split views, link underlines, transparent cursor and selection colors, overlapping links, menu placement, SSH deletion recovery, text editing, theme-family switching, and control-state priority. Current result: **188 passed; 0 failed**.
  **中文：** 新增回归测试，覆盖终端底部显示、分屏、链接下划线、透明窗口中的光标与选区、重叠链接、菜单位置、SSH 删除恢复、文本输入、主题系列切换和操作状态优先级。当前结果：**188 项通过，0 项失败**。

### Still In Progress / 还在继续做

- **Not marked as complete yet** — the full SSH connecting/connected/failed experience, further cleanup of `display/mod.rs`, one shared animation timeline, tab close/reflow animations, and the OpenGL/wgpu direction are still being worked on or evaluated.
  **中文：** 完整的 SSH 连接中/已连接/失败状态、`display/mod.rs` 的进一步拆分、统一动画时间线、标签页关闭与回流动画，以及 OpenGL/wgpu 方案选择都还在继续开发或评估，本次没有把它们算作已经交付。

## 0.3.0 - 2026-07-12

### Highlights / 亮点

- **Complete UI redesign** — the top bar and left sidebar now form a continuous L-shaped chrome shell, with a unified visual language across settings, the command palette, confirmation dialogs, and drawers.
  **中文：** 全面重设计窗口 UI：顶部栏与左侧栏组成连续 L 形 chrome，并统一设置、命令面板、确认框和抽屉的视觉语言。
- **Flexible tab interaction** — tabs support animated reordering, dragging into the active terminal to create a split, edge docking previews, and matching pointer feedback.
  **中文：** 标签页支持动画排序、拖入当前终端形成分屏、边缘停靠预览和对应的鼠标反馈。
- **Files and Git drawer** — adds a right-side directory tree and Git workspace with filtering, expansion, path dragging, file status, commit/push actions, and new full-color file-type icons.
  **中文：** 新增右侧目录树与 Git 工作区，支持筛选、展开、路径拖拽、文件状态、提交/推送操作以及新的彩色文件类型图标。
- **Markdown/GFM viewer** — adds read-only rendering for headings, lists, tables, task lists, code blocks, block quotes, links, and scrollable documents.
  **中文：** 新增 Markdown/GFM 只读查看器，支持标题、列表、表格、任务列表、代码块、引用、链接和滚动浏览。
- **Detected shells with brand icons** — discovers PowerShell, CMD, Git Bash, Nushell, WSL, and common Linux distributions and renders their full-color icons.
  **中文：** 新增 Shell 探测和品牌彩色图标，覆盖 PowerShell、CMD、Git Bash、Nushell、WSL 及常见 Linux 发行版。

### Terminal And Profiles / 终端与配置

- **New-tab shell menu** — the chevron beside `+` launches a detected shell or configured profile directly.
  **中文：** 标签栏 `+` 旁新增 Shell 菜单，可直接使用检测到的执行器或配置 Profile 创建标签页。
- **Inline default-shell picker** — the settings row expands in place, displays every detected shell with its color icon, persists the selected item, and collapses after selection.
  **中文：** 设置页“默认 Shell”改为原地展开列表，显示全部检测到的 Shell 及彩色图标；选择后立即持久化并收起。
- **Rich shell identifiers** — default-shell persistence supports `cmd`, `pwsh`, `nu`, and `wsl:<distribution>` while retaining Nebula prompt bootstrap support for PowerShell and Git Bash.
  **中文：** 默认 Shell 持久化支持 `cmd`、`pwsh`、`nu` 和 `wsl:<distribution>`，同时继续兼容 PowerShell/Git Bash 的 Nebula prompt bootstrap。
- **Appearance controls** — adds runtime window opacity, background image, background-image opacity, and independently scrollable settings sections.
  **中文：** 新增窗口透明度、背景图片、背景图片透明度控制，以及可独立滚动的设置分区。

### SSH

- **Native Rust SSH transport** — saved hosts now connect directly to a remote PTY channel without a wrapper shell, injected command, or external `ssh.exe` console window.
  **中文：** 保存的 SSH host 现在通过 Rust SSH 传输直接连接远端 PTY，不再依赖包装 Shell、命令注入或外部 `ssh.exe` 黑窗口。
- **Complete authentication chain** — resolves aliases, users, ports and identity files from `~/.ssh/config`, then supports private keys, OpenSSH certificates, encrypted-key passphrases, Windows OpenSSH Agent, Pageant, saved or prompted passwords, and keyboard-interactive/MFA.
  **中文：** 从 `~/.ssh/config` 解析别名、用户、端口和 IdentityFile，并支持私钥、OpenSSH 证书、加密密钥口令、Windows OpenSSH Agent、Pageant、已保存/现场输入密码以及 keyboard-interactive/MFA。
- **Connection reuse** — authenticated sessions are pooled by `user@host:port`, so additional SSH tabs open a new shell channel without repeating transport setup and authentication.
  **中文：** 已认证连接按 `user@host:port` 复用；后续 SSH 标签页直接创建新 Shell channel，无需重复传输握手和认证。
- **Standard host-key verification** — verifies and learns host keys through the standard `known_hosts` store, prompts on first connection, and rejects changed keys with a security warning.
  **中文：** 使用标准 `known_hosts` 校验和保存主机密钥；首次连接会确认，密钥变化时会拒绝连接并显示安全警告。
- **Authenticated remote Hook bridge** — remote AI lifecycle envelopes can travel through a private OSC protected by a random per-channel token; pane identity is always assigned locally before notifications are dispatched.
  **中文：** 远端 AI 生命周期信封可通过每通道随机令牌保护的私有 OSC 返回；通知分发前始终由本地分配 Pane 身份。
- **Built-in host editor** — the `SSH HOSTS` header has an add button and an internal form for `user@host`, optional non-default ports, and passwords.
  **中文：** `SSH HOSTS` 标题新增添加按钮和内部编辑面板，可输入 `user@host`、非默认端口和密码。
- **Secure credential persistence** — passwords are saved only with explicit consent and are stored in Windows Credential Manager, never in Nebula settings, command arguments, shell history, or logs.
  **中文：** 密码仅在用户明确选择保存时写入 Windows Credential Manager，绝不会进入 Nebula 设置、命令参数、Shell 历史或日志。
- **Host deletion and cleaner right-click behavior** — SSH rows keep their tab-style delete button and credential cleanup, while right-click no longer silently pins or reorders a host.
  **中文：** SSH host 行保留标签页式删除按钮和凭据清理；右键不再静默置顶或改变主机顺序。

### Session And Rendering / 会话与渲染

- **Smoother workspace interaction** — improves split layout, navigation animations, independent sidebar scrolling, tab rename input, hover hit-testing, and the resize HUD.
  **中文：** 改进分屏布局、导航动画、侧栏独立滚动、标签重命名输入、hover 命中和 resize HUD。
- **Safe image staging** — full-color shell icons, AI brand marks, and OSC 1337 images are staged into a final texture pass so inline images cannot corrupt later glyph batches.
  **中文：** 彩色 Shell 图标、AI 品牌标识和 OSC 1337 图片统一进入帧末贴图阶段，避免内联图片破坏后续 glyph batch。
- **Richer pane state** — expands OSC, cwd, process-state, and pane event routing for the directory tree, SSH activity, and AI CLI status indicators.
  **中文：** 扩展 OSC、cwd、进程状态和 pane 事件链路，为目录树、SSH 活动和 AI CLI 状态提供实时数据。

### Notes / 说明

- **Major update** — this release spans UI chrome, tabs and splits, the file drawer, Markdown, shell profiles, SSH, and the rendering pipeline.
  **中文：** 这是自 0.2.1 以来的大版本更新，覆盖 UI chrome、标签与分屏、文件抽屉、Markdown、Shell Profile、SSH 和渲染管线。

## 0.2.1 - 2026-07-11

### Fixes / 修复

- **Per-pane event routing** — window event batches previously resolved to one target pane, allowing output from a background tab to misroute keyboard input or terminal query replies. Events now route to their source pane, user input always targets the focused pane, and events for closed panes are dropped.
  **中文：** 修复逐 pane 事件路由：过去窗口事件批次只解析到单一 pane，后台标签输出可能导致键盘输入或终端查询回复发往错误 PTY；现在事件按来源 pane 路由，用户输入始终进入焦点 pane，已关闭 pane 的事件直接丢弃。
- **CJK text in chrome rendering** — removed the phantom spacer consumed after every wide glyph, which previously swallowed alternating CJK characters in ghost hints, HUD text, and link previews.
  **中文：** 修复 chrome 中的 CJK 文本渲染：移除宽字符后的虚假 spacer，避免幽灵提示、HUD 和链接预览隔字丢失。
- **History capture for wrapped prompts** — prompt text is reconstructed across soft-wrapped rows and snapshotted from the grid on Enter, preventing desynchronized keystroke buffers from polluting history.
  **中文：** 修复换行 prompt 的历史捕获：命令会跨软换行重建，并在按下 Enter 时直接从网格快照，避免失同步的按键缓冲污染历史。
- **`git.exe` close-confirmation noise** — Nebula's short-lived prompt helper is treated as stateless plumbing and no longer blocks tab closure with a busy-process dialog.
  **中文：** 修复 `git.exe` 触发关闭确认的问题：Nebula prompt 的短生命周期 git 辅助进程现在视为无状态工具，不再阻止标签页关闭。
- **Process lingering after window close** — teardown now terminates the shell tree first and drains ConPTY output on a detached thread, preventing `ClosePseudoConsole` deadlocks.
  **中文：** 修复窗口关闭后进程残留：销毁流程先终止 Shell 进程树，再由独立线程排空 ConPTY 输出，避免 `ClosePseudoConsole` 死锁。
- **ConPTY sideload hygiene** — `conpty.dll` is loaded only by absolute path when its matching `OpenConsole.exe` is present; failed resize calls now log warnings instead of aborting.
  **中文：** 改进 ConPTY side-load：仅在配套 `OpenConsole.exe` 存在时通过绝对路径加载 `conpty.dll`；resize 失败改为记录警告而非终止进程。

### Housekeeping / 工程维护

- **License and fixtures** — consolidated third-party attribution into `THIRD-PARTY-NOTICES` and renamed reference fixtures after the behavior they cover.
  **中文：** 将第三方许可归集到 `THIRD-PARTY-NOTICES`，并按实际行为重新命名参考测试 fixture。

## 0.2.0 - 2026-07-10

### Shell Experience / Shell 体验

- **Ctrl+V paste** — Windows and Linux users can paste with the expected shortcut while preserving bracketed paste and multi-line confirmation.
  **中文：** Windows 和 Linux 支持使用预期的 `Ctrl+V` 粘贴，同时保留 bracketed paste 和多行粘贴确认。
- **Safer pane spawning** — new tabs and splits validate inherited cwd before spawning, avoiding `os error 267` for deleted or virtual directories.
  **中文：** 新建标签和分屏前验证继承的 cwd，避免目录已删除或为虚拟目录时出现 `os error 267`。
- **SSH passthrough** — `nebula ssh user@host` bootstraps Nebula integration on Linux bash/zsh remotes while preserving forwarding, query, and explicit-command forms.
  **中文：** `nebula ssh user@host` 可在 Linux bash/zsh 远端引导 Nebula 集成，同时保持转发、查询和显式远程命令模式原样透传。

### AI Workflow / AI 工作流

- **opencode integration** — adds an opencode plugin that routes turn state through the same sidebar and toast bridge as Claude Code and Codex.
  **中文：** 新增 opencode 插件，通过与 Claude Code、Codex 相同的侧栏和通知桥接传递回合状态。
- **Remote AI awareness** — OSC cwd and command-state signals from bootstrapped SSH sessions update the local sidebar.
  **中文：** 已引导的 SSH 会话可把 OSC cwd 和命令状态信号传回本地侧栏。

### UI And UX / UI 与交互

- **Right-side Files/Git drawer** — adds filtering, persistent selection, drag-to-paste, Git staging/commit/push actions, and geometry aligned with the left tabs panel.
  **中文：** 新增右侧 Files/Git 抽屉，支持筛选、持久选择、拖拽粘贴和 Git 暂存/提交/推送，并与左侧标签栏对齐。
- **Chrome refactor** — moves chrome and side-panel rendering into dedicated modules while keeping rendering and hit-testing geometry synchronized.
  **中文：** 将 chrome 和侧面板渲染拆分到独立模块，同时保持渲染与 hit-test 几何同步。
- **Default font** — changes the packaged Nerd Font to `MapleMonoNormal-NF-CN-Regular.ttf`.
  **中文：** 发布包默认 Nerd Font 更换为 `MapleMonoNormal-NF-CN-Regular.ttf`。
- **Release documentation** — updates README and INSTALL for the 0.2 package and GPL-3.0-only licensing.
  **中文：** 更新 README 与 INSTALL 中的 0.2 发布包和 GPL-3.0-only 许可说明。

## 0.1.0 - 2026-07-07

Nebula Terminal's first public release.

Nebula Terminal 的第一个公开版本。

### AI Integration / AI 集成

- **Real brand marks in the sidebar** — renders the Anthropic starburst for `claude`, the OpenAI blossom for `codex`, and Nerd Font icons for other common developer tools.
  **中文：** 侧栏为 `claude` 显示 Anthropic 星芒、为 `codex` 显示 OpenAI 花结，并为其他常见开发工具显示 Nerd Font 图标。
- **Live turn state** — Claude Code hooks and Codex notify call the dependency-free `nebula-hook.exe`, forwarding prompt, completion, and input-needed events over a named pipe.
  **中文：** Claude Code hooks 和 Codex notify 调用无依赖的 `nebula-hook.exe`，通过命名管道转发提交、完成和等待输入事件。
- **Click-to-focus notifications** — activating a toast raises the window, selects the originating tab, and focuses the originating split.
  **中文：** 点击通知会前置窗口、选择来源标签页并聚焦来源分屏。
- **Zero setup and self-healing** — hook entries install automatically, recover after external configuration rewrites, remain scoped to Nebula, and can be removed with `nebula setup-ai --remove`.
  **中文：** hook 条目自动安装，可在外部配置重写后自愈，仅作用于 Nebula，并可通过 `nebula setup-ai --remove` 移除。
- **Codex chain mode** — wraps an existing Codex notifier instead of replacing it.
  **中文：** Codex chain 模式会包装已有 notifier，而不是覆盖它。
- **Fallback signals** — OSC 133 and BEL cover other CLIs and report long-command completion with duration.
  **中文：** OSC 133 和 BEL 为其他 CLI 提供兜底，并在长命令结束时报告耗时。

### Persistent Sessions / 会话保活

- **Session residency** — closing a window detaches its tabs while PTYs continue running; relaunching reattaches to the same processes and scrollback.
  **中文：** 关闭窗口仅分离标签页，PTY 继续运行；再次启动可接回相同进程和滚屏内容。
- **Cold restore** — autosaved tab layout and working directories restore after reboot or crash, with crash-loop protection.
  **中文：** 重启或崩溃后可从自动快照恢复标签布局和工作目录，并带崩溃循环保护。
- **Single instance** — subsequent launches hand off to the resident process.
  **中文：** 后续启动会交给常驻进程处理，保持单实例。

### Interface / 界面

- **Seven-theme skin system** — one token system drives seven light/dark themes across chrome, prompts, and dialogs, with persistence and hot reload.
  **中文：** 一套设计 token 驱动七种明暗主题，覆盖 chrome、prompt 和对话框，并支持持久化与热重载。
- **Sidebar tabs and splits** — supports tab reordering, drag-to-dock splits, dimmed unfocused panes, zoom, and CJK-aware chrome text.
  **中文：** 支持标签排序、拖拽停靠分屏、非焦点 pane 变暗、pane 缩放和 CJK-aware chrome 文本。
- **Quick terminal** — provides a global-hotkey Quake-style terminal with slide animation.
  **中文：** 提供全局快捷键唤起的 Quake 风格终端和滑入动画。
- **In-app settings** — configures themes, backgrounds, opacity, shells, and completion behavior in grouped panels with true clipping.
  **中文：** 应用内设置支持主题、背景、透明度、Shell 和补全行为，并使用真正裁剪的分组面板。
- **Chrome utilities** — adds the command palette, resize HUD, auto-hiding scrollbar, and visual bell.
  **中文：** 新增命令面板、resize HUD、自动隐藏滚动条和 visual bell。
- **Inline images** — supports OSC 1337 images with lazy upload and scrollback anchoring.
  **中文：** 支持 OSC 1337 内联图片、延迟上传和滚屏锚定。
- **Welcome page** — adds a fastfetch-style system introduction for new tabs.
  **中文：** 新标签页提供 fastfetch 风格的系统欢迎信息。

### Performance And Correctness / 性能与正确性

- **Modern ConPTY host** — bundles `conpty.dll` and `OpenConsole.exe`, pre-primes the DA1 handshake, improves resize behavior, and retains an in-box fallback.
  **中文：** 随包提供 `conpty.dll` 和 `OpenConsole.exe`，预热 DA1 握手、改善 resize，并保留系统内置 ConPTY 回退。
- **Coalesced resizing** — interactive resizing updates the PTY once after the drag settles, while rendering remains damage-tracked.
  **中文：** 交互式 resize 在拖动结束后一次性通知 PTY，同时继续使用 damage tracking 渲染。
- **Boot instrumentation** — `NEBULA_BOOT_TRACE=1` reports per-stage startup timing.
  **中文：** `NEBULA_BOOT_TRACE=1` 可输出逐阶段启动耗时。
- **Native notifications** — WinRT toasts use a registered Nebula identity, taskbar flashing, throttling, and a worker thread that cannot block rendering.
  **中文：** WinRT 通知使用注册的 Nebula 身份、任务栏闪烁和全局限流，并在独立线程运行以避免阻塞渲染。

### Shell Experience / Shell 体验

- **Fish-style ghost completions** — suggests commands from persistent JSONL history and filesystem paths, accepted with Right Arrow or Tab.
  **中文：** 从持久化 JSONL 历史和文件路径提供 fish 风格幽灵补全，可使用右方向键或 Tab 接受。
- **Built-in powerline prompt** — provides a themed Git branch and clock prompt for PowerShell and Git Bash without plugins.
  **中文：** 为 PowerShell 和 Git Bash 提供无需插件、包含 Git 分支和时钟的主题化 powerline prompt。
- **Input quality-of-life fixes** — supports unquoted paths with spaces, safely rewrites bare PowerShell environment assignments, and adds colored, clickable `ls` output.
  **中文：** 支持未加引号的空格路径、安全改写裸 PowerShell 环境变量赋值，并为 `ls` 增加彩色可点击输出。
- **OSC coverage** — supports OSC 7, 8, 9, 9;9, 133, and 1337 for cwd, hyperlinks, notifications, semantic prompts, and images.
  **中文：** 支持 OSC 7、8、9、9;9、133 和 1337，覆盖 cwd、超链接、通知、语义 prompt 和图片。
