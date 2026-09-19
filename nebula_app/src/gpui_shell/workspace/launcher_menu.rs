//! One context-menu host for the Shell/SSH launcher, with stable launch targets.

use super::*;
use crate::gpui_shell::toast::{ToastKind, toast};
use crate::i18n::Message;
use gpui::{Anchor, DismissEvent, Point, anchored, deferred};
use gpui_component::menu::PopupMenuItem;

pub(super) struct LauncherContextMenu {
    menu: Entity<PopupMenu>,
    position: Point<Pixels>,
    _subscription: Subscription,
}

#[derive(Clone)]
pub(super) enum LauncherTarget {
    Shell(crate::shell_detect::DetectedShell),
    Profile(crate::config::ui_config::Profile),
    Ssh(String),
}

impl LauncherTarget {
    pub(super) fn from_action(action: &WorkspacePaletteAction) -> Option<Self> {
        match action {
            WorkspacePaletteAction::LaunchShell(shell) => Some(Self::Shell(shell.clone())),
            WorkspacePaletteAction::LaunchProfile(profile) => Some(Self::Profile(profile.clone())),
            WorkspacePaletteAction::LaunchSshHost(host) => Some(Self::Ssh(host.clone())),
            _ => None,
        }
    }

    fn name(&self) -> &str {
        match self {
            Self::Shell(shell) => &shell.name,
            Self::Profile(profile) => &profile.name,
            Self::Ssh(host) => host,
        }
    }

    fn default_id(&self) -> Option<String> {
        match self {
            Self::Shell(shell) => Some(shell.id.clone()),
            Self::Profile(profile) => profile.settings_id(),
            Self::Ssh(_) => None,
        }
    }

