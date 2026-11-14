//! Shared tab names, task details and identity for sidebar and top tabs.

use super::*;

/// 标签元数据：用户名字、色标沿用 session v4；运行时身份只随活体标签移动，
/// 不写入 session，避免重启后的请求命中新标签。
/// 与 `tabs` **同下标**。长度必须一致，所以增删移三种结构性改动只允许走
/// `insert_tab_at` / `remove_tab_at` / `move_tab` / `reorder_tabs`——别处直接 `self.tabs.push`
/// 会让色条和名字错位到邻居身上。
#[derive(Clone, Debug, Default)]
pub(super) struct TabMeta {
    pub(super) runtime_id: crate::runtime_api::tabs::TabId,
    /// 用户重命名过的标签名；`None` = 跟着任务、目录或文件名自动走。
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
    pub(super) sidebar_created: Option<std::time::Instant>,
    pub(super) sidebar_selected: Option<std::time::Instant>,
    pub(super) sidebar_divider: bool,
}

/// 两种 tab 布局共用的只读展示数据。状态与动作仍由 `NebulaWorkspace`
/// 持有；这里只集中 cwd 标题、程序图标、AI 活动和用户元数据的解释。
pub(super) struct TabPresentation {
    pub(super) title: SharedString,
    pub(super) tooltip: Option<SharedString>,
    pub(super) activity: SidebarActivity,
    pub(super) icon: TabIcon,
    pub(super) shell_tag: Option<SharedString>,
    pub(super) color: Option<Rgb>,
    pub(super) renaming: Option<Entity<InputState>>,
    /// 本 tab 的分屏数（Terminal tab 才 > 0）。`> 1` 时行尾显示布局缩略图；
    /// 见 [`pane_header::split_layout_icon`]。
    pub(super) pane_count: usize,
    pub(super) local_administrator: bool,
}

pub(super) enum TabIcon {
    Symbol(IconName),
    Glyph(SharedString),
    Agent(Arc<RenderImage>),
    Shell(&'static [u8]),
    Split,
}

impl TabIcon {
    fn terminal() -> Self {
        Self::Symbol(IconName::SquareTerminal)
    }

    pub(super) fn leading_slot(
        self,
        id: SharedString,
        ink: gpui::Hsla,
        hover: Option<(SharedString, gpui::Hsla)>,
    ) -> impl IntoElement {
        TabIconSlot { icon: self, id, ink, hover }
    }
}

#[derive(gpui::IntoElement)]
struct TabIconSlot {
    icon: TabIcon,
    id: SharedString,
    ink: gpui::Hsla,
    hover: Option<(SharedString, gpui::Hsla)>,
}

impl gpui::RenderOnce for TabIconSlot {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Self { icon, id, ink, hover } = self;
        let size = px(TAB_LABEL_ICON_SIZE);
        let symbol = |name| Icon::new(name).size(size).text_color(ink).into_any_element();
        let content = match icon {
            TabIcon::Symbol(name) => symbol(name),
            TabIcon::Glyph(glyph) => div()
                .font_family(crate::font_install::SYMBOL_FONT_FAMILY)
                .text_size(size)
                .line_height(size)
                .text_color(ink)
                .when_some(hover.clone(), |glyph, (group, color)| {
                    glyph.group_hover(group, |style| style.text_color(color))
                })
                .child(crate::gpui_shell::widgets::terminal_glyph(glyph, TAB_LABEL_ICON_SIZE))
                .into_any_element(),
            TabIcon::Agent(image) => div()
                .relative()
                .size(size)
                .child(img(image.clone()).size_full().when_some(
                    hover.as_ref(),
                    |image, (group, _)| {
                        image
                            .grayscale(true)
                            .opacity(0.85)
                            .group_hover(group.clone(), |style| style.invisible())
                    },
                ))
                .when_some(hover, |slot, (group, _)| {
                    slot.child(
                        img(image)
                            .absolute()
                            .inset_0()
                            .size_full()
                            .invisible()
                            .group_hover(group, |style| style.visible()),
                    )
                })
                .into_any_element(),
            TabIcon::Shell(artwork) => {
                let physical_size =
                    (TAB_LABEL_ICON_SIZE * window.scale_factor()).round().max(1.0) as u32;
                window
                    .use_asset::<ShellTabTexture>(&ShellTextureSource(artwork, physical_size), cx)
                    .flatten()
                    .map_or_else(
                        || symbol(IconName::SquareTerminal),
                        |image| img(image).size(size).into_any_element(),
                    )
            },
            TabIcon::Split => {
                pane_header::split_glyph(TAB_LABEL_ICON_SIZE * 0.78, ink).into_any_element()
            },
        };
        div()
            .debug_selector(move || id.to_string())
            .w(px(TAB_LABEL_ICON_W))
            .h(size)
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .child(content)
    }
}

#[derive(Clone)]
struct ShellTextureSource(&'static [u8], u32);

impl std::hash::Hash for ShellTextureSource {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Embedded artwork is immutable; avoid hashing its compressed bytes on every frame.
        state.write_usize(self.0.as_ptr() as usize);
        state.write_usize(self.0.len());
        state.write_u32(self.1);
    }
}

