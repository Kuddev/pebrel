//! Persisting `nebula_settings.txt` and reloading it when another window
//! (or a hand edit) changes the file out from under us.

use winit::dpi::PhysicalSize;
use winit::window::Theme as WinitTheme;

use super::settings;
use super::ssh_ui::merge_ssh_hosts;
use super::{keymap, system_theme_snapshot};

use crate::config::UiConfig;

use super::Display;

impl Display {
    pub(crate) fn persist_nebula_settings(&mut self) {
        settings::nebula_settings_write(&settings::NebulaRuntimeSettings {
            language: self.nebula_language_preference,
            ghost: self.nebula_ghost_enabled,
            accept: self.nebula_accept,
            completion_style: self.nebula_completion_style,
            shell: self.nebula_shell,
            shell_id: self.nebula_shell_id.clone(),
            startup_directory: self.nebula_startup_directory.clone(),
            font_family: self.nebula_font_family.clone(),
            fetch: self.nebula_fetch_enabled,
            powerline: self.nebula_powerline_enabled,
            blur: self.nebula_blur,
            keep_session: self.nebula_keep_session,
            restore_session: self.nebula_restore_session,
            resume_ai: self.nebula_resume_ai,
            tray: self.nebula_tray,
            opacity: self.nebula_window_opacity,
            background: self.nebula_background,
            background_image: self.nebula_background_image.clone(),
            background_image_opacity: self.nebula_background_image_opacity,
            background_image_fit: self.nebula_background_image_fit,
            background_image_alignment: self.nebula_background_image_alignment,
            background_image_cover_chrome: self.nebula_background_image_cover_chrome,
            font_size: Some(self.font_size.as_px() / self.window.scale_factor as f32),
            cursor_shape: self.nebula_cursor_shape,
            cursor_blink: self.nebula_cursor_blink,
            copy_on_select: self.nebula_copy_on_select,
            cjk_bold_regular: self.nebula_cjk_bold_regular,
            tabs_position: self.nebula_tabs_position,
            tab_reveal: self.nebula_tab_reveal_motion,
            density: self.nebula_density,
            new_tab_position: self.nebula_new_tab_position,
            cell_width_mode: self.nebula_cell_width_mode,
            theme: self.nebula_theme_preference,
            follow_system_theme: self.nebula_follow_system_theme,
            pinned_hosts: self.nebula_pinned_hosts.clone(),
            saved_hosts: self.nebula_saved_hosts.clone(),
            hidden_hosts: self.nebula_hidden_hosts.clone(),
            panel_resize: self.nebula_panel_resize,
            sidebar_w: self.nebula_sidebar_w,
            drawer_w: self.nebula_drawer_w,
            hosts_band: self.nebula_hosts_band,
            keybinds: self.nebula_keybinds.clone(),
            quick_terminal_hotkey: self.nebula_quick_terminal_hotkey.clone(),
            ssh_proxy_mode: self.nebula_ssh_proxy_mode,
            ssh_proxy_url: self.nebula_ssh_proxy_url.clone(),
            ssh_proxy_no_proxy: self.nebula_ssh_proxy_no_proxy.clone(),
        });
        self.nebula_settings_mtime = settings::nebula_settings_mtime();
    }

