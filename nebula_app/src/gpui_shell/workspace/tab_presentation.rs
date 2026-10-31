//! Shared tab names, task details and identity for sidebar and top tabs.

use super::*;

/// 标签元数据：用户名字、色标沿用 session v4；运行时身份只随活体标签移动，
/// 不写入 session，避免重启后的请求命中新标签。
/// 与 `tabs` **同下标**。长度必须一致，所以增删移三种结构性改动只允许走
/// `insert_tab_at` / `remove_tab_at` / `move_tab`——别处直接 `self.tabs.push`
/// 会让色条和名字错位到邻居身上。
#[derive(Clone, Debug)]
pub(super) struct TabMeta {
    pub(super) runtime_id: crate::runtime_api::tabs::TabId,
    /// 用户重命名过的标签名；`None` = 跟着 cwd/文件名自动走。
    pub(super) custom_name: Option<String>,
    /// 色标（右键菜单的标签颜色）；`None` = 不画色条。
    pub(super) color: Option<Rgb>,
    /// 本 Tab 创建时实际采用的 shell 短标。默认 shell 是“新建时参数”，
    /// 不是全局实时主题；设置改变后既有 PTY 不会换进程，这个标签也不能
    /// 跟着全局值漂移。非终端 Tab 为 `None`。
    pub(super) shell_tag: Option<SharedString>,
    /// 与旧壳 `TabEntry::launch` 同义：保存“这个 Tab 创建时实际采用什么
    /// 启动方式”。共享 session v4 已有完整 schema，GPUI 只需把它保留下来，
    /// 不能在快照时把所有本地 Tab 都降级成 `None`。
    pub(super) launch: Option<crate::session::LaunchSession>,
    /// 后台 tab 响过 BEL（旧壳 `has_bell`）。激活即清。
    pub(super) has_bell: bool,
}

impl Default for TabMeta {
    fn default() -> Self {
        Self {
            runtime_id: Default::default(),
            custom_name: None,
            color: None,
            shell_tag: None,
            launch: None,
            has_bell: false,
        }
    }
}

/// 两种 tab 布局共用的只读展示数据。状态与动作仍由 `NebulaWorkspace`
/// 持有；这里只集中 cwd 标题、程序图标、AI 活动和用户元数据的解释。
pub(super) struct TabPresentation {
    pub(super) title: SharedString,
    /// 侧栏任务条目第一行的展示名：Agent 会话期显示品牌名（"Claude Code"），
    /// 其余与 `title` 一致。顶栏、runtime API 与重命名预填继续用 `title`
    /// （目录名身份），品牌名只属于分组视图的条目行。
    pub(super) headline: SharedString,
    pub(super) tooltip: Option<SharedString>,
    pub(super) is_settings: bool,
    pub(super) activity: SidebarActivity,
    pub(super) logo_image: Option<Arc<RenderImage>>,
    pub(super) logo_pending: bool,
    pub(super) program_glyph: Option<&'static str>,
    pub(super) shell_tag: Option<SharedString>,
    pub(super) color: Option<Rgb>,
    pub(super) renaming: Option<Entity<InputState>>,
    /// 本 tab 的分屏数（Terminal tab 才 > 0）。`> 1` 时行尾显示布局缩略图；
    /// 见 [`pane_header::split_layout_icon`]。
    pub(super) pane_count: usize,
    pub(super) local_administrator: bool,
    /// 聚焦 pane 是否是识别出的 AI agent 会话（`AgentKind::parse(program)`，
    /// 与品牌 logo / `agent_headline` 同一权威）。agent 侧栏模式用它过滤
    /// 状态徽章：普通终端不画常规状态（pane 级故障除外，见
    /// [`sidebar::agent_scoped_activity`]）；顶栏不消费该字段。
    pub(super) is_agent_session: bool,
}

