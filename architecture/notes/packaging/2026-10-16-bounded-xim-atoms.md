# 固定 XIM 客户端属性原子名称

## Status

提议采用固定提交的依赖修复。传输回归与真实 Fcitx 拼音协议测试通过。
这不是正式发布或完整 GUI 验收记录。

## Context

Pebrel 的 Linux X11 输入路径通过 GPUI 使用 `zed-xim`。
X server 保存原子名称，其生命周期不随窗口或属性删除而结束。
原客户端按递增的 `u16` 序号生成 `_XIM_DATA_*`，在回绕前可产生 65536 个名称。
长期输入因此扩大整个桌面会话的原子表，并可能触发其他旧 X11 客户端的编号上限。
修复必须保留 XIM 中文输入和报文顺序。

## Evidence

- 原依赖固定在 [16f35a2](https://github.com/zed-industries/xim-rs/tree/16f35a2c881b815a2b6cdfd6687988e84f8447d8)。
- [依赖修复提交](https://github.com/MeshedPoto/xim-rs/commit/405efc92426b3dc3a761005709a84567185cc3ca) 包含传输实现与回归测试。
- [XIM 传输规范](https://xorg.freedesktop.org/archive/current/doc/libX11/i18n/trans/trans.html) 定义属性报文和 ClientMessage 传输。
- 修复前后使用同一积压消息测试。旧分配逻辑首轮产生 1755 个名称；修复后全部三轮只使用 32 个。

## Decision

在根 Cargo 清单覆盖原 XIM Git 来源，只替换 `zed-xim` 的固定提交。
其同仓库路径依赖 `xim-parser` 和 `xim-ctext` 随之解析到同一 fork 提交。
保留原包版本、特性和 GPUI 固定点；不升级无关依赖，不引用浮动分支。

X11rb 客户端发送复用 32 个固定的 `_XIM_DATA_*` 名称，继续使用 `APPEND`。
接收器按窗口和原子保存字节偏移，每个 ClientMessage 只消费对应长度。
X11 读取参数转换为四字节单位；读到属性末尾时由 X server 原子删除属性并清除偏移。
该规则保留积压属性报文与内联报文的事件顺序。

服务器发送仍保持原策略，避免扩大到尚未修复的 xlib 客户端接收路径。
没有修改输入编码、中文提交、预编辑接口、线程模型或持久化格式。
依赖中同时修正三个示例文件及服务器反馈枚举的既有编译问题，使所用特性测试能在稳定工具链运行。

## Rejected alternatives

- 禁用 XIM：会失去中文输入能力。
- 用 `REPLACE` 复用属性：可能覆盖尚未消费的报文。
- 只循环名称、不处理读取偏移：可能重复读取首帧并丢失后续报文。
- 将本机相邻源码目录作为依赖：上游和 CI 无法复现。
- 将整套依赖复制进 Pebrel：扩大审查和维护范围；固定提交已经可以提供可追溯来源。
- 同时修改服务器发送：不是 Pebrel 客户端修复所需，会扩大兼容范围。

## Consequences

这条客户端发送路径最多使用 32 个名称。补丁不能删除当前 X server 已有的原子，
也不限制其他应用或服务器发送路径产生的名称。
新增的 Git fork 是临时维护来源，需要保持可获取并审查后续更新。

互操作验证覆盖 Fcitx5 和修复后的 x11rb 接收器，不保证所有旧 XIM 服务器。
例如原版 xim-rs 服务端始终从偏移零读取，积压超过复用槽位时仍可能丢帧。
不能通过禁用中文输入掩盖这类兼容问题。

## Validation

依赖的 `cargo test --features x11rb-client,x11rb-server -- --test-threads=1` 通过两项测试。
测试需要 Xvfb，可用 `XIM_TEST_XVFB` 指定路径；测试自行创建私有显示，不连接主 `DISPLAY`。
6144 条消息覆盖三次连接、两窗口、积压、长短消息顺序、属性排空后复用、无效长度和格式。
恢复旧计数逻辑后，原子数量断言按预期失败。

独立 Fcitx 5.1.7 拼音实例向修复后的 XIM 客户端提交 40 次“你好”，全部匹配，
收到 240 次预编辑回调，共转发 480 次按键事件。该验证使用独立显示和 D-Bus。
同一修复源码在 Pebrel v2.2.1 的 Linux GPUI release 构建通过。
PR 基线 `8de5331f5607a24c72a8cf209ab4099c8ba4c424` 上的
`cargo check -p nebula --bin pebrel --features gpui-shell --tests --locked --release` 通过。
架构检查、55 项架构规则测试、99 项设置测试、24 项国际化测试（另 1 项忽略）及格式检查通过。
依赖元数据确认三个 XIM 包各只有一个实例，均来自同一修复提交；锁文件没有无关升级。
这些结果不替代完整 GUI 人工验收、长期运行、其他输入法或 Windows/macOS 验证。
未声称 `--all-features` 测试通过；原 xlib 示例的既有编译问题不在本次范围内。

## Supersedes

None。

## Revisit when

GPUI 所固定的 XIM 来源包含等效的有界名称与队列消费修复，并通过上述回归与真实中文输入验证后，
删除根补丁，统一更新上游固定点与锁文件。
若 fork 可用性或输入法兼容性改变，先复现并选择已审查的固定提交，不退回浮动分支。
