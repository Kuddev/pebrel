//! Settings entry points for actions initiated in the Shell/SSH launcher.

use super::*;

impl SettingsPane {
    pub(in crate::gpui_shell) fn sync_launcher_shell(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.runtime = RuntimeSettings::load();
        self.restore_shell_selection(window, cx);
        cx.notify();
    }

    pub(in crate::gpui_shell) fn manage_launcher_ssh(
        &mut self,
        host: String,
        edit: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings_search_input.update(cx, |input, cx| input.set_value("", window, cx));
        self.search_origin_section = None;
        self.active_section = SECTION_IDS.iter().position(|section| *section == "ssh").unwrap();
        self.ssh_hosts = crate::gpui_shell::ssh_hosts::SshHostLists::load();
        self.prepare_launcher_ssh_host(&host, window, cx);
        if edit {
            self.open_ssh_editor(Some(host), window, cx);
        } else {
            if self.ssh_editor.is_some() {
                self.close_ssh_editor(window, cx);
            }
            self.ssh_delete_confirm = Some(host);
        }
        cx.notify();
    }
}