impl NebulaWorkspace {
    /// 完整标签文本。**不在这里截断**：可见宽度是布局问题，字符数上限会在
    /// 窄侧栏下漏出、在宽侧栏下白扔字符。截断由 `render_sidebar` 按实测
    /// cell 宽换算成列数后交给旧壳的 `truncate_tab_label`（带省略号）。
    pub(super) fn tab_title(&self, ix: usize, cx: &App) -> SharedString {
        if let Some(custom) = self.meta(ix).custom_name {
            return custom.into();
        }
        match &self.tabs[ix] {
            WorkspaceTab::Settings { .. } => crate::gpui_shell::config::ui_language(cx)
                .text(crate::i18n::Message::CommonSettings)
                .into(),
            WorkspaceTab::Image { view } => view.read(cx).title.clone().into(),
            WorkspaceTab::Document { view, .. } => view.read(cx).tab_title().into(),
            WorkspaceTab::Code { view, .. } => view.read(cx).tab_title(cx).into(),
            tab @ WorkspaceTab::Terminal { .. } => {
                // 标签 = 聚焦 pane 的 cwd 末级目录名（旧壳 chrome_tab_label
                // 规则）。分屏计数**不拼在这里**：这份字符串还要喂给 runtime
                // API 的 tab label、跨窗拖拽标题和重命名预填，掺进 "⊞2" 会
                // 一路泄漏，而且长标题下会被 truncate_tab_label 截掉、被
                // custom_name 整条顶掉。计数改由 TabPresentation::pane_count
                // 决定是否显示布局缩略图，见 sidebar/top_tabs 的渲染。
                match tab.focused_view() {
                    Some(view) => view.read(cx).tab_label().into(),
                    None => SharedString::from("shell"),
                }
            },
        }
    }

    pub(super) fn tab_presentation(&self, ix: usize, cx: &App, dark: bool) -> TabPresentation {
        let active = ix == self.active;
        let title = self.tab_title(ix, cx);
        let is_settings = self.tabs[ix].is_settings();
        let is_terminal = self.tabs[ix].is_terminal();
        let local_administrator = crate::platform::elevation::is_elevated().unwrap_or(false)
            && self.tabs[ix]
                .focused_view()
                .is_some_and(|view| view.read(cx).inherits_windows_host_token());
        let pane_count = match &self.tabs[ix] {
            WorkspaceTab::Terminal { panes, .. } => panes.len(),
            _ => 0,
        };
        let (program, activity) = self.tabs[ix]
            .focused_view()
            .map(|entity| {
                let view = entity.read(cx);
                let program = view
                    .running_program
                    .clone()
                    .or_else(|| view.ai_session.as_ref().map(|identity| identity.source.clone()))
                    .or_else(|| view.ssh_destination.as_ref().map(|_| "ssh".to_owned()));
                (program, view.sidebar_activity())
            })
            .unwrap_or((None, SidebarActivity::Idle));
        // 事件 vs 状态的唯一裁定处（侧栏与顶栏共用这份 presentation），语义
        // 跟随「AI Agent 侧边栏」开关：开 = 完成读作 Ready 状态（当前 tab
        // 也画，见 [`sidebar_tree::tree_resting_activity`]）；关 = main 的
        // 经典语义（完成是事件，当前 tab 不画，[`sidebar::resting_activity`]，
        // 与上游逐字一致）。
        let activity = if crate::gpui_shell::config::ai_agent_sidebar(cx) {
            sidebar_tree::tree_resting_activity(activity, active, self.meta(ix).has_bell)
        } else {
            sidebar::resting_activity(activity, active, self.meta(ix).has_bell)
        };
        // agent 身份只 parse 一次：headline 与 `is_agent_session` 共用。
        let agent = program.as_deref().and_then(crate::ai_agents::AgentKind::parse);
        let brand_logo = program.as_deref().and_then(crate::display::ai_logo_for_program);
        let logo_image =
            brand_logo.and_then(|logo| self.sidebar_logo_images.get(&(logo, dark)).cloned());
        let logo_pending = brand_logo.is_some() && logo_image.is_none();
        let program_glyph = program
            .as_deref()
            .filter(|_| logo_image.is_none())
            .map(crate::display::program_icon)
            .or_else(|| match &self.tabs[ix] {
                WorkspaceTab::Document { .. } => Some("\u{eb1d}"),
                WorkspaceTab::Code { view, .. } => {
                    Some(crate::display::side_panel::file_type_icon(&view.read(cx).title))
                },
                WorkspaceTab::Image { view } => {
                    Some(crate::display::side_panel::file_type_icon(&view.read(cx).title))
                },
                _ => None,
            });
        let meta = self.meta(ix);
        // Agent 会话期的侧栏第一行标题：品牌名优先于目录名；用户重命名
        // 是显式意志，永远最优先。退出 agent 后自然回落目录名。
        let headline = agent_headline(
            meta.custom_name.as_deref(),
            agent.filter(|_| is_terminal),
            title.clone(),
        );
        // 分屏 tab 不画 shell 短标：一个 tab 里的 N 个 pane 完全可能跑着不同
        // 的 shell，只贴其中一个（聚焦那个）是误导；数量胶囊「这是一组」才是
        // 此时该占这个槽位的信息。顺带把 28px 让回标题——顶栏挤到 120px 时，
        // 图标+胶囊+短标三样一起上，标题只剩两三个字符。
        let shell_tag = (is_terminal && activity == SidebarActivity::Idle && pane_count <= 1)
            .then_some(meta.shell_tag.clone())
            .flatten()
            .filter(|tag| !tag.is_empty());
        let renaming = self
            .tab_rename
            .as_ref()
            .filter(|rename| rename.ix == ix)
            .map(|rename| rename.input.clone());
        let tooltip =
            self.tabs[ix].focused_view().map(|view| view.read(cx).tab_tooltip(&title).into());
        TabPresentation {
            title,
            headline,
            tooltip,
            is_settings,
            activity,
            logo_image,
            logo_pending,
            program_glyph,
            shell_tag,
            color: meta.color,
            renaming,
            pane_count,
            local_administrator,
            is_agent_session: agent.is_some(),
        }
    }

