//! Shared tab names, task details and identity for sidebar and top tabs.

use super::*;

/// 两种 tab 布局共用的只读展示数据。状态与动作仍由 `NebulaWorkspace`
/// 持有；这里只集中 cwd 标题、程序图标、AI 活动和用户元数据的解释。
pub(super) struct TabPresentation {
    pub(super) title: SharedString,
    pub(super) tooltip: Option<SharedString>,
    pub(super) is_settings: bool,
    pub(super) activity: SidebarActivity,
    pub(super) logo_image: Option<Arc<RenderImage>>,
    pub(super) program_glyph: Option<&'static str>,
    pub(super) shell_tag: Option<SharedString>,
    pub(super) color: Option<Rgb>,
    pub(super) renaming: Option<Entity<InputState>>,
    /// 本 tab 的分屏数（Terminal tab 才 > 0）。`> 1` 时行首图标换成 2×2 分屏
    /// 标记、行尾挂一枚数量胶囊；见 [`pane_header::split_badge`]。
    pub(super) pane_count: usize,
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
            WorkspaceTab::Settings { .. } => "设置".into(),
            WorkspaceTab::Image { view } => view.read(cx).title.clone().into(),
            WorkspaceTab::Document { view, .. } => view.read(cx).tab_title().into(),
            WorkspaceTab::Code { view, .. } => view.read(cx).tab_title(cx).into(),
            tab @ WorkspaceTab::Terminal { .. } => {
                // 标签 = 聚焦 pane 的 cwd 末级目录名（旧壳 chrome_tab_label
                // 规则）。分屏计数**不拼在这里**：这份字符串还要喂给 runtime
                // API 的 tab label、跨窗拖拽标题和重命名预填，掺进 "⊞2" 会
                // 一路泄漏，而且长标题下会被 truncate_tab_label 截掉、被
                // custom_name 整条顶掉。计数改由 TabPresentation::pane_count
                // 单独画成胶囊，见 sidebar/top_tabs 的渲染。
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
        // 事件 vs 状态的唯一裁定处（侧栏与顶栏共用这份 presentation），规则与
        // 理由见 [`sidebar::resting_activity`]。
        let activity = sidebar::resting_activity(activity, active, self.meta(ix).has_bell);
        let logo_image = program
            .as_deref()
            .and_then(crate::display::ai_logo_for_program)
            .and_then(|logo| self.sidebar_logo_images.get(&(logo, dark)).cloned());
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
            tooltip,
            is_settings,
            activity,
            logo_image,
            program_glyph,
            shell_tag,
            color: meta.color,
            renaming,
            pane_count,
        }
    }
}

pub(super) fn tooltip(text: SharedString, window: &mut Window, cx: &mut App) -> gpui::AnyView {
    gpui_component::tooltip::Tooltip::element(move |_, _| {
        div().max_w(px(560.0)).whitespace_normal().child(text.clone())
    })
    .build(window, cx)
}