    pub(super) fn reload_nebula_settings_if_changed(&mut self, config: &UiConfig) {
        let mtime = settings::nebula_settings_mtime();
        if mtime == self.nebula_settings_mtime {
            return;
        }

        let settings = settings::nebula_settings_load(config);
        self.nebula_language_preference = settings.language;
        self.nebula_language = settings.language.resolved();
        self.nebula_palette.set_language(self.nebula_language);
        let image_changed = settings.background_image != self.nebula_background_image;
        let font_changed = settings.font_family != self.nebula_font_family;
        self.nebula_theme_preference = settings.theme;
        let follow_system_changed = self.nebula_follow_system_theme != settings.follow_system_theme;
        self.nebula_follow_system_theme = settings.follow_system_theme;
        if follow_system_changed {
            self.window.set_theme(if settings.follow_system_theme {
                None
            } else {
                self.nebula_window_theme_override
            });
            if settings.follow_system_theme {
                self.nebula_system_theme =
                    system_theme_snapshot(self.nebula_system_theme, self.window.theme());
            }
        }
        let active_theme = if settings.follow_system_theme {
            self.nebula_system_theme
                .map(|system| {
                    settings.theme.for_system_appearance(matches!(system, WinitTheme::Light))
                })
                .unwrap_or(settings.theme)
        } else {
            settings.theme
        };
        if active_theme != self.nebula_theme {
            // Hand-edited theme or automatic-mode setting: apply and publish
            // it exactly like an in-panel selection would.
            self.apply_nebula_theme(active_theme);
        }
        self.nebula_ghost_enabled = settings.ghost;
        self.nebula_accept = settings.accept;
        self.nebula_completion_style = settings.completion_style;
        self.nebula_shell = settings.shell;
        self.nebula_shell_id = settings.shell_id;
        self.nebula_startup_directory = settings.startup_directory;
        self.nebula_font_family = settings.font_family;
        if font_changed {
            #[cfg(windows)]
            {
                self.nebula_font_families = self.glyph_cache.refresh_private_fonts();
                self.nebula_font_families
                    .retain(|family| family != crate::font_install::REQUIRED_FONT_FAMILY);
                self.nebula_font_families
                    .insert(0, crate::font_install::REQUIRED_FONT_FAMILY.to_owned());
            }
            let font = self.effective_font(&config.font).with_size(self.font_size);
            self.pending_update.set_font(font);
        }
        self.nebula_fetch_enabled = settings.fetch;
        self.nebula_powerline_enabled = settings.powerline;
        self.nebula_blur = settings.blur;
        self.nebula_keep_session = settings.keep_session;
        self.nebula_restore_session = settings.restore_session;
        self.nebula_resume_ai = settings.resume_ai;
        if self.nebula_tray != settings.tray {
            self.nebula_tray = settings.tray;
            crate::tray::set_enabled(settings.tray);
        }
        self.nebula_panel_resize = settings.panel_resize;
        // 手改文件把宽度调了的话，和拖拽一样要触发一次 reflow。
        let panel_dims_changed = (self.nebula_sidebar_w - settings.sidebar_w).abs() > 0.5
            || (self.nebula_drawer_w - settings.drawer_w).abs() > 0.5;
        self.nebula_sidebar_w = settings.sidebar_w;
        self.nebula_drawer_w = settings.drawer_w;
        self.nebula_hosts_band = settings.hosts_band;
        if panel_dims_changed {
            let size =
                PhysicalSize::new(self.size_info.width() as u32, self.size_info.height() as u32);
            self.pending_update.set_dimensions(size);
        }
        if self.nebula_cjk_bold_regular != settings.cjk_bold_regular {
            // 字形层策略变了：已缓存的 bold CJK 位图作废，清缓存重栅格。
            self.nebula_cjk_bold_regular = settings.cjk_bold_regular;
            self.glyph_cache.wide_bold_use_regular = settings.cjk_bold_regular;
            self.reset_glyph_cache();
        }
        self.nebula_tabs_position = settings.tabs_position;
        self.nebula_tab_reveal_motion = settings.tab_reveal;
        self.nebula_density = settings.density;
        self.nebula_new_tab_position = settings.new_tab_position;
        self.nebula_cell_width_mode = settings.cell_width_mode;
        self.nebula_window_opacity = settings.opacity;
        self.nebula_background = if settings.follow_system_theme {
            Some(active_theme.palette().term_bg)
        } else {
            settings.background
        };
        self.nebula_background_image = settings.background_image;
        self.nebula_background_image_opacity = settings.background_image_opacity;
        self.nebula_background_image_fit = settings.background_image_fit;
        self.nebula_background_image_alignment = settings.background_image_alignment;
        self.nebula_background_image_cover_chrome = settings.background_image_cover_chrome;
        // Sync the host lists too: another window shares the settings file,
        // and skipping this would let this window's next persist overwrite a
        // host that window just saved or pinned.
        self.nebula_pinned_hosts = settings.pinned_hosts;
        self.nebula_saved_hosts = settings.saved_hosts;
        self.nebula_hidden_hosts = settings.hidden_hosts;
        // Hand-edited keybind lines take effect on the next keypress; an
        // in-flight capture is dropped so it can't overwrite the file edit.
        self.nebula_keymap = keymap::build_bindings(&settings.keybinds);
        self.nebula_keybinds = settings.keybinds;
        if self.nebula_quick_terminal_hotkey != settings.quick_terminal_hotkey {
            self.nebula_quick_terminal_hotkey = settings.quick_terminal_hotkey.clone();
            self.nebula_quick_hotkey_request = Some(self.nebula_quick_terminal_hotkey.clone());
            self.nebula_quick_hotkey_error = None;
        }
        self.nebula_keymap_capture = None;
        // 代理键也参与「手改文件即生效」：下一次连接读到的就是新值，这里
        // 只需让设置页与下一次 persist 不吐回旧值。
        let proxy_changed = self.nebula_ssh_proxy_mode != settings.ssh_proxy_mode
            || self.nebula_ssh_proxy_url != settings.ssh_proxy_url
            || self.nebula_ssh_proxy_no_proxy != settings.ssh_proxy_no_proxy;
        self.nebula_ssh_proxy_mode = settings.ssh_proxy_mode;
        self.nebula_ssh_proxy_url = settings.ssh_proxy_url;
        self.nebula_ssh_proxy_no_proxy = settings.ssh_proxy_no_proxy;
        if proxy_changed {
            self.invalidate_proxy_test();
        }
        self.nebula_ssh_proxy_protocol = settings::manual_proxy_parts(&self.nebula_ssh_proxy_url).0;
        self.nebula_ssh_proxy_choice = self
            .nebula_local_proxies
            .iter()
            .position(|proxy| proxy.url() == self.nebula_ssh_proxy_url)
            .map(settings::ProxyChoice::Detected)
            .unwrap_or_else(|| {
                if crate::ssh_proxy::jump_target(&self.nebula_ssh_proxy_url).is_some() {
                    settings::ProxyChoice::Jump
                } else if crate::ssh_proxy::command_target(&self.nebula_ssh_proxy_url).is_some() {
                    settings::ProxyChoice::Command
                } else {
                    settings::ProxyChoice::Manual
                }
            });
        if self.nebula_ssh_proxy_mode == crate::ssh_proxy::ProxyMode::System {
            self.refresh_system_proxy_probe();
        }
        self.nebula_ssh_hosts = merge_ssh_hosts(
            &self.nebula_saved_hosts,
            &self.nebula_pinned_hosts,
            &self.nebula_hidden_hosts,
        );
        if image_changed {
            self.renderer.invalidate_background_image();
        }
        self.nebula_settings_mtime = mtime;
        self.update_window_transparency();
        self.pending_update.dirty = true;
    }
}