    /// 当前标签的元数据（下标越界时给一份默认值，读取点因此不必各自判空）。
    pub(super) fn meta(&self, ix: usize) -> TabMeta {
        self.tab_meta.get(ix).cloned().unwrap_or_default()
    }

    /// 所有新建标签共用同一个插入口，避免本地/SSH/设置各自解释配置。
    pub(super) fn insert_new_tab(&mut self, tab: WorkspaceTab) {
        let position = nebula_settings::RuntimeSettings::load().new_tab_position;
        let at = new_tab_insert_index(position, self.active, self.tabs.len());
        self.insert_tab_at(at, tab, TabMeta::default());
        self.active = at;
        self.reveal_active_tab();
    }
}

/// 新建标签的下标：设置「新标签位置」（当前标签之后/末尾）与当前布局
/// 共同决定。放这里因为它只被 [`Self::insert_new_tab`] 一类的新建路径
/// 消费（`insert_new_tab` 在本模块）。
pub(super) fn new_tab_insert_index(
    position: nebula_settings::NewTabPositionName,
    active: usize,
    tab_count: usize,
) -> usize {
    match position {
        nebula_settings::NewTabPositionName::AfterCurrent if tab_count > 0 => {
            active.saturating_add(1).min(tab_count)
        },
        nebula_settings::NewTabPositionName::AfterCurrent
        | nebula_settings::NewTabPositionName::End => tab_count,
    }
}

/// 侧栏条目第一行显示名：识别出的 agent 品牌名替换目录名；用户重命名与
/// 非 agent 程序保持 `title` 原样。`agent` 是 [`Self::tab_presentation`] 里
/// 已经 parse 好的身份（与 `is_agent_session` 同一份）。
fn agent_headline(
    custom_name: Option<&str>,
    agent: Option<crate::ai_agents::AgentKind>,
    title: SharedString,
) -> SharedString {
    if custom_name.is_some() {
        return title;
    }
    agent.map(|agent| SharedString::from(agent.display_name())).unwrap_or(title)
}

