//! Legacy presentation adapter for the shared Shell/SSH launcher.

use super::Display;
use super::command_palette::LauncherFilter;

impl Display {
    /// Open the new-tab dropdown: detected shells (installed-shell order) plus
    /// any config profiles. Detection runs once and is cached — the chevron
    /// beside the "+" opens this — the familiar profile menu.
    pub fn open_shell_menu(&mut self, profiles: &[crate::config::ui_config::Profile]) {
        self.open_launcher_menu(profiles, LauncherFilter::All);
    }

    fn open_launcher_menu(
        &mut self,
        profiles: &[crate::config::ui_config::Profile],
        filter: LauncherFilter,
    ) {
        let ssh_rows: Vec<(String, String, String)> = self
            .nebula_ssh_hosts
            .iter()
            .map(|host| {
                let label =
                    self.nebula_ssh_labels.get(host).cloned().unwrap_or_else(|| host.clone());
                let icon = self
                    .nebula_ssh_icons
                    .get(host)
                    .cloned()
                    .unwrap_or_else(|| crate::display::ui::os_icons::DEFAULT_ID.to_owned());
                (label, host.clone(), icon)
            })
            .collect();
        let shells =
            self.nebula_detected_shells.get_or_insert_with(crate::shell_detect::detect_shells);
        let default_shell =
            self.nebula_shell_id.as_deref().unwrap_or_else(|| self.nebula_shell.settings_value());
        self.nebula_palette.set_shell_menu(shells, profiles, default_shell);
        self.nebula_palette.set_ssh_hosts_with_icons(&ssh_rows);
        self.nebula_palette.open_launcher(filter);
        self.pending_update.dirty = true;
    }

    /// Ctrl+K：开/关 shell picker——与 "+" 旁 chevron 打开的是同一份列表
    /// （settings 页的 shell 下拉是另一回事，见 `toggle_shell_picker`）。
    /// 已开着的 shell picker 再按一次收起；其他 palette 模式则切换过来。
    pub fn toggle_shell_menu(&mut self, profiles: &[crate::config::ui_config::Profile]) {
        let picker_open = self.nebula_palette.is_picker()
            && !self.nebula_palette.is_picking_default()
            && !self.nebula_palette.is_picking_directory();
        if picker_open {
            self.nebula_palette.close();
            self.pending_update.dirty = true;
        } else {
            self.open_shell_menu(profiles);
        }
    }

    pub fn open_ssh_menu(&mut self, profiles: &[crate::config::ui_config::Profile]) {
        self.open_launcher_menu(profiles, LauncherFilter::Ssh);
    }

    pub fn toggle_ssh_menu(&mut self, profiles: &[crate::config::ui_config::Profile]) {
        if self.nebula_palette.is_launcher()
            && self.nebula_palette.launcher_filter() == LauncherFilter::Ssh
        {
            self.nebula_palette.close();
            self.pending_update.dirty = true;
        } else {
            self.open_ssh_menu(profiles);
        }
    }
}
