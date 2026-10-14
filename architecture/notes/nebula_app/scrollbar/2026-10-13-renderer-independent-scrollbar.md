# Shared scrollbar rules are independent of the retiring legacy shell

## Status

首个共享滚动条切片已实现并通过下述针对性验证；旧壳整体退役仍在进行。
维护者于 2026-10-13 明确选择逐段迁移，而不是继续扩展旧 OpenGL 壳。

## Context

默认产品使用 GPUI。`main.rs` 已通过 feature 选择 `product_ui`、
`product_renderer`、`product_input` 和 `product_window_context`，但这些门面
仍以 `#[path]` 复用旧目录中的共享代码。目录名不是运行时依赖边界。

滚动条的几何、命中和拖拽映射已经与后端无关，却位于 `display/ui/scrollbar.rs`。
GPUI 的 Tab 列表通过 `display::ui::widgets` 使用它；对应的三个几何测试则
位于需要旧绘制类型的 `display/ui/widgets.rs`。直接删除旧目录会连同共享规则
一起删除，只复制到 GPUI 又会形成第二份实现。

## Evidence

- 当前 Windows 主机的默认应用依赖树没有 `glutin`、`crossfont` 或旧 GL 绑定
  生成器；显式启用 `legacy-shell` 才出现这些依赖。
- `winit` 仍是默认依赖，现有配置和按键等代码仍借用其类型；旧事件循环未因此
  进入默认启动路径。移除该依赖需要另外迁移实际类型消费者。
- `gpui_shell/workspace/tab_scroll.rs` 是旧 `product_ui::ui::widgets` 几何门面的
  GPUI 消费者；旧绘制调用则通过 `display/ui/widgets.rs` 使用相同规则。
- Tab 滑块的 `.occlude()` 会遮断决定其显隐的父列表 hover。静止鼠标触发的
  重绘会形成反馈循环，不能用强制常显、延时或新增轮询代替修复命中关系。

## Decision

将原几何实现及其三个现有测试迁到应用级 `scrollbar.rs`，保持数据结构和
计算表达式不变。它不依赖 GPUI、winit、OpenGL 或配置持久化。

GPUI Tab 列表直接使用 `crate::scrollbar`，移除已无消费者的产品 `widgets`
门面。旧 `display/ui/widgets.rs` 暂时转导出同一模块，维持旧绘制调用的类型
与行为，且不让共享规则反向依赖旧壳。

Tab 滑块由私有绘制函数生成，不遮挡父列表。独立轨道继续处理点击，拖拽
捕获层继续拥有拖拽输入；尺寸、配色、滚动范围和行密度合同保持不变。
原有静止指针回归随同这条真实使用路径进入迁移分支。

## Rejected alternatives

- 一次删除 `display/`、`renderer/` 和输入目录：其中仍有 GPUI 的真实消费者。
- 为 GPUI 复制一份几何或建立另一套滑动状态机：共享规则会分叉。
- 新建服务、通用组件注册器或另一层兼容子系统：现有模块和一次转导出足够。
- 为旧壳增加无关修复再开始迁移：默认产品不使用旧渲染链路；应优先切开共享
  规则与旧绘制的关系。

## Consequences

这一步只切开滚动条几何权威，不声称全部 `display` 引用或 winit 依赖已消除。
行密度令牌等其他共享模块仍有自己的迁移边界。旧壳入口和旧绘制代码暂时保留，
后续移除时不再需要从旧 UI 目录中抢救这份滚动条算法。

没有新增生产依赖、线程、缓存、持久化字段或堆分配；私有绘制函数保留原有
元素结构。调试 bounds 标识在不带 GPUI test-support 的产品构建中为空操作。

## Validation

在 Windows 主机、固定 Rust 1.97.1 与当前锁定依赖下，几何模块独立编译并通过
三个原有测试。实际应用测试二进制中的三个几何测试、七个 Tab 列表测试
（含真实 GPUI 悬停回归）通过，默认 GPUI 产品编译与架构检查通过。
采用现有 CI 编译 profile，不运行全仓测试；组件测试不等同于完整应用或
跨平台视觉验收，也不宣称旧壳扩展构建已完成验证。

## Supersedes

None。保留历史决策的原始依据；这是逐步退役方向下的第一个共享规则切片。

## Revisit when

旧壳绘制消费者移除后，删除临时转导出。其余共享模型、输入协议和窗口类型
完成迁移并验证 GPUI 路径后，再移除旧入口、`legacy-shell` 及其专属依赖。
