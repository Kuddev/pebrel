//! A visual grouping of the existing tabs, shared by rendering and row geometry.

use super::*;
use crate::i18n::{Message, UiLanguage};
use gpui_component::menu::PopupMenuItem;
use std::{collections::BTreeMap, ops::Range};

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
    Group(TabGroup),
    Tab(usize),
}

pub(super) enum SidebarTabRows {
    Flat(usize),
    Grouped(Vec<SidebarTabRow>),
}

impl SidebarTabRows {
    fn grouped(groups: impl IntoIterator<Item = TabGroup>) -> Self {
        let mut members = BTreeMap::<_, Vec<_>>::new();
        for (ix, group) in groups.into_iter().enumerate() {
            members.entry(group).or_default().push(ix);
        }
        Self::Grouped(
            members
                .into_iter()
                .flat_map(|(group, tabs)| {
                    std::iter::once(SidebarTabRow::Group(group))
                        .chain(tabs.into_iter().map(SidebarTabRow::Tab))
                })
                .collect(),
        )
    }

    pub(super) fn len(&self) -> usize {
        match self {
            Self::Flat(len) => *len,
            Self::Grouped(rows) => rows.len(),
        }
    }

    pub(super) fn row(&self, slot: usize) -> Option<SidebarTabRow> {
        match self {
            Self::Flat(len) => (slot < *len).then_some(SidebarTabRow::Tab(slot)),
            Self::Grouped(rows) => rows.get(slot).cloned(),
        }
    }

    pub(super) fn index_of(&self, tab: usize) -> Option<usize> {
        match self {
            Self::Flat(len) => (tab < *len).then_some(tab),
            Self::Grouped(rows) => rows.iter().position(|row| *row == SidebarTabRow::Tab(tab)),
        }
    }

    /// A grouped drag can reorder members, but cannot change what directory or shell they use.
    pub(super) fn group_span(&self, tab: usize) -> Option<(usize, Range<usize>)> {
        let Self::Grouped(rows) = self else { return None };
        let slot = self.index_of(tab)?;
        let is_heading = |row: &SidebarTabRow| matches!(row, SidebarTabRow::Group(_));
        let start = rows[..slot].iter().rposition(is_heading).map_or(0, |index| index + 1);
        let end = rows[slot..].iter().position(is_heading).map_or(rows.len(), |n| slot + n);
        Some((slot, start..end))
    }
}

impl NebulaWorkspace {
    pub(super) fn sidebar_tab_rows(&self, cx: &App) -> SidebarTabRows {
        if self.sidebar_grouping == SidebarGrouping::None {
            return SidebarTabRows::Flat(self.tabs.len());
        }
        SidebarTabRows::grouped(self.tabs.iter().enumerate().map(|(ix, tab)| {
            if tab.is_settings() {
                return TabGroup::Settings;
            }
            let Some(terminal) = tab.focused_view() else { return TabGroup::Files };
            let terminal = terminal.read(cx);
            if self.sidebar_grouping == SidebarGrouping::Project {
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
                if let Some(shell) =
                    self.tab_meta.get(ix).and_then(|meta| meta.shell_tag.as_deref())
                    && !shell.is_empty()
                {
                    return TabGroup::Shell(shell.to_uppercase());
                }
            }
            TabGroup::Terminal
        }))
    }

    pub(super) fn sidebar_group_control(&self, cx: &Context<Self>) -> gpui::AnyElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        let label = language.text(Message::ChromeTabGroupingLabel);
        let grouping = self.sidebar_grouping;
        let workspace = cx.weak_entity();
        let icon = Icon::new(Icon::empty())
            .path(crate::gpui_shell::assets::nav::TAB_GROUPING)
            .with_size(px(SIDEBAR_HEADER_ICON));
        Button::new("sidebar-tab-grouping")
            .debug_selector(|| "sidebar-tab-grouping".into())
            .ghost()
            .xsmall()
            .relative()
            .size(px(SIDEBAR_HEADER_ACTION))
            .px_0()
            .cursor_pointer()
            .aria_label(label)
            .tooltip(label)
            .child(icon)
            .when(grouping != SidebarGrouping::None, |button| {
                button.child(
                    div()
                        .absolute()
                        .top(px(4.0))
                        .right(px(4.0))
                        .size(px(5.0))
                        .rounded_full()
                        .bg(cx.theme().primary),
                )
            })
            .dropdown_menu_with_anchor(gpui::Anchor::TopRight, move |menu, _, _| {
                [SidebarGrouping::None, SidebarGrouping::Project, SidebarGrouping::Shell]
                    .into_iter()
                    .fold(menu, |menu, choice| {
                        let workspace = workspace.clone();
                        menu.item(
                            PopupMenuItem::new(language.text(choice.message()))
                                .checked(grouping == choice)
                                .on_click(move |_, _, cx| {
                                    let _ = workspace.update(cx, |this, cx| {
                                        this.sidebar_grouping = choice;
                                        this.tab_menu = None;
                                        this.tabs_scroll = 0;
                                        this.tabs_scroll_grab = None;
                                        this.reveal_active_tab(cx);
                                        cx.notify();
                                    });
                                }),
                        )
                    })
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grouped_rows_keep_tab_indices_and_allow_drag_only_within_the_group() {
        let shell = |name: &str| TabGroup::Shell(name.to_owned());
        let rows = SidebarTabRows::grouped([shell("BASH"), shell("ZSH"), shell("BASH")]);
        assert_eq!(rows.len(), 5);
        assert_eq!(rows.row(0), Some(SidebarTabRow::Group(shell("BASH"))));
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
    fn equal_directory_names_do_not_merge_different_projects_or_wsl_guests() {
        let rows = SidebarTabRows::grouped([
            TabGroup::Directory { path: "/one/app".to_owned(), distro: None },
            TabGroup::Directory { path: "/two/app".to_owned(), distro: None },
            TabGroup::Directory { path: "/one/app".to_owned(), distro: Some("Ubuntu".to_owned()) },
        ]);
        assert_eq!(rows.len(), 6);
        let (_, group) = rows.group_span(0).unwrap();
        assert!(!group.contains(&rows.index_of(1).unwrap()));
        assert!(!group.contains(&rows.index_of(2).unwrap()));
    }
}