pub(super) fn tooltip(text: SharedString, window: &mut Window, cx: &mut App) -> gpui::AnyView {
    gpui_component::tooltip::Tooltip::element(move |_, _| {
        div().max_w(px(560.0)).whitespace_normal().child(text.clone())
    })
    .build(window, cx)
}

/// [`tooltip`] 的可寻址变体：内容根带 debug selector。tooltip 是窗口级覆盖
/// 层，UI 测试断言「悬浮层确实画出了这段文本」只能靠 debug_bounds 寻址
/// （同一时刻窗口至多画一个 tooltip，静态 selector 多行共用不冲突）。
pub(super) fn tooltip_with_selector(
    text: SharedString,
    selector: &'static str,
    window: &mut Window,
    cx: &mut App,
) -> gpui::AnyView {
    gpui_component::tooltip::Tooltip::element(move |_, _| {
        div()
            .debug_selector(move || selector.to_owned())
            .max_w(px(560.0))
            .whitespace_normal()
            .child(text.clone())
    })
    .build(window, cx)
}

/// 单行文本按指定字体与字号的完整渲染宽度（不换行、不截断时的固有宽）。
/// 「真被截断才显示提示」的两个消费方共用：行内探针（layout 时与可用宽
/// 比较）与侧栏分支 chip（build 时的宽度预算比较）。字体度量来自应用
/// 自带的内嵌字体，跨平台结果一致。
pub(super) fn text_line_width(
    text_system: &gpui::WindowTextSystem,
    text: &str,
    font: gpui::Font,
    font_size: gpui::Pixels,
) -> gpui::Pixels {
    let run = gpui::TextRun {
        len: text.len(),
        font,
        color: gpui::Hsla::default(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    text_system.shape_line(text.into(), font_size, &[run], None).width
}

/// 「真被截断才显示悬浮全文」的截断探针（用户 10-23 需求）。
///
/// gpui 的 div tooltip API 不接受「不显示」，且省略号发生在 paint 阶段、
/// 元素构建时无从得知文本是否被截断。做法：把返回的探针（一个零高全宽
/// 的 canvas）放进被测的 `truncate()` div 里，layout 阶段拿到该 div 的
/// 实际内容宽，与 [`text_line_width`] 的全文固有宽比较，结果写入共享
/// Cell。tooltip 至少在悬浮延迟（默认 500ms）到期后才构建，那时读到的
/// Cell 已是当前布局的真值；未截断时消费方返回空视图（[`gpui::Empty`]，
/// 什么都不画，等效没有 tooltip）。
///
/// 探针高度为 0、不参与命中，不影响宿主 div 的布局；但 `w_full()` 依赖
/// 宿主 div 自身宽度由外部约束（flex 收缩/`max_w`）决定——这正是被测
/// 截断条件的来源。
pub(super) fn truncation_probe(
    text: SharedString,
    font: gpui::Font,
    font_size: gpui::Pixels,
) -> (std::rc::Rc<std::cell::Cell<bool>>, impl gpui::IntoElement) {
    let truncated = std::rc::Rc::new(std::cell::Cell::new(false));
    let probe = truncated.clone();
    let canvas = gpui::canvas(
        move |bounds, window, _| {
            let width = text_line_width(window.text_system(), &text, font, font_size);
            // 半像素容差吸收字体取整：恰好等宽不算截断。
            probe.set(width > bounds.size.width + px(0.5));
        },
        |_, _, _, _| {},
    )
    .w_full()
    .h(px(0.0));
    (truncated, canvas)
}

pub(super) fn administrator_badge(selector: SharedString, cx: &App) -> gpui::Stateful<gpui::Div> {
    let label = crate::gpui_shell::config::ui_language(cx)
        .text(crate::i18n::Message::ChromeLocalAdministrator);
    div()
        .id(selector.clone())
        .debug_selector(move || selector.to_string())
        .role(gpui::accesskit::Role::Image)
        .aria_label(label)
        .flex_shrink_0()
        .size(px(TAB_LABEL_ICON_SIZE))
        .flex()
        .items_center()
        .justify_center()
        // 权限提示与任务图标同尺寸；矢量图避免 emoji 字体改变颜色和几何。
        .child(
            Icon::new(Icon::empty())
                .path(crate::gpui_shell::assets::nav::SHIELD)
                .size(px(TAB_LABEL_ICON_SIZE))
                .text_color(cx.theme().warning),
        )
        .tooltip(move |window, cx| tooltip(label.into(), window, cx))
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests {
    use super::*;
    use gpui::{AssetSource as _, TestAppContext};
    use gpui_component::Root;

    #[test]
    fn agent_headline_prefers_brand_name_but_never_a_rename() {
        let title = || SharedString::from("pebrel");
        assert_eq!(
            agent_headline(None, agent("claude"), title()),
            SharedString::from("Claude Code")
        );
        assert_eq!(agent_headline(None, agent("kimi"), title()), SharedString::from("Kimi Code"));
        // 用户重命名是显式意志，agent 在跑也不顶掉。
        assert_eq!(
            agent_headline(Some("my-task"), agent("claude"), title()),
            SharedString::from("pebrel")
        );
        // 非 agent 程序与空闲 shell 保持目录名。
        assert_eq!(agent_headline(None, agent("ssh"), title()), title());
        assert_eq!(agent_headline(None, None, title()), title());
    }

    fn agent(raw: &str) -> Option<crate::ai_agents::AgentKind> {
        crate::ai_agents::AgentKind::parse(raw)
    }

    struct BadgeSurface;

    impl Render for BadgeSurface {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            h_flex()
                .h(px(36.0))
                .items_center()
                .child(administrator_badge("test-administrator".into(), cx))
                .child(
                    div()
                        .debug_selector(|| "test-task-icon".into())
                        .size(px(TAB_LABEL_ICON_SIZE))
                        .flex_shrink_0()
                        .child(Icon::new(IconName::SquareTerminal).size(px(TAB_LABEL_ICON_SIZE))),
                )
                .child(div().debug_selector(|| "test-admin-title".into()).child("pwsh"))
        }
    }

    #[gpui::test]
    fn administrator_badge_keeps_a_readable_slot_with_reduced_motion(cx: &mut TestAppContext) {
        let asset = crate::gpui_shell::assets::NebulaAssets
            .load(crate::gpui_shell::assets::nav::SHIELD)
            .unwrap()
            .expect("administrator shield must be embedded in the product");
        assert!(std::str::from_utf8(&asset).unwrap().contains("viewBox=\"0 0 24 24\""));
        cx.update(|cx| {
            gpui_component::init(cx);
            cx.set_global(crate::gpui_shell::config::Settings::load(
                nebula_settings::ThemeName::Nord,
            ));
        });
        let (_, window) = cx.add_window_view(|window, cx| {
            let surface = cx.new(|_| BadgeSurface);
            Root::new(surface, window, cx)
        });
        let mut previous = None;
        for reduced_motion in [false, true] {
            window.update(|window, cx| {
                cx.set_reduce_motion(reduced_motion);
                window.refresh();
                window.draw(cx).clear(cx);
            });
            let badge = window.debug_bounds("test-administrator").unwrap();
            let task = window.debug_bounds("test-task-icon").unwrap();
            let title = window.debug_bounds("test-admin-title").unwrap();
            assert_eq!(badge.size, gpui::size(px(TAB_LABEL_ICON_SIZE), px(TAB_LABEL_ICON_SIZE)));
            assert_eq!(badge.size, task.size);
            assert!(badge.right() <= task.left());
            assert!(task.right() <= title.left());
            if let Some(previous) = previous {
                assert_eq!(badge, previous);
            }
            previous = Some(badge);
        }
    }
}
