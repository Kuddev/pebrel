//! Settings pane state: open/section/scroll plumbing, the settings view
//! snapshot, tab activation, theme selection, dropdown and toggle families,
//! the fetch/powerline/blur/session toggles, opacity drag, the background
//! color picker, sync actions, background-image and workspace dialogs, and
//! the appearance reset.

use nebula_terminal::vte::ansi::NamedColor;
use winit::dpi::PhysicalSize;
use winit::window::Theme as WinitTheme;

use super::settings;
use super::{
    BackgroundImageAlignment, BackgroundImageFit, NebulaConfirm, NebulaSettingsSection,
    NebulaTheme, SettingsDropdown, SettingsHit, SettingsOpacityTarget, ToastKind, UiLanguage,
    write_nebula_prompt_theme,
};
use super::{file_dialog, keymap, system_theme_snapshot};

use crate::config::UiConfig;
use crate::config::font::Font;
use crate::display::color::Rgb;
use crate::i18n::LanguagePreference;

use super::Display;

impl Display {
    pub fn settings_open(&self) -> bool {
        self.nebula_settings_open
    }

    pub fn ui_language(&self) -> UiLanguage {
        self.nebula_language
    }

    pub fn settings_section(&self) -> NebulaSettingsSection {
        self.nebula_settings_section
    }

    pub fn select_settings_section(&mut self, section: NebulaSettingsSection) {
        self.close_shell_picker();
        self.close_font_picker();
        if self.nebula_settings_section != section {
            self.nebula_settings_section = section;
            // Each section starts reading from its top.
            self.nebula_settings_scroll = 0.0;
            self.pending_update.dirty = true;
        }
        if section == NebulaSettingsSection::Proxy {
            // 进网络页刷新「跟随系统」探测——跨进程读注册表只发生在点击。
            self.refresh_system_proxy_probe();
            if self.nebula_local_proxies.is_empty() {
                self.request_local_proxy_scan();
            }
        }
        if section == NebulaSettingsSection::Providers {
            if self.nebula_providers.active_id.is_empty() {
                if let Some(provider) = self.nebula_providers.providers.first() {
                    self.nebula_providers.active_id = provider.id.clone();
                }
            }
            self.provider_sync_inputs();
        }
        self.update_settings_ime_cursor();
    }

    /// Scroll the settings content by `delta` px (positive = content moves
    /// up). Clamped against the active section's overflow; no-op while the
    /// panel is closed.
    pub fn settings_scroll_by(&mut self, delta: f32) {
        if !self.nebula_settings_open {
            return;
        }
        let area = self.terminal_card_rect();
        let max = settings::settings_max_scroll(
            &self.size_info,
            self.window.scale_factor as f32,
            area,
            self.nebula_settings_section,
            self.nebula_hidden_hosts.len(),
            self.nebula_ssh_hosts.len(),
            self.nebula_density,
            self.ssh_proxy_pane_state(),
            self.keymap_pane_state(),
            self.nebula_providers.providers.len(),
        );
        let next = (self.nebula_settings_scroll + delta).clamp(0.0, max);
        if (next - self.nebula_settings_scroll).abs() > f32::EPSILON {
            self.nebula_settings_scroll = next;
            self.pending_update.dirty = true;
            self.window.request_redraw();
        }
    }

    pub fn settings_scroll(&self) -> f32 {
        self.nebula_settings_scroll
    }

