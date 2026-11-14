//! A visual grouping of the existing tabs, shared by rendering and row geometry.

use super::tab_menu_surface::TabMenuSurface;
use super::*;
use crate::i18n::{Message, UiLanguage};
use gpui_component::{ElementExt as _, button::ButtonCustomVariant, menu::PopupMenuItem};
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    ops::Range,
    rc::Rc,
    time::Instant,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum SidebarGrouping {
    #[default]
    None,
    Project,
    Shell,
}

impl SidebarGrouping {
    fn message(self) -> Message {
        match self {
            Self::None => Message::ChromeTabGroupingNone,
            Self::Project => Message::ChromeTabGroupingProject,
            Self::Shell => Message::ChromeTabGroupingShell,
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum SidebarTabOrder {
    #[default]
    Created,
    Recent,
    Manual,
}

impl SidebarTabOrder {
    fn message(self) -> Message {
        match self {
            Self::Created => Message::ChromeTabGroupingOrderCreated,
            Self::Recent => Message::ChromeTabGroupingOrderRecent,
            Self::Manual => Message::ChromeTabGroupingOrderManual,
        }
    }

    fn sort(self, indices: &mut [usize], meta: &[TabMeta]) {
        match self {
            Self::Created => indices.sort_by_key(|&ix| meta[ix].sidebar_created),
            Self::Recent => indices.sort_by_key(|&ix| {
                let item = &meta[ix];
                (std::cmp::Reverse(item.sidebar_selected), item.sidebar_created)
            }),
            Self::Manual => {},
        }
    }
}

struct GroupMenu {
    panel: Entity<TabMenuSurface>,
    _dismiss: gpui::Subscription,
}

#[derive(Default)]
pub(super) struct SidebarTabLayout {
    grouping: SidebarGrouping,
    pub(super) order: SidebarTabOrder,
    collapsed: BTreeSet<TabGroup>,
    last_selected: Option<crate::runtime_api::tabs::TabId>,
    menu: Option<GroupMenu>,
    trigger_bounds: Rc<Cell<Bounds<Pixels>>>,
}

#[derive(Clone, Copy)]
enum MenuCommand {
    Group(SidebarGrouping),
    Order(SidebarTabOrder),
    Divider,
    GroupsVisibility,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum TabGroup {
    Directory { path: String, distro: Option<String> },
    Host(String),
    Shell(String),
    Agents,
    Terminal,
    Files,
    Settings,
}

impl TabGroup {
    pub(super) fn label(&self, language: UiLanguage) -> SharedString {
        match self {
            Self::Directory { path, distro } => {
                let name = path.rsplit(['/', '\\']).find(|part| !part.is_empty()).unwrap_or(path);
                match distro {
                    Some(distro) => format!("{distro} · {name}").into(),
                    None => name.to_owned().into(),
                }
            },
            Self::Host(name) | Self::Shell(name) => name.clone().into(),
            Self::Agents => language.text(Message::ChromeTabGroupingAgents).into(),
            Self::Terminal => language.text(Message::ChromeTabGroupingTerminal).into(),
            Self::Files => language.text(Message::ChromeTabGroupingFiles).into(),
            Self::Settings => language.text(Message::CommonSettings).into(),
        }
    }

    pub(super) fn directory_path(&self) -> Option<SharedString> {
        match self {
            Self::Directory { path, distro: Some(distro) } => {
                Some(format!("{distro} · {path}").into())
            },
            Self::Directory { path, distro: None } => Some(path.clone().into()),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum SidebarTabRow {
    Group { group: TabGroup, count: usize, collapsed: bool },
    Divider,
    Tab(usize),
}

pub(super) enum SidebarTabRows {
    Flat(usize),
    Projected { rows: Vec<SidebarTabRow>, hidden: BTreeMap<usize, usize> },
}

impl SidebarTabRows {
    fn grouped(
        groups: impl IntoIterator<Item = (usize, TabGroup)>,
        collapsed: &BTreeSet<TabGroup>,
    ) -> Self {
        let mut members = BTreeMap::<_, (usize, Vec<_>)>::new();
        for (order, (ix, group)) in groups.into_iter().enumerate() {
            members.entry(group).or_insert_with(|| (order, Vec::new())).1.push(ix);
        }
        let mut members = members.into_iter().collect::<Vec<_>>();
        members.sort_unstable_by_key(|(_, (order, _))| *order);
        let mut rows = Vec::new();
        let mut hidden = BTreeMap::new();
        for (group, (_, tabs)) in members {
            let closed = collapsed.contains(&group);
            let heading = rows.len();
            rows.push(SidebarTabRow::Group { group, count: tabs.len(), collapsed: closed });
            if closed {
                hidden.extend(tabs.into_iter().map(|tab| (tab, heading)));
            } else {
                rows.extend(tabs.into_iter().map(SidebarTabRow::Tab));
            }
        }
        Self::Projected { rows, hidden }
    }

    pub(super) fn len(&self) -> usize {
        match self {
            Self::Flat(len) => *len,
            Self::Projected { rows, .. } => rows.len(),
        }
    }

    pub(super) fn row(&self, slot: usize) -> Option<SidebarTabRow> {
        match self {
            Self::Flat(len) => (slot < *len).then_some(SidebarTabRow::Tab(slot)),
            Self::Projected { rows, .. } => rows.get(slot).cloned(),
        }
    }

    pub(super) fn index_of(&self, tab: usize) -> Option<usize> {
        match self {
            Self::Flat(len) => (tab < *len).then_some(tab),
            Self::Projected { rows, .. } => {
                rows.iter().position(|row| *row == SidebarTabRow::Tab(tab))
            },
        }
    }

    pub(super) fn reveal_index_of(&self, tab: usize) -> Option<usize> {
        self.index_of(tab).or_else(|| match self {
            Self::Projected { hidden, .. } => hidden.get(&tab).copied(),
            Self::Flat(_) => None,
        })
    }

    /// A grouped drag can reorder members, but cannot change what directory or shell they use.
    pub(super) fn group_span(&self, tab: usize) -> Option<(usize, Range<usize>)> {
        let Self::Projected { rows, .. } = self else { return None };
        let slot = self.index_of(tab)?;
        let is_heading = |row: &SidebarTabRow| matches!(row, SidebarTabRow::Group { .. });
        let start = rows[..slot].iter().rposition(is_heading).map_or(0, |index| index + 1);
        let end = rows[slot..].iter().position(is_heading).map_or(rows.len(), |n| slot + n);
        Some((slot, start..end))
    }
}

impl NebulaWorkspace {
    fn sidebar_tab_group(&self, ix: usize, cx: &App) -> TabGroup {
        let tab = &self.tabs[ix];
        if tab.is_settings() {
            return TabGroup::Settings;
        }
        let Some(terminal) = tab.focused_view() else { return TabGroup::Files };
        let terminal = terminal.read(cx);
        if self.sidebar_tab_layout.grouping == SidebarGrouping::Project {
            if let Some(host) = terminal.ssh_destination.as_ref() {
                return TabGroup::Host(host.clone());
            }
            let cwd = terminal.cwd.trim();
            if !cwd.is_empty() && !cwd.chars().any(char::is_control) {
                let path = cwd.trim_end_matches(['/', '\\']);
                return TabGroup::Directory {
                    path: if path.is_empty() { cwd } else { path }.to_owned(),
                    distro: terminal.wsl_distro().map(str::to_owned),
                };
            }
        } else {
            let agent = terminal
                .ai_session
                .as_ref()
                .map(|session| session.source.as_str())
                .or(terminal.running_program.as_deref())
                .and_then(crate::ai_agents::AgentKind::parse);
            if agent.is_some() {
                return TabGroup::Agents;
            }
            if let Some(shell) = self.tab_meta[ix].shell_tag.as_deref()
                && !shell.is_empty()
            {
                return TabGroup::Shell(shell.to_uppercase());
            }
        }
        TabGroup::Terminal
    }

    fn sidebar_order_matches_storage(&self) -> bool {
        match self.sidebar_tab_layout.order {
            SidebarTabOrder::Manual => true,
            SidebarTabOrder::Created => self.created_order_is_storage_order(),
            SidebarTabOrder::Recent => false,
        }
    }

    fn created_order_is_storage_order(&self) -> bool {
        self.tab_meta.windows(2).all(|pair| pair[0].sidebar_created <= pair[1].sidebar_created)
    }

    fn sidebar_tab_indices(&self) -> Vec<usize> {
        let mut indices: Vec<_> = (0..self.tabs.len()).collect();
        self.sidebar_tab_layout.order.sort(&mut indices, &self.tab_meta);
        indices
    }

    pub(super) fn sidebar_tab_rows(&self, cx: &App) -> SidebarTabRows {
        let layout = &self.sidebar_tab_layout;
        if layout.grouping == SidebarGrouping::None
            && self.sidebar_order_matches_storage()
            && self.tab_meta.iter().all(|meta| !meta.sidebar_divider)
        {
            return SidebarTabRows::Flat(self.tabs.len());
        }
        let indices = self.sidebar_tab_indices();
        if layout.grouping != SidebarGrouping::None {
            return SidebarTabRows::grouped(
                indices.into_iter().map(|ix| (ix, self.sidebar_tab_group(ix, cx))),
                &layout.collapsed,
            );
        }
        let rows = indices
            .into_iter()
            .flat_map(|ix| {
                std::iter::once(SidebarTabRow::Tab(ix))
                    .chain(self.tab_meta[ix].sidebar_divider.then_some(SidebarTabRow::Divider))
            })
            .collect();
        SidebarTabRows::Projected { rows, hidden: BTreeMap::new() }
    }

    pub(super) fn reorder_sidebar_tabs(
        &mut self,
        source: usize,
        target: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if source == target {
            return;
        }
        let mut order = self.sidebar_tab_indices();
        if self.sidebar_tab_layout.grouping == SidebarGrouping::None {
            let Some(from) = order.iter().position(|&ix| ix == source) else { return };
            let Some(to) = order.iter().position(|&ix| ix == target) else { return };
            order.remove(from);
            order.insert(to, source);
        } else {
            // Reorder only the slots occupied by this group. A drag inside a
            // group should not move its heading past a neighboring group.
            let group = self.sidebar_tab_group(source, cx);
            let mut members: Vec<_> =
                order.iter_mut().filter(|ix| self.sidebar_tab_group(**ix, cx) == group).collect();
            let Some(from) = members.iter().position(|ix| **ix == source) else { return };
            let Some(to) = members.iter().position(|ix| **ix == target) else { return };
            if from < to {
                for i in from..to {
                    *members[i] = *members[i + 1];
                }
            } else {
                for i in (to + 1..=from).rev() {
                    *members[i] = *members[i - 1];
                }
            }
            *members[to] = source;
        }
        self.sidebar_tab_layout.order = SidebarTabOrder::Manual;
        self.reorder_tabs(order, window, cx);
    }

    pub(super) fn mark_sidebar_tab_selected(&mut self) {
        let Some(meta) = self.tab_meta.get_mut(self.active) else { return };
        let last = &mut self.sidebar_tab_layout.last_selected;
        if last.as_ref() != Some(&meta.runtime_id) {
            *last = Some(meta.runtime_id.clone());
            meta.sidebar_selected = Some(Instant::now());
        }
    }

    pub(super) fn toggle_sidebar_tab_group(
        &mut self,
        group: TabGroup,
        all: bool,
        cx: &mut Context<Self>,
    ) {
        let was_collapsed = self.sidebar_tab_layout.collapsed.contains(&group);
        if all {
            self.sidebar_tab_layout.collapsed =
                if was_collapsed { BTreeSet::new() } else { self.sidebar_groups(cx) };
        } else if was_collapsed {
            self.sidebar_tab_layout.collapsed.remove(&group);
        } else {
            self.sidebar_tab_layout.collapsed.insert(group);
        }
        self.tabs_scroll_grab = None;
        cx.notify();
    }

    fn sidebar_groups(&self, cx: &App) -> BTreeSet<TabGroup> {
        if self.sidebar_tab_layout.grouping == SidebarGrouping::None {
            return BTreeSet::new();
        }
        (0..self.tabs.len()).map(|ix| self.sidebar_tab_group(ix, cx)).collect()
    }

    fn apply_sidebar_menu_command(&mut self, command: MenuCommand, cx: &mut Context<Self>) {
        match command {
            MenuCommand::Group(grouping) => {
                if self.sidebar_tab_layout.grouping != grouping {
                    self.sidebar_tab_layout.collapsed.clear();
                    self.sidebar_tab_layout.grouping = grouping;
                }
                self.tabs_scroll = 0;
            },
            MenuCommand::Order(order) => {
                self.sidebar_tab_layout.order = order;
                self.tabs_scroll = 0;
            },
            MenuCommand::Divider if self.sidebar_tab_layout.grouping == SidebarGrouping::None => {
                if let Some(meta) = self.tab_meta.get_mut(self.active) {
                    meta.sidebar_divider = !meta.sidebar_divider;
                }
            },
            MenuCommand::GroupsVisibility => {
                let groups = self.sidebar_groups(cx);
                let collapsed = &mut self.sidebar_tab_layout.collapsed;
                if groups.is_subset(collapsed) {
                    collapsed.clear();
                } else {
                    *collapsed = groups;
                }
            },
            MenuCommand::Divider => {},
        }
        self.tabs_scroll_grab = None;
        self.reveal_active_tab(cx);
        cx.notify();
    }

    pub(super) fn sidebar_group_control(
        &self,
        header_group: SharedString,
        cx: &mut Context<'_, Self>,
    ) -> gpui::AnyElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        let label = language.text(Message::ChromeTabGroupingLabel);
        let grouping = self.sidebar_tab_layout.grouping;
        let open = self.sidebar_tab_layout.menu.is_some();
        let trigger_bounds = self.sidebar_tab_layout.trigger_bounds.clone();
        let palette = cx.theme();
        let ink = palette.foreground;
        let chrome = crate::gpui_shell::theme::tab_bar_background(cx);
        let hover_group: SharedString = "sidebar-filter-hover".into();
        let icon = gpui::svg()
            .path(crate::gpui_shell::assets::nav::TAB_GROUPING)
            .debug_selector(|| "sidebar-tab-grouping-icon".into())
            .size(px(SIDEBAR_HEADER_ICON))
            .text_color(palette.muted_foreground)
            .group_hover(hover_group.clone(), |style| style.text_color(ink));
        let trigger = Button::new("sidebar-tab-grouping")
            .debug_selector(|| "sidebar-tab-grouping".into())
            .custom(
                ButtonCustomVariant::new(cx)
                    .foreground(palette.muted_foreground)
                    .hover(palette.list_hover)
                    .active(ink.opacity(0.13)),
            )
            .xsmall()
            .rounded(px(8.0))
            .relative()
            .size(px(SIDEBAR_HEADER_ACTION))
            .px_0()
            .cursor_pointer()
            .aria_label(label)
            .tooltip(label)
            .invisible()
            .group_hover(header_group, |style| style.visible())
            .focus_visible(|style| style.visible())
            .selected(open)
            .when(open, |button| button.bg(palette.list_hover))
            .on_click(cx.listener(|this, _, window, cx| {
                cx.stop_propagation();
                this.toggle_sidebar_group_menu(window, cx);
            }))
            .child(icon)
            .when(grouping != SidebarGrouping::None, |button| {
                button.child(
                    div()
                        .absolute()
                        .top(px(4.0))
                        .right(px(4.0))
                        .size(px(10.0))
                        .rounded_full()
                        .border_2()
                        .border_color(chrome)
                        .bg(palette.primary),
                )
            });
        div()
            .id("sidebar-filter-hit-area")
            .size(px(SIDEBAR_HEADER_ACTION))
            .flex_shrink_0()
            .group(hover_group)
            .on_prepaint(move |bounds, _, _| trigger_bounds.set(bounds))
            .child(trigger)
            .into_any_element()
    }

    fn toggle_sidebar_group_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(open) = self.sidebar_tab_layout.menu.as_ref() {
            open.panel.update(cx, |panel, cx| panel.dismiss(window, cx));
            return;
        }
        self.tab_menu = None;
        let language = crate::gpui_shell::config::ui_language(cx);
        let layout = &self.sidebar_tab_layout;
        let grouping = layout.grouping;
        let order = layout.order;
        let trigger_bounds = layout.trigger_bounds.clone();
        let groups = self.sidebar_groups(cx);
        let all_collapsed = !groups.is_empty() && groups.is_subset(&layout.collapsed);
        let divider = self.tab_meta.get(self.active).is_some_and(|meta| meta.sidebar_divider);
        let divider_allowed = grouping == SidebarGrouping::None && !self.tabs.is_empty();
        let workspace = cx.weak_entity();
        let command = |message, command: MenuCommand| {
            let workspace = workspace.clone();
            PopupMenuItem::new(language.text(message)).on_click(move |_, _, cx| {
                let _ =
                    workspace.update(cx, |this, cx| this.apply_sidebar_menu_command(command, cx));
            })
        };
        let width = px(236.0);
        let panel = TabMenuSurface::build(cx, width, |mut menu| {
            menu = menu
                .grouping(trigger_bounds)
                .item(PopupMenuItem::label(language.text(Message::ChromeTabGroupingSection)));
            for choice in [SidebarGrouping::None, SidebarGrouping::Project, SidebarGrouping::Shell]
            {
                menu = menu.choice(
                    command(choice.message(), MenuCommand::Group(choice)),
                    grouping == choice,
                    None,
                );
            }
            menu = menu
                .separator()
                .item(PopupMenuItem::label(language.text(Message::ChromeTabGroupingOrderSection)));
            for choice in
                [SidebarTabOrder::Created, SidebarTabOrder::Recent, SidebarTabOrder::Manual]
            {
                menu = menu.choice(
                    command(choice.message(), MenuCommand::Order(choice)),
                    order == choice,
                    (choice == SidebarTabOrder::Created)
                        .then(|| language.text(Message::ChromeTabGroupingOrderDefault)),
                );
            }
            menu.separator()
                .with_hint(
                    command(
                        if divider {
                            Message::ChromeTabGroupingDividerRemove
                        } else {
                            Message::ChromeTabGroupingDividerInsert
                        },
                        MenuCommand::Divider,
                    )
                    .disabled(!divider_allowed),
                    language.text(if grouping == SidebarGrouping::None {
                        Message::ChromeTabGroupingDividerBelow
                    } else {
                        Message::ChromeTabGroupingDividerUngrouped
                    }),
                )
                .with_hint(
                    command(
                        if all_collapsed {
                            Message::ChromeTabGroupingExpandAll
                        } else {
                            Message::ChromeTabGroupingCollapseAll
                        },
                        MenuCommand::GroupsVisibility,
                    )
                    .disabled(groups.is_empty()),
                    language.text(Message::ChromeTabGroupingGroupModifier),
                )
        });
        let previous_focus = window.focused(cx);
        panel.update(cx, |panel, _| panel.remember_focus(previous_focus));
        let dismiss = cx.subscribe(&panel, |this, _, _: &gpui::DismissEvent, cx| {
            this.sidebar_tab_layout.menu = None;
            cx.notify();
        });
        panel.focus_handle(cx).focus(window, cx);
        self.sidebar_tab_layout.menu = Some(GroupMenu { panel, _dismiss: dismiss });
        cx.notify();
    }

    fn sidebar_group_menu_visible(&self, cx: &App) -> bool {
        !self.sidebar_collapsed
            && !self.settings_open
            && !self.reader_focus_active(cx)
            && self.tabs_position == nebula_settings::TabsPositionName::Sidebar
    }

    pub(super) fn render_sidebar_group_menu(&self, cx: &App) -> Option<gpui::Deferred> {
        if !self.sidebar_group_menu_visible(cx) {
            return None;
        }
        let open = self.sidebar_tab_layout.menu.as_ref()?;
        Some(
            gpui::deferred(
                gpui::anchored()
                    .anchor(gpui::Anchor::TopLeft)
                    .position(self.tabs_list_origin)
                    .snap_to_window_with_margin(px(8.0))
                    .child(open.panel.clone()),
            )
            .with_priority(2),
        )
    }

    pub(super) fn dismiss_hidden_sidebar_group_menu(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.sidebar_group_menu_visible(cx) {
            if let Some(open) = self.sidebar_tab_layout.menu.take() {
                open.panel.update(cx, |panel, cx| panel.dismiss(window, cx));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grouped_rows_keep_tab_indices_and_allow_drag_only_within_the_group() {
        let shell = |name: &str| TabGroup::Shell(name.to_owned());
        let rows = SidebarTabRows::grouped(
            [shell("BASH"), shell("ZSH"), shell("BASH")].into_iter().enumerate(),
            &BTreeSet::new(),
        );
        assert_eq!(rows.len(), 5);
        assert_eq!(
            rows.row(0),
            Some(SidebarTabRow::Group { group: shell("BASH"), count: 2, collapsed: false })
        );
        assert_eq!(rows.row(1), Some(SidebarTabRow::Tab(0)));
        assert_eq!(rows.row(2), Some(SidebarTabRow::Tab(2)));
        assert_eq!(rows.index_of(1), Some(4));
        assert_eq!(rows.group_span(2), Some((2, 1..3)));

        let drag = TabDrag {
            source: 0,
            cross_window: None,
            cross_window_target: None,
            press_x: 0.0,
            press_y: 0.0,
            axis: TabDragAxis::Vertical,
            pitch: 32.0,
            offset: 320.0,
            active: true,
            force_tear_out: false,
            dock: None,
        };
        assert_eq!(NebulaWorkspace::sidebar_drag_slot(&drag, &rows), 2);
        assert_eq!(NebulaWorkspace::sidebar_drag_slot(&drag, &SidebarTabRows::Flat(4)), 3);
    }

    #[test]
    fn recently_used_order_preserves_tab_identity_and_collapsed_rows_reveal_the_group() {
        let now = Instant::now();
        let ago = now - Duration::from_secs(1);
        let tabs = [
            TabMeta { sidebar_created: Some(ago), ..Default::default() },
            TabMeta {
                sidebar_created: Some(now),
                sidebar_selected: Some(now),
                ..Default::default()
            },
            TabMeta {
                sidebar_created: Some(now),
                sidebar_selected: Some(ago),
                ..Default::default()
            },
        ];
        let mut order = [0, 1, 2];
        SidebarTabOrder::Recent.sort(&mut order, &tabs);
        assert_eq!(order, [1, 2, 0]);
        let shell = |name: &str| TabGroup::Shell(name.to_owned());
        let rows = SidebarTabRows::grouped(
            order.into_iter().map(|ix| (ix, shell(if ix == 0 { "BASH" } else { "ZSH" }))),
            &[shell("ZSH")].into_iter().collect(),
        );
        assert_eq!(rows.len(), 3);
        assert_eq!(rows.reveal_index_of(1), Some(0));
        assert_eq!(rows.reveal_index_of(2), Some(0));
        assert_eq!(rows.index_of(2), None);
        assert_eq!(rows.index_of(0), Some(2));
        SidebarTabOrder::Created.sort(&mut order, &tabs);
        assert_eq!(order, [0, 1, 2]);
    }

    #[test]
    fn equal_directory_names_do_not_merge_different_projects_or_wsl_guests() {
        let rows = SidebarTabRows::grouped(
            [
                TabGroup::Directory { path: "/one/app".to_owned(), distro: None },
                TabGroup::Directory { path: "/two/app".to_owned(), distro: None },
                TabGroup::Directory {
                    path: "/one/app".to_owned(),
                    distro: Some("Ubuntu".to_owned()),
                },
            ]
            .into_iter()
            .enumerate(),
            &BTreeSet::new(),
        );
        assert_eq!(rows.len(), 6);
        let (_, group) = rows.group_span(0).unwrap();
        assert!(!group.contains(&rows.index_of(1).unwrap()));
        assert!(!group.contains(&rows.index_of(2).unwrap()));
    }
}
