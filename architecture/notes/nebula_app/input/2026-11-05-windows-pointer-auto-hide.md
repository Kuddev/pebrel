# Windows 打字隐藏鼠标指针只在真实移动后恢复

## Status

提议采用固定提交的 GPUI 修复（issue #868）。Windows 目标类型检查与状态机单元测试通过；
尚未在 Windows 真机上做 GUI 验收。

## Context

GPUI 默认 `CursorHideMode::OnTypingAndAction`：产生字符的按键调用
`hide_cursor_until_mouse_moves`，Windows 后端 `SetCursor(None)` 隐藏指针，
并在 `WM_MOUSEMOVE` / `WM_NCMOUSEMOVE` 中无条件恢复。
用户在 Windows 11 上把指针停在终端里打字，指针每按一次键就闪一下，
而期望与系统常见终端一致：第一次按键后隐藏，移动鼠标后才出现。
Pebrel 应用层没有隐藏或恢复系统指针的代码，`load_cursor` 也不会返回空指针，
因此无法在应用层绕开，只能修 `gpui_windows`。

## Evidence

- 固定基线 `Kuddev/zed@1e716d16`：`crates/gpui_windows/src/events.rs`
  的 `handle_mouse_move_msg` 与 `handle_nc_mouse_move_msg` 先调用
  `restore_cursor_after_hide`，只要隐藏标志为真就立即 `SetCursor(current_cursor)`。
- Win32 在窗口显示、隐藏、移动或需要重新计算指针形状时，会在指针没动的情况下
  设置“鼠标移动”标志并补发 `WM_MOUSEMOVE`。Raymond Chen 的
  [Why do I get spurious WM_MOUSEMOVE messages?](https://devblogs.microsoft.com/oldnewthing/20031001-00/?p=42343)
  说明了这一点，并建议比较坐标判断是否真的移动。
- 终端每次按键都会重绘（光标、回显、平滑光标动画），补发的移动消息随之出现，
  隐藏后马上被恢复，表现为每次按键闪烁。

## Decision

修复只改 `gpui_windows`，基于 `1e716d16`：

- 新增 `CursorVisibility`，替换原 `Arc<AtomicBool>`，由平台和所有窗口共享。
  隐藏时用 `GetCursorPos` 记录屏幕坐标；已隐藏时再次隐藏保留第一次的坐标。
- 客户区移动经 `ClientToScreen` 换算，非客户区消息本身就是屏幕坐标；
  只有坐标与隐藏位置不同才恢复指针。坐标无法取得时退回原来的“任意移动即恢复”。
- 离开窗口和窗口失活仍直接复位为可见，与原行为一致。

修复提交 `80142b1e` 经 Kuddev/zed#3 并入维护分支 `fix/native-window-input-lifetime-20261015`，
合并提交 `4f2c269f` 同时包含 #744 的 DirectWrite 修复（Kuddev/zed#2）。
`gpui-component` 经 Kuddev/gpui-component#3 合并为 `4b62121b`，五个 Zed 引用都指向 `4f2c269f`，
保证全图只有一套 GPUI 类型。锁文件只替换这两个 Git 来源。

## Rejected alternatives

- 应用层关闭自动隐藏（`CursorHideMode::Never`）：消除闪烁，但失去用户期望的隐藏行为。
- 应用层在每次按键后再次隐藏：补发的移动消息仍会先恢复，只是把闪烁变成另一种时序。
- 按时间窗口忽略按键后的移动：真实的快速移动会被吞掉，阈值也依赖机器负载。
- 在根清单用 `[patch."https://github.com/Kuddev/zed"]` 重定向：两个依赖都按 rev 固定，
  Cargo 的补丁按来源匹配，实测无法覆盖组件内的固定引用，并解析出冲突。
- 引用浮动分支或本机路径：违反精确固定约定，CI 无法复现。

## Consequences

- 指针静止时任何补发移动都不会恢复它；只要屏幕坐标变化一个物理像素就恢复。
- 隐藏期间如果系统把指针移到别处（如其他程序调用 `SetCursorPos`），下一次移动消息
  坐标不同，指针会恢复，符合预期。
- macOS 与 Linux 后端不变。

## Validation

- `CursorVisibility` 的 4 项单元测试：同坐标补发移动保持隐藏、重复隐藏保留首个坐标、
  未知坐标任意移动恢复、复位后可重新隐藏。
- Windows 目标（`x86_64-pc-windows-msvc`）上 `cargo clippy -p gpui_windows --tests`
  在修改文件中无警告；交叉检查环境没有 MSVC 工具链，C/RC 构建步骤用空桩代替，
  只验证 Rust 类型检查，不产生可运行二进制。
- 同一交叉环境下 `cargo check -p nebula --bin pebrel --features gpui-shell
  --target x86_64-pc-windows-msvc --locked` 通过（已有警告不变）。
- macOS 本机缺少 Metal Toolchain，`gpui_macos` 着色器构建失败，未能完成 macOS
  检查；`gpui_macos` 在本次 rev 区间内没有改动。
- 未做 Windows 真机 GUI 验收：需在 Windows 11 上把指针停在终端里连续打字，
  确认指针保持隐藏，移动鼠标后立即出现。

## Supersedes

None。

## Revisit when

上游 Zed 提供等效的真实移动判断后，随 GPUI 升级移除这段 fork 修改。
若 Windows 真机验收发现坐标相同的真实移动（如高精度设备）仍需恢复，先复现再调整判据。