struct ShellTabTexture;

impl gpui::Asset for ShellTabTexture {
    type Source = ShellTextureSource;
    type Output = Option<Arc<RenderImage>>;

    fn load(
        source: Self::Source,
        _: &mut App,
    ) -> impl std::future::Future<Output = Self::Output> + Send + 'static {
        async move { crate::gpui_shell::widgets::shell_brand_texture(source.0, source.1) }
    }
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
                // 标签跟随聚焦 pane 的原生 AI 任务名，普通 shell 回退到目录或主机。
                // 分屏计数**不拼在这里**：这份字符串还要喂给 runtime
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
        let tab = &self.tabs[ix];
        let view = tab.focused_view().map(|entity| entity.read(cx));
        let local_administrator = crate::platform::elevation::is_elevated().unwrap_or(false)
            && view.is_some_and(TerminalView::inherits_windows_host_token);
        let pane_count = match tab {
            WorkspaceTab::Terminal { panes, .. } => panes.len(),
            _ => 0,
        };
        // 事件 vs 状态的唯一裁定处（侧栏与顶栏共用这份 presentation），规则与
        // 理由见 [`sidebar::resting_activity`]。
        let meta = self.meta(ix);
        let activity = sidebar::resting_activity(
            view.map_or(SidebarActivity::Idle, TerminalView::sidebar_activity),
            active,
            meta.has_bell,
        );
        let icon = match tab {
            WorkspaceTab::Settings { .. } => TabIcon::Symbol(IconName::Settings),
            WorkspaceTab::Document { .. } => TabIcon::Glyph("\u{eb1d}".into()),
            WorkspaceTab::Code { view, .. } => TabIcon::Glyph(
                crate::display::side_panel::file_type_icon(&view.read(cx).title).into(),
            ),
            WorkspaceTab::Image { view } => TabIcon::Glyph(
                crate::display::side_panel::file_type_icon(&view.read(cx).title).into(),
            ),
            WorkspaceTab::Terminal { .. } => self.terminal_tab_icon(view, pane_count, dark),
        };
        // 分屏 tab 不画 shell 短标：一个 tab 里的 N 个 pane 完全可能跑着不同
        // 的 shell，只贴其中一个（聚焦那个）是误导；数量胶囊「这是一组」才是
        // 此时该占这个槽位的信息。顺带把 28px 让回标题——顶栏挤到 120px 时，
        // 图标+胶囊+短标三样一起上，标题只剩两三个字符。
        let shell_tag = (tab.is_terminal() && activity == SidebarActivity::Idle && pane_count <= 1)
            .then_some(meta.shell_tag)
            .flatten()
            .filter(|tag| !tag.is_empty());
        let renaming = self
            .tab_rename
            .as_ref()
            .filter(|rename| rename.ix == ix)
            .map(|rename| rename.input.clone());
        let tooltip = view.map(|view| view.tab_tooltip(&title).into());
        TabPresentation {
            title,
            tooltip,
            activity,
            icon,
            shell_tag,
            color: meta.color,
            renaming,
            pane_count,
            local_administrator,
        }
    }

    fn terminal_tab_icon(
        &self,
        view: Option<&TerminalView>,
        pane_count: usize,
        dark: bool,
    ) -> TabIcon {
        if let Some(view) = view {
            let program = view
                .running_program
                .as_deref()
                .or_else(|| view.ai_session.as_ref().map(|identity| identity.source.as_str()));
            if let Some(image) = program
                .and_then(crate::display::ai_logo_for_program)
                .and_then(|logo| self.sidebar_logo_images.get(&(logo, dark)))
            {
                return TabIcon::Agent(image.clone());
            }
            if let Some(glyph) = view.ssh_host_glyph() {
                let generic = crate::display::ui::os_icons::resolve(None).glyph;
                return if glyph.chars().eq([generic]) {
                    TabIcon::Symbol(IconName::Network)
                } else {
                    TabIcon::Glyph(glyph)
                };
            }
            if let Some(glyph) = program
                .filter(|name| !matches!(*name, "ssh" | "mosh"))
                .and_then(crate::display::recognized_program_icon)
            {
                return TabIcon::Glyph(glyph.into());
            }
        }
        if pane_count > 1 {
            return TabIcon::Split;
        }
        if view.is_some_and(|view| {
            matches!(view.session_launch, crate::session::LaunchSession::Serial { .. })
        }) {
            return TabIcon::Symbol(IconName::Network);
        }
        view.and_then(|view| view.shell_artwork).map_or_else(TabIcon::terminal, TabIcon::Shell)
    }
}

pub(super) fn tooltip(text: SharedString, window: &mut Window, cx: &mut App) -> gpui::AnyView {
    gpui_component::tooltip::Tooltip::element(move |_, _| {
        div().max_w(px(560.0)).whitespace_normal().child(text.clone())
    })
    .build(window, cx)
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