    /// Snapshot of the state the settings render reads, owning the wallpaper
    /// path so `draw_chrome` can still borrow `&mut renderer` afterwards.
    pub(super) fn settings_view(&self) -> settings::SettingsView {
        settings::SettingsView {
            area: self.terminal_card_rect(),
            language_preference: self.nebula_language_preference,
            language: self.nebula_language,
            section: self.nebula_settings_section,
            hover: self.nebula_settings_hover,
            pressed: self.nebula_settings_pressed,
            toggle_motion: std::array::from_fn(|index| {
                self.nebula_ui_anims.settings_toggles[index].value()
            }),
            theme: self.nebula_theme,
            follow_system_theme: self.nebula_follow_system_theme,
            ghost: self.nebula_ghost_enabled,
            accept: self.nebula_accept,
            completion_style: self.nebula_completion_style,
            shell_label: {
                // Rich picked id (cmd/pwsh/nu/wsl:X) wins; else the 2-value
                // enum label. Icon comes from the same table the dropdown
                // rows use, so the setting always mirrors the menu.
                let id = self.nebula_shell_id.as_deref();
                let name = id
                    .and_then(|id| {
                        self.nebula_profiles
                            .iter()
                            .find(|profile| profile.settings_id().as_deref() == Some(id))
                            .map(|profile| profile.name.clone())
                    })
                    .or_else(|| id.map(crate::shell_detect::display_name_for_id))
                    .unwrap_or_else(|| self.nebula_shell.label().to_owned());
                let icon = crate::shell_detect::icon_for_id(
                    id.and_then(|value| {
                        self.nebula_profiles
                            .iter()
                            .find(|profile| profile.settings_id().as_deref() == Some(value))
                            .and_then(|profile| profile.shell_id.as_deref())
                    })
                    .unwrap_or_else(|| id.unwrap_or_else(|| self.nebula_shell.settings_value())),
                );
                format!("{icon}  {name}")
            },
            dropdown: self.nebula_settings_dropdown,
            shells: {
                let mut shells = self
                    .nebula_detected_shells
                    .as_ref()
                    .map(|detected| {
                        detected
                            .iter()
                            .map(|shell| {
                                (shell.id.clone(), shell.name.clone(), shell.program.clone())
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                shells.extend(self.nebula_profiles.iter().filter_map(|profile| {
                    Some((profile.settings_id()?, profile.name.clone(), profile.command.clone()))
                }));
                shells
            },
            shell_id: self.nebula_shell_id.clone(),
            startup_directory: self
                .nebula_startup_directory
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned()),
            providers: self.nebula_providers.providers.clone(),
            active_provider_id: self.nebula_providers.active_id.clone(),
            provider_inputs: self.nebula_provider_inputs.clone(),
            provider_cursors: self.nebula_provider_cursors.clone(),
            provider_focus: self.nebula_provider_focus,
            provider_status: self.nebula_provider_status.clone(),
            font_family: self.nebula_font_family.clone(),
            font_size_px: self.font_size.as_px() / self.window.scale_factor as f32,
            fonts: self.nebula_font_families.clone(),
            font_notice: self.nebula_font_notice.clone(),
            font_show_all: self.nebula_font_show_all,
            font_query: self.nebula_font_query.clone(),
            font_query_cursor: self.nebula_font_query_cursor.clone(),
            font_popup_scroll: self.nebula_font_popup_scroll,
            font_popup_dragging: self.nebula_font_popup_drag.is_some(),
            font_proportional: self.nebula_font_proportional.clone(),
            hidden_hosts: self.nebula_hidden_hosts.clone(),
            ssh_hosts: self
                .nebula_ssh_hosts
                .iter()
                .map(|destination| settings::SshSettingsHost {
                    destination: destination.clone(),
                    label: self
                        .nebula_ssh_labels
                        .get(destination)
                        .cloned()
                        .unwrap_or_else(|| destination.clone()),
                    icon: self
                        .nebula_ssh_icons
                        .get(destination)
                        .cloned()
                        .unwrap_or_else(|| crate::display::ui::os_icons::DEFAULT_ID.to_owned()),
                    pinned: self.nebula_pinned_hosts.iter().any(|host| host == destination),
                })
                .collect(),
            fetch: self.nebula_fetch_enabled,
            powerline: self.nebula_powerline_enabled,
            blur: self.nebula_blur,
            keep_session: self.nebula_keep_session,
            restore_session: self.nebula_restore_session,
            resume_ai: self.nebula_resume_ai,
            tray: self.nebula_tray,
            opacity: self.nebula_window_opacity,
            dragging_opacity: self.nebula_settings_opacity_drag.map(|(target, _, _)| target),
            cursor_shape: self.nebula_cursor_shape,
            cursor_blink: self.nebula_cursor_blink,
            copy_on_select: self.nebula_copy_on_select,
            panel_resize: self.nebula_panel_resize,
            cjk_bold_regular: self.nebula_cjk_bold_regular,
            tab_reveal: self.nebula_tab_reveal_motion,
            density: self.nebula_density,
            new_tab_position: self.nebula_new_tab_position,
            cell_width_mode: self.nebula_cell_width_mode,
            preview_bg: self.preview_terminal_bg(),
            preview_fg: {
                let bg = self.preview_terminal_bg();
                // 亮底配深字、暗底配浅字：预览要在任何自定义背景色上可读。
                let luma = 0.299 * bg.r as f32 + 0.587 * bg.g as f32 + 0.114 * bg.b as f32;
                if luma > 140.0 { Rgb::new(40, 44, 52) } else { Rgb::new(225, 228, 240) }
            },
            background: self.nebula_background,
            bg_hex_input: self.nebula_bg_hex_input.clone(),
            bg_hex_active: self.nebula_bg_hex_active,
            bg_picker_hsv: self.nebula_bg_picker_hsv,
            background_image: self.nebula_background_image.clone(),
            background_image_opacity: self.nebula_background_image_opacity,
            background_image_fit: self.nebula_background_image_fit,
            background_image_alignment: self.nebula_background_image_alignment,
            background_image_cover_chrome: self.nebula_background_image_cover_chrome,
            scroll: self.nebula_settings_scroll,
            keymap: keymap::EDITABLE_ACTIONS
                .iter()
                .map(|(action, ..)| keymap::effective_combo(action, &self.nebula_keymap))
                .collect(),
            quick_terminal_hotkey: self.nebula_quick_terminal_hotkey.clone(),
            quick_hotkey_error: self.nebula_quick_hotkey_error.clone(),
            keymap_capture: self.nebula_keymap_capture,
            keymap_capture_preview: self.nebula_keymap_capture_preview.clone(),
            keymap_query: self.nebula_keymap_query.clone(),
            keymap_query_cursor: self.nebula_keymap_query_cursor.clone(),
            keymap_search_focus: self.nebula_keymap_search_focus,
            keymap_visible: self.keymap_visible_editable(),
            keymap_readonly_visible: self.keymap_visible_readonly(),
            keymap_clash_rows: self.keymap_clash_info().0,
            keymap_clash_note: self.keymap_clash_info().1,
            sync_inputs: self.nebula_sync_inputs.clone(),
            sync_focus: self.nebula_sync_focus,
            sync_auto_pull: self.nebula_sync_auto_pull,
            sync_secret_set: self.nebula_sync_secret_set,
            sync_status: self.nebula_sync_status.clone(),
            sync_busy: self.nebula_sync_busy,
            ssh_proxy_mode: self.nebula_ssh_proxy_mode,
            ssh_proxy_inputs: [
                if self.nebula_ssh_proxy_choice == settings::ProxyChoice::Manual {
                    settings::manual_proxy_parts(&self.nebula_ssh_proxy_url).1.to_owned()
                } else {
                    // 旧版全局 jump:/command: 继续在后端生效，但精简页不把
                    // 这些高级编码伪装成普通 host:port。
                    String::new()
                },
                self.nebula_ssh_proxy_no_proxy.clone(),
                crate::ssh_proxy::command_target(&self.nebula_ssh_proxy_url)
                    .unwrap_or("")
                    .to_owned(),
            ],
            ssh_proxy_cursors: self.nebula_ssh_proxy_cursor.clone(),
            ssh_proxy_focus: self.nebula_ssh_proxy_focus,
            ssh_proxy_protocol: self.nebula_ssh_proxy_protocol,
            ssh_proxy_choice: self.nebula_ssh_proxy_choice,
            local_proxies: self.nebula_local_proxies.clone(),
            proxy_scanning: self.nebula_proxy_scanning,
            system_proxy_probe: self.nebula_system_proxy_probe.clone(),
            proxy_test_status: self.nebula_proxy_test_status.clone(),
            ssh_proxy_overrides: Vec::new(),
            backup_selection: self.nebula_backup_selection,
            backup_status: self.nebula_backup_status.clone(),
            backup_status_remote: self.nebula_backup_status_remote,
            backup_protocol: self.nebula_backup_protocol,
            backup_remote_inputs: self.nebula_backup_remote_inputs.clone(),
            backup_remote_focus: self.nebula_backup_remote_focus,
            backup_remote_secret_set: self.nebula_backup_remote_secret_set,
            backup_busy: self.nebula_backup_busy,
        }
    }

    pub fn set_settings_tab_active(&mut self, active: bool) {
        if self.nebula_settings_open == active {
            if active {
                // 再次聚焦设置页时布尔状态不会变化，但仍要维持非 Shell
                // 页面不占用文件抽屉宽度的布局约束。
                self.close_side_panel_for_special_tab();
            }
            return;
        }
        self.nebula_settings_open = active;
        if active {
            self.nebula_special_tab_active = true;
            self.close_side_panel_for_special_tab();
        }
        if !active {
            if self.nebula_backup_operation.is_some() {
                self.cancel_backup_operation();
            }
            self.commit_sync_field();
            self.commit_backup_remote_field();
            self.nebula_settings_dropdown = None;
            self.nebula_settings_hover = SettingsHit::None;
            self.nebula_settings_pressed = SettingsHit::None;
            self.nebula_keymap_capture = None;
            self.nebula_settings_text_drag = None;
        } else {
            self.load_sync_state();
            self.load_backup_remote_state();
            // Each explicit visit starts at a predictable page origin.
            self.nebula_settings_scroll = 0.0;
            self.nebula_settings_text_drag = None;
            if self.nebula_settings_section == NebulaSettingsSection::Proxy {
                self.refresh_system_proxy_probe();
            }
        }
        self.update_settings_ime_cursor();
        self.pending_update.dirty = true;
    }

    pub fn set_special_tab_active(&mut self, active: bool) {
        self.nebula_special_tab_active = active;
        if active {
            self.close_side_panel_for_special_tab();
            // Keep queued messages for the next terminal tab, but invalidate
            // terminal-only close geometry while a special tab is visible.
            self.nebula_message_close = None;
            self.nebula_message_close_hover = false;
        }
        if !active {
            if self.nebula_backup_operation.is_some() {
                self.cancel_backup_operation();
            }
            self.nebula_settings_open = false;
        }
    }

    /// 文档/设置页接管内容区时关闭右侧抽屉。这里只隐藏抽屉而不销毁 SFTP
    /// 控制器，切回 SSH 标签仍可复用连接，同时非终端页面不再被抽屉挤压。
    fn close_side_panel_for_special_tab(&mut self) {
        if !self.nebula_side_panel.open {
            return;
        }
        self.nebula_side_panel.search_unfocus(false);
        self.nebula_side_panel.commit_unfocus();
        self.nebula_side_panel.open = false;
        let size = PhysicalSize::new(self.size_info.width() as u32, self.size_info.height() as u32);
        self.pending_update.set_dimensions(size);
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    pub fn set_ui_language(&mut self, preference: LanguagePreference) {
        if self.nebula_language_preference == preference {
            return;
        }
        self.nebula_language_preference = preference;
        self.nebula_language = preference.resolved();
        self.nebula_palette.set_language(self.nebula_language);
        self.persist_nebula_settings();
        self.pending_update.dirty = true;
    }

    pub(super) fn apply_nebula_theme(&mut self, theme: NebulaTheme) {
        let previous_theme = self.nebula_theme;
        let theme_changed = previous_theme != theme;
        self.nebula_theme = theme;
        // A theme carries its terminal background (the light themes are
        // unusable without it). Switching theme IS choosing the look, so it
        // overwrites a previous custom color by design.
        self.nebula_background = Some(theme.palette().term_bg);
        // Restyle the terminal color table: OSC 11 must report the new
        // background (TUIs key light/dark off it) and light themes need the
        // light ANSI set to stay readable.
        let defaults = self.nebula_default_colors;
        theme.apply_term_colors(&mut self.colors, &defaults);
        if theme_changed {
            // 旧 pane 可能持有应用通过 OSC 写入的上一主题颜色；交给窗口层在
            // 未持有任何终端锁时统一清理，避免只刷新当前焦点 pane。
            self.terminal_color_resolver
                .theme_changed(previous_theme.palette().term_bg, theme.palette().term_bg);
            self.pending_update.set_terminal_colors_dirty();
        }
        write_nebula_prompt_theme(theme);
        self.pending_update.dirty = true;
    }

    pub fn select_nebula_theme(&mut self, theme: NebulaTheme) {
        self.nebula_theme_preference = theme;
        // Clicking a concrete theme is an explicit manual choice. Automatic
        // mode must step aside instead of changing it again on the next OS
        // appearance event.
        self.nebula_follow_system_theme = false;
        self.window.set_theme(self.nebula_window_theme_override);
        self.apply_nebula_theme(theme);
        self.persist_nebula_settings();
        // Panel stays open so users can adjust several settings at once.
    }

    pub fn toggle_system_theme_following(&mut self) {
        self.nebula_follow_system_theme = !self.nebula_follow_system_theme;
        if self.nebula_follow_system_theme {
            // winit explicitly suppresses ThemeChanged for overridden
            // windows, so automatic mode must let the OS own this value.
            self.window.set_theme(None);
            self.nebula_system_theme =
                system_theme_snapshot(self.nebula_system_theme, self.window.theme());
        } else {
            self.window.set_theme(self.nebula_window_theme_override);
        }
        let theme = if self.nebula_follow_system_theme {
            self.nebula_system_theme
                .map(|system| {
                    self.nebula_theme_preference
                        .for_system_appearance(matches!(system, WinitTheme::Light))
                })
                .unwrap_or(self.nebula_theme_preference)
        } else {
            self.nebula_theme_preference
        };
        self.apply_nebula_theme(theme);
        self.persist_nebula_settings();
    }

    /// Apply a live operating-system appearance change without rewriting the
    /// stored theme family. This is intentionally a no-op in manual mode.
    pub fn system_theme_changed(&mut self, system_theme: WinitTheme) {
        self.sync_system_theme(Some(system_theme));
    }

    /// Refresh the system appearance independently from the window's cached
    /// theme. This also keeps manual-mode windows ready to switch immediately
    /// when the user enables automatic following.
    pub fn sync_system_theme(&mut self, system_theme: Option<WinitTheme>) {
        let Some(system_theme) = system_theme else { return };
        if self.nebula_system_theme == Some(system_theme) {
            return;
        }

        self.nebula_system_theme = Some(system_theme);
        if self.nebula_follow_system_theme {
            let theme = self
                .nebula_theme_preference
                .for_system_appearance(matches!(system_theme, WinitTheme::Light));
            self.apply_nebula_theme(theme);
        }
    }

    /// Remember a reloaded window-decoration preference without allowing it
    /// to suppress OS theme notifications while automatic mode is enabled.
    pub fn update_window_theme_override(&mut self, theme: Option<WinitTheme>) {
        self.nebula_window_theme_override = theme;
        self.window.set_theme(if self.nebula_follow_system_theme { None } else { theme });
    }

    pub fn toggle_ghost(&mut self) {
        self.nebula_ghost_enabled = !self.nebula_ghost_enabled;
        self.persist_nebula_settings();
        self.pending_update.dirty = true;
    }

    pub fn cycle_accept(&mut self) {
        self.nebula_accept = self.nebula_accept.cycle();
        self.persist_nebula_settings();
        self.pending_update.dirty = true;
    }

    /// Flip between inline ghost and popup-list completion (palette /
    /// keybinding path; the settings page goes through
    /// [`Self::set_completion_style_option`]).
    pub fn cycle_completion_style(&mut self) {
        self.nebula_completion_style = self.nebula_completion_style.cycle();
        self.persist_nebula_settings();
        self.pending_update.dirty = true;
    }

    pub fn set_completion_style_option(&mut self, index: usize) {
        if let Some(style) = settings::COMPLETION_STYLE_OPTIONS.get(index) {
            self.nebula_completion_style = *style;
            self.persist_nebula_settings();
        }
        self.nebula_settings_dropdown = None;
        self.pending_update.dirty = true;
    }

    /// Open the "默认 Shell" picker (the settings row click): the same
    /// Toggle the inline shell picker in settings (expand/collapse the list).
    /// Toggle a settings combobox. All dropdowns share one field so opening
    /// one always closes the others.
    pub fn toggle_settings_dropdown(&mut self, dropdown: settings::SettingsDropdown) {
        if self.nebula_settings_dropdown == Some(dropdown) {
            self.nebula_settings_dropdown = None;
        } else {
            if dropdown == settings::SettingsDropdown::Shell {
                // Ensure shells are detected before opening.
                let _ = self
                    .nebula_detected_shells
                    .get_or_insert_with(crate::shell_detect::detect_shells);
            }
            if dropdown == settings::SettingsDropdown::Font {
                self.nebula_font_notice = None;
                self.nebula_font_popup_scroll = 0;
            }
            self.nebula_settings_dropdown = Some(dropdown);
        }
        self.pending_update.dirty = true;
    }

    pub fn close_settings_dropdown(&mut self) -> bool {
        if self.nebula_settings_dropdown.take().is_none() {
            return false;
        }
        self.nebula_bg_hex_active = false;
        // 搜索是这次展开的临时状态：关掉就清空，下次打开从完整目录开始。
        if !self.nebula_font_query.is_empty() {
            self.nebula_font_query.clear();
            self.nebula_font_query_cursor = Default::default();
            self.rebuild_font_catalog();
        }
        self.nebula_font_popup_scroll = 0;
        self.update_settings_ime_cursor();
        self.pending_update.dirty = true;
        self.window.request_redraw();
        true
    }

    pub fn set_background_image_fit_option(&mut self, index: usize) {
        if let Some(fit) = settings::BACKGROUND_FIT_OPTIONS.get(index) {
            self.nebula_background_image_fit = *fit;
            self.persist_nebula_settings();
        }
        self.nebula_settings_dropdown = None;
        self.pending_update.dirty = true;
    }

    pub fn set_background_image_alignment_option(&mut self, index: usize) {
        if let Some(alignment) = settings::BACKGROUND_ALIGNMENT_OPTIONS.get(index) {
            self.nebula_background_image_alignment = *alignment;
            self.persist_nebula_settings();
        }
        self.nebula_settings_dropdown = None;
        self.pending_update.dirty = true;
    }

    pub fn set_accept_option(&mut self, index: usize) {
        if let Some(accept) = settings::ACCEPT_OPTIONS.get(index) {
            self.nebula_accept = *accept;
            self.persist_nebula_settings();
        }
        self.nebula_settings_dropdown = None;
        self.pending_update.dirty = true;
    }

    pub fn set_tab_reveal_option(&mut self, index: usize) {
        if let Some(motion) = settings::TAB_REVEAL_OPTIONS.get(index) {
            self.nebula_tab_reveal_motion = *motion;
            self.persist_nebula_settings();
        }
        self.nebula_settings_dropdown = None;
        self.pending_update.dirty = true;
    }

    /// 切换界面外观预设。密度只影响 Nebula 原生界面的留白、行高与圆角；
    /// 终端字体、单元格几何与 shell 输出一概不受影响，但界面让出的空间会
    /// 让终端行列数增加——那正是紧凑档的收益。
    pub fn set_density_option(&mut self, index: usize) {
        if let Some(density) = settings::DENSITY_OPTIONS.get(index).copied()
            && density != self.nebula_density
        {
            self.nebula_density = density;
            self.persist_nebula_settings();
            // 与折叠侧栏同样的重排路径：界面尺寸变了，网格与 PTY 要跟上。
            let size =
                PhysicalSize::new(self.size_info.width() as u32, self.size_info.height() as u32);
            self.pending_update.set_dimensions(size);
        }
        self.nebula_settings_dropdown = None;
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    pub fn set_new_tab_position_option(&mut self, index: usize) {
        if let Some(position) = settings::NEW_TAB_POSITION_OPTIONS.get(index) {
            self.nebula_new_tab_position = *position;
            self.persist_nebula_settings();
        }
        self.nebula_settings_dropdown = None;
        self.pending_update.dirty = true;
    }

    /// 切换单元格宽度模式。列宽取整方式变了就必须重算单元格——重推当前
    /// 字体让字体更新路径走一遍，网格、viewport、pane 与 PTY 随之一致更新，
    /// 无需重启。字号、字体家族与行高都不变。
    pub fn set_cell_width_mode_option(&mut self, index: usize, base: &Font) {
        if let Some(mode) = settings::CELL_WIDTH_MODE_OPTIONS.get(index)
            && *mode != self.nebula_cell_width_mode
        {
            self.nebula_cell_width_mode = *mode;
            self.persist_nebula_settings();
            let font = self.effective_font(base).with_size(self.font_size);
            self.pending_update.set_font(font);
        }
        self.nebula_settings_dropdown = None;
        self.pending_update.dirty = true;
    }

    /// Returns true when the default cursor style changed (the caller then
    /// pushes the new default into every live terminal).
    pub fn set_cursor_shape_option(&mut self, index: usize) -> bool {
        self.nebula_settings_dropdown = None;
        self.pending_update.dirty = true;
        let Some(shape) = settings::CURSOR_SHAPE_OPTIONS.get(index).copied() else {
            return false;
        };
        if self.nebula_cursor_shape == shape {
            return false;
        }
        self.nebula_cursor_shape = shape;
        self.persist_nebula_settings();
        true
    }

    pub fn toggle_cursor_blink(&mut self) {
        self.nebula_cursor_blink = !self.nebula_cursor_blink;
        self.persist_nebula_settings();
        self.pending_update.dirty = true;
    }

    pub fn toggle_copy_on_select(&mut self) {
        self.nebula_copy_on_select = !self.nebula_copy_on_select;
        self.persist_nebula_settings();
        self.pending_update.dirty = true;
    }

    /// 同步拉到新历史后热加载（spec 003）：ghost 补全的内存副本只在
    /// 启动时 load，这里手动重读一次。
    pub fn reload_nebula_history(&mut self) {
        self.nebula_history = crate::nebula_history::NebulaHistory::load();
        self.pending_update.dirty = true;
    }

    pub fn toggle_cjk_bold_regular(&mut self) {
        self.nebula_cjk_bold_regular = !self.nebula_cjk_bold_regular;
        self.glyph_cache.wide_bold_use_regular = self.nebula_cjk_bold_regular;
        // 已缓存的 bold CJK 位图立即作废：切换要当场可见，不能等重启。
        self.reset_glyph_cache();
        self.persist_nebula_settings();
        self.pending_update.dirty = true;
    }

    /// The default cursor style (shape + blink) every terminal should fall
    /// back to when no escape has overridden it.
    pub fn nebula_default_cursor_style(&self) -> nebula_terminal::vte::ansi::CursorStyle {
        nebula_terminal::vte::ansi::CursorStyle {
            shape: self.nebula_cursor_shape,
            blinking: self.nebula_cursor_blink,
        }
    }

    pub fn toggle_fetch(&mut self) {
        self.nebula_fetch_enabled = !self.nebula_fetch_enabled;
        self.persist_nebula_settings();
        self.pending_update.dirty = true;
    }

    pub fn toggle_powerline(&mut self) {
        self.nebula_powerline_enabled = !self.nebula_powerline_enabled;
        self.persist_nebula_settings();
        self.pending_update.dirty = true;
    }

    /// 界面→背景模糊。直接作用到窗口本身：DWM 的 backdrop 是**窗口属性**，
    /// 不经过我们的渲染循环，所以标脏重绘是等不到它的。
    pub fn toggle_blur(&mut self) {
        self.nebula_blur = !self.nebula_blur;
        self.window.set_blur(self.nebula_blur);
        self.persist_nebula_settings();
        self.pending_update.dirty = true;
    }

    /// 高级→会话: whether closing a window keeps its shells in the resident
    /// process (detach / re-attach restore) or kills them outright.
    pub fn toggle_keep_session(&mut self) {
        self.nebula_keep_session = !self.nebula_keep_session;
        self.persist_nebula_settings();
        self.pending_update.dirty = true;
    }

    /// 高级→会话: 启动时是否回放上次的标签。写进设置文件即可——真正读它的
    /// 是下次启动的 `create_initial_window`。
    pub fn toggle_restore_session(&mut self) {
        self.nebula_restore_session = !self.nebula_restore_session;
        self.persist_nebula_settings();
        self.pending_update.dirty = true;
    }

    pub fn begin_settings_opacity_drag(&mut self, target: SettingsOpacityTarget, pointer_x: f32) {
        let slider = settings::opacity_slider_rect(
            &self.ui_size_info(),
            self.window.scale_factor as f32,
            self.terminal_card_rect(),
            self.nebula_settings_scroll,
            target,
            self.nebula_density,
        );
        self.nebula_settings_opacity_drag = Some((target, slider.0, slider.2));
        self.update_settings_opacity_drag(pointer_x);
    }

    pub fn update_settings_opacity_drag(&mut self, pointer_x: f32) -> bool {
        let Some((target, track_x, track_width)) = self.nebula_settings_opacity_drag else {
            return false;
        };
        let value = settings::opacity_from_pointer(pointer_x, (track_x, 0.0, track_width, 0.0));
        match target {
            SettingsOpacityTarget::Terminal => {
                if (self.nebula_window_opacity - value).abs() <= f32::EPSILON {
                    return true;
                }
                self.nebula_window_opacity = value;
                self.update_window_transparency();
            },
            SettingsOpacityTarget::BackgroundImage => {
                if (self.nebula_background_image_opacity - value).abs() <= f32::EPSILON {
                    return true;
                }
                self.nebula_background_image_opacity = value;
                self.update_window_transparency();
            },
        }
        self.pending_update.dirty = true;
        self.window.request_redraw();
        true
    }

    pub fn finish_settings_opacity_drag(&mut self) -> bool {
        if self.nebula_settings_opacity_drag.take().is_none() {
            return false;
        }
        // 拖动过程只刷新画面，松手后集中落盘，避免连续写设置文件。
        self.persist_nebula_settings();
        self.pending_update.dirty = true;
        true
    }

    /// 键盘快捷键仍可循环预设背景色（与色盘同一色板）。
    pub fn cycle_background_color(&mut self) {
        self.nebula_bg_palette_index =
            (self.nebula_bg_palette_index + 1) % settings::BACKGROUND_SWATCHES.len();
        self.nebula_background = Some(settings::BACKGROUND_SWATCHES[self.nebula_bg_palette_index]);
        self.persist_nebula_settings();
        self.pending_update.dirty = true;
    }

    /// 打开/关闭背景色浮层（调色盘 + 色板 + 16 进制输入），草稿预填当前
    /// 生效色。灰/黑/白的色相在 RGB 里是缺失的：那时保留上一次的色相，
    /// 用户把明度拨回来时不会发现色相被重置到红色。
    pub fn open_background_color_picker(&mut self) {
        let current = self.nebula_background.unwrap_or(self.colors[NamedColor::Background]);
        self.nebula_bg_hex_input = format!("#{:02X}{:02X}{:02X}", current.r, current.g, current.b);
        self.nebula_bg_hex_active = false;
        let (h, s, v) = settings::rgb_to_hsv(current);
        if s > f32::EPSILON && v > f32::EPSILON {
            self.nebula_bg_picker_hsv = (h, s, v);
        } else {
            self.nebula_bg_picker_hsv = (self.nebula_bg_picker_hsv.0, s, v);
        }
        self.toggle_settings_dropdown(settings::SettingsDropdown::BackgroundColor);
    }

    /// 点选色板某格：应用、落盘并收起浮层。
    pub fn set_background_color_option(&mut self, index: usize) {
        if let Some(color) = settings::BACKGROUND_SWATCHES.get(index) {
            self.nebula_bg_palette_index = index;
            self.nebula_background = Some(*color);
            self.nebula_bg_picker_hsv = settings::rgb_to_hsv(*color);
            self.persist_nebula_settings();
        }
        self.close_settings_dropdown();
        self.pending_update.dirty = true;
    }

    /// 调色盘按下：记录拖拽目标并立即按指针位置取一次色。
    pub fn begin_bg_picker_drag(&mut self, part: settings::BgPickerPart, x: f32, y: f32) {
        self.nebula_bg_picker_drag = Some(part);
        self.update_bg_picker_drag(x, y);
    }

    /// 调色盘拖拽中：指针 → HSV → 实时应用为背景色（不落盘）。
    /// 预览卡、终端和 hex 草稿同步跟随，松手才写设置文件。
    pub fn update_bg_picker_drag(&mut self, x: f32, y: f32) -> bool {
        let Some(part) = self.nebula_bg_picker_drag else {
            return false;
        };
        let (sv, hue) = settings::background_color_picker_rects(
            &self.ui_size_info(),
            self.window.scale_factor as f32,
            self.terminal_card_rect(),
            self.nebula_settings_scroll,
            self.nebula_density,
        );
        let (h, s, v) = &mut self.nebula_bg_picker_hsv;
        match part {
            settings::BgPickerPart::Sv => {
                *s = ((x - sv.0) / sv.2.max(1.0)).clamp(0.0, 1.0);
                *v = (1.0 - (y - sv.1) / sv.3.max(1.0)).clamp(0.0, 1.0);
            },
            settings::BgPickerPart::Hue => {
                *h = ((x - hue.0) / hue.2.max(1.0)).clamp(0.0, 1.0) * 360.0;
            },
        }
        let color = settings::hsv_to_rgb(*h, *s, *v);
        self.nebula_background = Some(color);
        self.nebula_bg_hex_input = format!("#{:02X}{:02X}{:02X}", color.r, color.g, color.b);
        self.nebula_bg_hex_active = false;
        self.pending_update.dirty = true;
        self.window.request_redraw();
        true
    }

    /// 调色盘松手：集中落盘（拖动过程只刷新画面，避免连续写设置文件）。
    pub fn finish_bg_picker_drag(&mut self) -> bool {
        if self.nebula_bg_picker_drag.take().is_none() {
            return false;
        }
        self.persist_nebula_settings();
        self.pending_update.dirty = true;
        true
    }

    pub fn focus_bg_hex_input(&mut self) {
        self.nebula_bg_hex_active = true;
        self.pending_update.dirty = true;
    }

    /// 追加 hex 字符：只收 `#` 与 16 进制位，总长 ≤ 7（`#RRGGBB`）。
    pub fn bg_hex_push(&mut self, ch: char) {
        let ok = ch == '#' || ch.is_ascii_hexdigit();
        if ok && self.nebula_bg_hex_input.chars().count() < 7 {
            self.nebula_bg_hex_input.push(ch);
            self.pending_update.dirty = true;
        }
    }

    pub fn bg_hex_backspace(&mut self) {
        if self.nebula_bg_hex_input.pop().is_some() {
            self.pending_update.dirty = true;
        }
    }

    /// 回车应用 16 进制草稿；解析失败保持浮层与草稿原样。
    pub fn bg_hex_commit(&mut self) -> bool {
        let Some(color) = settings::parse_hex_rgb(self.nebula_bg_hex_input.trim()) else {
            return false;
        };
        self.nebula_background = Some(color);
        self.nebula_bg_picker_hsv = settings::rgb_to_hsv(color);
        self.persist_nebula_settings();
        self.close_settings_dropdown();
        self.pending_update.dirty = true;
        true
    }

    pub fn toggle_sync_auto_pull(&mut self) {
        self.nebula_sync_auto_pull = !self.nebula_sync_auto_pull;
        let mut cfg = crate::sync::SyncConfig::load();
        cfg.url = self.nebula_sync_inputs[0].trim().to_owned();
        cfg.username = self.nebula_sync_inputs[1].trim().to_owned();
        cfg.auto_pull = self.nebula_sync_auto_pull;
        if let Err(err) = cfg.save() {
            self.nebula_sync_status = Some((err, true));
        }
        self.pending_update.dirty = true;
    }

    /// 推/拉按钮按下：提交草稿、置忙。实际网络动作由调用侧发事件。
    pub fn begin_sync_action(&mut self) -> bool {
        if self.nebula_sync_busy {
            return false;
        }
        self.commit_sync_field();
        self.nebula_sync_busy = true;
        self.nebula_sync_status = Some(("同步中…".to_owned(), false));
        self.pending_update.dirty = true;
        true
    }

    /// 后台同步线程回报（`NebulaSyncDone`）。
    pub fn sync_action_done(&mut self, message: &str, error: bool) {
        self.nebula_sync_busy = false;
        self.nebula_sync_status = Some((message.to_owned(), error));
        // 拉取可能改写了设置文件的凭据外字段；存在性也可能被首存翻转。
        self.nebula_sync_secret_set = [crate::sync::has_password(), crate::sync::has_passphrase()];
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    /// Pick a background image through the OS file dialog, then persist it and
    /// refresh the renderer's cached wallpaper. On non-Windows platforms the
    /// native dialog isn't wired up, so we fall back to opening the settings
    /// file for the path to be entered by hand.
    /// Native save dialog for a workspace export, pre-filled with
    /// `default_name`. `None` when the user cancels.
    pub fn save_workspace_dialog(&self, default_name: &str) -> Option<std::path::PathBuf> {
        file_dialog::save_workspace_file(&self.window, default_name)
    }

    /// Native open dialog for a workspace import. `None` when cancelled.
    pub fn pick_workspace_dialog(&self) -> Option<std::path::PathBuf> {
        file_dialog::pick_workspace_file(&self.window)
    }

    pub fn pick_background_image(&mut self) {
        #[cfg(windows)]
        {
            if let Some(path) = file_dialog::pick_image_file(&self.window) {
                self.nebula_background_image = Some(path);
                self.persist_nebula_settings();
                self.renderer.invalidate_background_image();
                self.update_window_transparency();
                self.pending_update.dirty = true;
            }
        }
        #[cfg(not(windows))]
        {
            self.open_user_config_file();
        }
    }

    pub fn clear_background_image(&mut self) {
        if self.nebula_background_image.take().is_some() {
            self.persist_nebula_settings();
            self.renderer.invalidate_background_image();
            self.update_window_transparency();
            self.pending_update.dirty = true;
        }
    }

    pub fn request_toggle_background_image_cover_chrome(&mut self) {
        if self.nebula_background_image_cover_chrome {
            self.nebula_background_image_cover_chrome = false;
            self.persist_nebula_settings();
        } else {
            self.nebula_confirm = Some(NebulaConfirm::EnableBackgroundImageCoverChrome);
        }
        self.pending_update.dirty = true;
    }

    /// 设置·交互「拖拽调节侧栏」开关。关→开要过一次确认框（宽度拖动会
    /// 实时重排终端，用户裁定必须明确告知）；开→关直接生效。
    pub fn request_toggle_panel_resize(&mut self) {
        if self.nebula_panel_resize {
            self.nebula_panel_resize = false;
            self.persist_nebula_settings();
        } else {
            self.nebula_confirm = Some(NebulaConfirm::EnablePanelResize);
        }
        self.pending_update.dirty = true;
    }

    /// 确认框「是」：真正开启拖拽调节。
    pub fn confirm_panel_resize(&mut self) {
        self.nebula_confirm = None;
        self.nebula_panel_resize = true;
        self.persist_nebula_settings();
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    pub fn confirm_background_image_cover_chrome(&mut self) {
        self.nebula_confirm = None;
        self.nebula_background_image_cover_chrome = true;
        self.persist_nebula_settings();
        self.pending_update.dirty = true;
    }

    pub fn open_user_config_file(&mut self) {
        self.persist_nebula_settings();
        let active_lua = self.nebula_config_paths.first().filter(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("lua"))
        });
        let path = active_lua.cloned().or_else(|| crate::config::source::default_lua_path().ok());
        let Some(path) = path else {
            log::error!(
                target: crate::logging::LOG_TARGET_CONFIG,
                "Unable to determine Lua config path"
            );
            return;
        };
        if !path.exists() {
            let language = crate::config::template::resolve_template_language(
                Some(self.nebula_language_preference.as_str()),
                None,
                crate::config::template::system_locale().as_deref(),
            )
            .unwrap_or(crate::config::template::TemplateLanguage::EnUs);
            if let Err(error) = crate::config::template::ensure_user_lua_config(&path, language) {
                log::error!(
                    target: crate::logging::LOG_TARGET_CONFIG,
                    "Unable to create Lua config {:?}: {error}",
                    path
                );
                return;
            }
        }
        #[cfg(windows)]
        let _ = std::process::Command::new("notepad.exe").arg(&path).spawn();
        #[cfg(target_os = "macos")]
        let _ = std::process::Command::new("open").arg(&path).spawn();
        #[cfg(all(not(windows), not(target_os = "macos")))]
        let _ = std::process::Command::new("xdg-open").arg(&path).spawn();
        self.pending_update.dirty = true;
    }

    pub fn reset_appearance_settings(&mut self) {
        self.nebula_theme_preference = NebulaTheme::default();
        self.nebula_follow_system_theme = false;
        self.window.set_theme(self.nebula_window_theme_override);
        self.nebula_theme = self.nebula_theme_preference;
        let defaults = self.nebula_default_colors;
        self.nebula_theme.apply_term_colors(&mut self.colors, &defaults);
        write_nebula_prompt_theme(self.nebula_theme);
        self.nebula_window_opacity = 1.0;
        self.nebula_background = None;
        self.nebula_background_image = None;
        self.nebula_background_image_opacity = 0.38;
        self.nebula_background_image_fit = BackgroundImageFit::default();
        self.nebula_background_image_alignment = BackgroundImageAlignment::default();
        self.nebula_background_image_cover_chrome = false;
        self.window.set_transparent(false);
        self.persist_nebula_settings();
        self.pending_update.dirty = true;
    }

    pub(super) fn settings_toggle_targets(&self) -> [bool; settings::SETTINGS_TOGGLE_COUNT] {
        let provider =
            self.provider_edit_index().and_then(|index| self.nebula_providers.providers.get(index));
        [
            self.nebula_follow_system_theme,
            self.nebula_ghost_enabled,
            self.nebula_cursor_blink,
            self.nebula_copy_on_select,
            self.nebula_panel_resize,
            self.nebula_cjk_bold_regular,
            self.nebula_fetch_enabled,
            self.nebula_powerline_enabled,
            self.nebula_blur,
            self.nebula_keep_session,
            self.nebula_restore_session,
            self.nebula_sync_auto_pull,
            self.nebula_background_image_cover_chrome,
            provider.is_some_and(|provider| provider.codex_goals),
            provider.is_some_and(|provider| provider.codex_remote_compaction),
            self.nebula_resume_ai,
            self.nebula_tray,
        ]
    }

    /// 高级：「常驻托盘图标」开关。翻转即生效：托盘线程收到 enable/disable
    /// 后立刻挂上或摘掉通知区图标。
    pub fn toggle_tray(&mut self) {
        self.nebula_tray = !self.nebula_tray;
        crate::tray::set_enabled(self.nebula_tray);
        self.persist_nebula_settings();
        self.pending_update.dirty = true;
    }

    /// 高级·会话：「恢复时接续 AI 对话」开关。
    pub fn toggle_resume_ai(&mut self) {
        self.nebula_resume_ai = !self.nebula_resume_ai;
        self.persist_nebula_settings();
        self.pending_update.dirty = true;
    }
}