    fn local_launch(&self) -> Option<crate::session::LaunchSession> {
        match self {
            Self::Shell(detected) => {
                let shell = detected.shell();
                Some(crate::session::LaunchSession::Shell {
                    name: detected.name.clone(),
                    program: shell.program().to_owned(),
                    args: shell.args().to_vec(),
                })
            },
            Self::Profile(profile) => {
                Some(super::shell_launch::profile_launch_session(profile.clone()))
            },
            Self::Ssh(_) => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LauncherCommand {
    OpenAdmin,
    SetDefault,
    Connect,
    Edit,
    Delete,
}

impl LauncherCommand {
    fn label(self) -> Message {
        match self {
            Self::OpenAdmin => Message::LauncherOpenAdmin,
            Self::SetDefault => Message::LauncherSetDefault,
            Self::Connect => Message::LauncherConnect,
            Self::Edit => Message::LauncherEdit,
            Self::Delete => Message::LauncherDelete,
        }
    }
}

fn commands(target: &LauncherTarget) -> Vec<LauncherCommand> {
    if matches!(target, LauncherTarget::Ssh(_)) {
        vec![LauncherCommand::Connect, LauncherCommand::Edit, LauncherCommand::Delete]
    } else {
        let mut commands = Vec::new();
        if crate::platform::elevation::SUPPORTED {
            commands.push(LauncherCommand::OpenAdmin);
        }
        commands.push(LauncherCommand::SetDefault);
        commands
    }
}

impl NebulaWorkspace {
    pub(super) fn open_launcher_context_menu(
        &mut self,
        target: LauncherTarget,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.shell_picker_open || !self.command_palette_open {
            return;
        }
        let current = crate::platform::shell::effective_shell_id(
            cx.try_global::<crate::gpui_shell::config::Settings>()
                .and_then(|settings| settings.shell_id.as_deref()),
        );
        let is_default = target.default_id().is_some_and(|id| id.eq_ignore_ascii_case(&current));
        let pending = self.launcher_admin_task.is_some();
        let weak = cx.entity().downgrade();
        let language = workspace_ui_language();
        let menu = PopupMenu::build(window, cx, move |menu, _, _| {
            commands(&target).into_iter().fold(menu.external_link_icon(false), |menu, command| {
                let target = target.clone();
                let weak = weak.clone();
                let label = if command == LauncherCommand::SetDefault && is_default {
                    Message::LauncherCurrentDefault
                } else {
                    command.label()
                };
                menu.item(
                    PopupMenuItem::new(language.text(label))
                        .disabled(
                            (command == LauncherCommand::SetDefault && is_default)
                                || (command == LauncherCommand::OpenAdmin && pending),
                        )
                        .on_click(move |_, window, cx| {
                            if let Some(workspace) = weak.upgrade() {
                                workspace.update(cx, |workspace, cx| {
                                    workspace.run_launcher_command(
                                        command,
                                        target.clone(),
                                        window,
                                        cx,
                                    )
                                });
                            }
                        }),
                )
            })
        });
        menu.focus_handle(cx).focus(window, cx);
        let subscription =
            cx.subscribe_in(&menu, window, |this, _, _: &DismissEvent, window, cx| {
                this.launcher_menu = None;
                if this.command_palette_open && this.shell_picker_open {
                    this.command_palette_input.read(cx).focus_handle(cx).focus(window, cx);
                }
                cx.notify();
            });
        self.launcher_menu =
            Some(LauncherContextMenu { menu, position, _subscription: subscription });
        cx.notify();
    }

    pub(super) fn render_launcher_context_menu(&self) -> Option<gpui::AnyElement> {
        let state = self.launcher_menu.as_ref()?;
        Some(
            deferred(
                anchored()
                    .position(state.position)
                    .anchor(Anchor::TopLeft)
                    .snap_to_window_with_margin(px(8.0))
                    .child(
                        div()
                            .id("launcher-context-menu")
                            .debug_selector(|| "launcher-context-menu".to_owned())
                            .child(state.menu.clone()),
                    ),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    fn run_launcher_command(
        &mut self,
        command: LauncherCommand,
        target: LauncherTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.launcher_menu = None;
        match command {
            LauncherCommand::SetDefault => self.set_launcher_default(&target, window, cx),
            LauncherCommand::OpenAdmin => self.launch_administrator(&target, window, cx),
            LauncherCommand::Connect => {
                if let LauncherTarget::Ssh(host) = target {
                    self.dismiss_palette_state();
                    self.add_ssh_terminal(host, window, cx);
                }
            },
            LauncherCommand::Edit | LauncherCommand::Delete => {
                let LauncherTarget::Ssh(host) = target else {
                    return;
                };
                self.dismiss_palette_state();
                self.open_settings(window, cx);
                if let Some((pane, _)) = &self.settings_surface {
                    let edit = command == LauncherCommand::Edit;
                    let pane = pane.clone();
                    window.defer(cx, move |window, cx| {
                        pane.update(cx, |pane, cx| {
                            pane.manage_launcher_ssh(host, edit, window, cx)
                        });
                    });
                }
            },
        }
        if self.command_palette_open && self.shell_picker_open {
            self.command_palette_input.read(cx).focus_handle(cx).focus(window, cx);
        }
        cx.notify();
    }

    fn set_launcher_default(
        &mut self,
        target: &LauncherTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = target.default_id() else {
            return;
        };
        let language = workspace_ui_language();
        if let Err(error) = nebula_settings::persist_keys(&[("shell", id.clone())]) {
            toast(
                window,
                cx,
                ToastKind::Warning,
                language.format(Message::SettingsSaveFailed, &[("error", &error.to_string())]),
            );
            return;
        }
        self.apply_runtime_settings(cx);
        if let Some((pane, _)) = &self.settings_surface {
            pane.update(cx, |pane, cx| pane.sync_launcher_shell(window, cx));
        }
        if let Some(rows) = &mut self.palette_override {
            for row in rows.iter_mut() {
                let Some(target) = LauncherTarget::from_action(&row.action) else {
                    continue;
                };
                let Some(row_id) = target.default_id() else {
                    continue;
                };
                let selected = row_id.eq_ignore_ascii_case(&id);
                row.group_order = if selected { 0 } else { 1 };
                row.group = language
                    .pick(
                        if selected { "推荐" } else { "所有 Shell" },
                        if selected { "Recommended" } else { "All shells" },
                    )
                    .to_owned();
            }
            rows.sort_by_key(|row| row.group_order);
        }
        self.command_palette_selected = self
            .filtered_palette_rows(cx)
            .iter()
            .position(|row| {
                LauncherTarget::from_action(&row.action)
                    .and_then(|target| target.default_id())
                    .is_some_and(|row_id| row_id.eq_ignore_ascii_case(&id))
            })
            .unwrap_or(0);
        toast(
            window,
            cx,
            ToastKind::Success,
            language.format(Message::LauncherDefaultSaved, &[("shell", target.name())]),
        );
    }

    fn launch_administrator(
        &mut self,
        target: &LauncherTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::platform::elevation;
        if self.launcher_admin_task.is_some() {
            return;
        }
        let Some(launch) = target.local_launch() else {
            return;
        };
        let cwd = Self::startup_directory().or_else(|| {
            self.tabs
                .get(self.active)
                .and_then(WorkspaceTab::focused_view)
                .and_then(|view| view.read(cx).local_cwd())
        });
        match elevation::is_elevated() {
            Ok(true) => {
                self.dismiss_palette_state();
                self.add_terminal_with(launch, cwd, None, window, cx);
                return;
            },
            Err(error) => {
                toast(
                    window,
                    cx,
                    ToastKind::Warning,
                    workspace_ui_language()
                        .format(Message::LauncherAdminFailed, &[("error", &error.to_string())]),
                );
                return;
            },
            Ok(false) => {},
        }
        let crate::gpui_shell::terminal::view::TerminalLaunch::Local {
            shell: Some(shell),
            cwd,
            ..
        } = Self::terminal_launch_from_session(&launch, cwd)
        else {
            return;
        };
        let args = elevation::arguments(shell.program(), shell.args(), cwd.as_deref());
        let name = target.name().to_owned();
        self.close_command_palette(window, cx);
        let task = cx.background_executor().spawn(async move { elevation::launch(&args) });
        self.launcher_admin_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.launcher_admin_task = None;
                let language = workspace_ui_language();
                let (kind, text) = match result {
                    Ok(elevation::LaunchOutcome::Started) => (
                        ToastKind::Success,
                        language.format(Message::LauncherAdminStarted, &[("shell", &name)]),
                    ),
                    Ok(elevation::LaunchOutcome::Cancelled) => {
                        (ToastKind::Info, language.text(Message::LauncherAdminCancelled).to_owned())
                    },
                    Err(error) => (
                        ToastKind::Warning,
                        language
                            .format(Message::LauncherAdminFailed, &[("error", &error.to_string())]),
                    ),
                };
                toast(window, cx, kind, text);
                cx.notify();
            });
        }));
    }
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests;
