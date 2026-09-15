use crate::backup_remote;
use crate::display::ui::widgets;
use crate::display::{contains_rect, NebulaSettingsSection, SizeInfo};

use super::geometry::{
    background_color_popup, dropdown_anchor, font_popup_slot, font_popup_window, font_search_field_rect,
    fit_provider_rows, keymap_search_rect, popup_visible_index, provider_input_rect, row_action_rect,
    settings_geometry, ssh_host_action_rect, ssh_proxy_input_rect, STANDARD_ROW_ACTION_W,
};
use super::view::{
    backup_remote_actions_rect, backup_segment_rects, ssh_proxy_expand_control,
    ssh_proxy_manual_controls, ssh_proxy_mode_control, ssh_proxy_test_button, sync_button_rects,
    sync_input_rect,
};
use super::{
    KeymapPaneState, ProxyPaneState, SettingsDropdown, SettingsHit,
    ACCEPT_OPTIONS, BACKUP_PROTOCOL_OPTIONS, BACKGROUND_ALIGNMENT_OPTIONS,
    BACKGROUND_FIT_OPTIONS, CELL_WIDTH_MODE_OPTIONS, COMPLETION_STYLE_OPTIONS,
    CURSOR_SHAPE_OPTIONS, DENSITY_OPTIONS, LANGUAGE_OPTIONS, NEW_TAB_POSITION_OPTIONS,
SHOW_BACKUP_SETTINGS, SHOW_WEBDAV_SYNC_SETTINGS, SSH_PROXY_MODE_OPTIONS, TAB_REVEAL_OPTIONS,
};

pub fn settings_hit(
    size_info: &SizeInfo,
    scale_factor: f32,
    area: (f32, f32, f32, f32),
    x: f32,
    y: f32,
    popup_open: bool,
    section: NebulaSettingsSection,
    scroll: f32,
    dropdown: Option<SettingsDropdown>,
    shell_count: usize,
    font_count: usize,
    font_popup_scroll: usize,
    hidden_host_count: usize,
    ssh_host_count: usize,
    density: crate::display::ui::tokens::Density,
    proxy: ProxyPaneState,
    keymap_pane: KeymapPaneState,
    provider_count: usize,
    backup_protocol: crate::backup_remote::BackupProtocol,
) -> SettingsHit {
    let mut geometry = settings_geometry(
        size_info,
        scale_factor,
        area,
        scroll,
        hidden_host_count,
        ssh_host_count,
        density,
        proxy,
        keymap_pane,
    );
    fit_provider_rows(&mut geometry, provider_count);
    let s = |v: f32| v * scale_factor;

    if contains_rect(geometry.gear, x, y) {
        return SettingsHit::Toggle;
    }

    if !popup_open {
        return SettingsHit::None;
    }

    // Scrolled content only responds inside its viewport (below the fixed
    // header, above the popup's bottom edge).
    let (_, py, _, ph) = geometry.popup;
    let in_viewport = y >= geometry.content_top && y <= py + ph;

    // An expanded dropdown owns the pointer first: its floating option list
    // covers later rows, and those must not react through it.
    if let Some(dropdown) = dropdown {
        // 背景色是专用浮层（色板网格 + hex 输入），不走通用行列表。
        if dropdown == SettingsDropdown::BackgroundColor {
            if section == NebulaSettingsSection::Appearance {
                let popup = background_color_popup(&geometry, scale_factor);
                if contains_rect(popup.sv, x, y) {
                    return SettingsHit::BackgroundSvPlane;
                }
                if contains_rect(popup.hue, x, y) {
                    return SettingsHit::BackgroundHueBar;
                }
                for (index, rect) in popup.swatch.iter().enumerate() {
                    if contains_rect(*rect, x, y) {
                        return SettingsHit::BackgroundSwatch(index);
                    }
                }
                if contains_rect(popup.hex, x, y) {
                    return SettingsHit::BackgroundHexInput;
                }
                if contains_rect(popup.rect, x, y) {
                    return SettingsHit::BackgroundPopupPanel;
                }
            }
        } else if let Some((anchor, total)) =
            dropdown_anchor(&geometry, section, dropdown, shell_count, font_count, scale_factor)
        {
            let (offset, count) = if dropdown == SettingsDropdown::Font {
                font_popup_window(total, font_popup_scroll)
            } else {
                (0, total)
            };
            let popup = widgets::combobox_popup_rect(
                anchor,
                count,
                scale_factor,
                geometry.content_top,
                py + ph - s(6.0),
            );
            if let Some(index) = widgets::popup_row_at(popup, count, scale_factor, x, y) {
                let index = if dropdown == SettingsDropdown::Font && index > 0 {
                    index + offset
                } else {
                    index
                };
                return match dropdown {
                    SettingsDropdown::Shell => SettingsHit::ShellPickerRow(index),
                    SettingsDropdown::Font => match font_popup_slot(index) {
                        Some(slot) => SettingsHit::FontPickerRow(slot),
                        None => SettingsHit::FontSearchField,
                    },
                    SettingsDropdown::BackgroundFit => SettingsHit::FitOption(index),
                    SettingsDropdown::BackgroundAlignment => SettingsHit::AlignOption(index),
                    SettingsDropdown::Language => SettingsHit::Language(LANGUAGE_OPTIONS[index]),
                    SettingsDropdown::Accept => SettingsHit::AcceptOption(index),
                    SettingsDropdown::CompletionStyle => SettingsHit::CompletionStyleOption(index),
                    SettingsDropdown::BackupProtocol => SettingsHit::BackupProtocolOption(index),
                    SettingsDropdown::TabReveal => SettingsHit::TabRevealOption(index),
                    SettingsDropdown::Density => SettingsHit::DensityOption(index),
                    SettingsDropdown::NewTabPosition => SettingsHit::NewTabPositionOption(index),
                    SettingsDropdown::CellWidthMode => SettingsHit::CellWidthModeOption(index),
                    SettingsDropdown::CursorShape => SettingsHit::CursorShapeOption(index),
                    SettingsDropdown::SshProxyMode => SettingsHit::SshProxyModeOption(index),
                    SettingsDropdown::SshProxyProtocol => {
                        SettingsHit::SshProxyProtocolOption(index)
                    },
                    SettingsDropdown::SshJumpHost => SettingsHit::SshJumpHostOption(index),
                    // 背景色浮层在上方特判处理，走不到通用行列表。
                    SettingsDropdown::BackgroundColor => SettingsHit::Panel,
                };
            }
            if contains_rect(popup, x, y) {
                // Padding strip inside the floating list: swallow the click
                // so rows underneath cannot react through the popup.
                return SettingsHit::Panel;
            }
        }
    }

    // Sidebar navigation and the header reset button are available from every
    // section.
    for (nav_section, nx, ny, nw, nh) in geometry.nav {
        if nav_section == NebulaSettingsSection::Backup && !SHOW_BACKUP_SETTINGS {
            continue;
        }
        if contains_rect((nx, ny, nw, nh), x, y) {
            return SettingsHit::Nav(nav_section);
        }
    }
    if !matches!(section, NebulaSettingsSection::Ssh | NebulaSettingsSection::Providers)
        && contains_rect(geometry.reset, x, y)
    {
        return SettingsHit::Reset;
    }

    if in_viewport {
        match section {
            NebulaSettingsSection::Appearance => {
                for (theme, ox, oy, ow, oh) in geometry.options {
                    if contains_rect((ox, oy, ow, oh), x, y) {
                        return SettingsHit::Theme(theme);
                    }
                }
                if contains_rect(widgets::toggle_rect(geometry.system_theme, scale_factor), x, y) {
                    return SettingsHit::SystemThemeToggle;
                }
                if contains_rect(widgets::combobox_rect(geometry.background, scale_factor), x, y) {
                    return SettingsHit::BackgroundColor;
                }
                if contains_rect(geometry.background_image_clear, x, y) {
                    return SettingsHit::BackgroundImageClear;
                }
                if contains_rect(geometry.background_image, x, y) {
                    return SettingsHit::BackgroundImage;
                }
                if contains_rect(
                    widgets::combobox_rect(geometry.background_image_fit, scale_factor),
                    x,
                    y,
                ) {
                    return SettingsHit::BackgroundImageFit;
                }
                if contains_rect(
                    widgets::combobox_rect(geometry.background_image_alignment, scale_factor),
                    x,
                    y,
                ) {
                    return SettingsHit::BackgroundImageAlignment;
                }
                if contains_rect(geometry.background_image_opacity_slider, x, y) {
                    return SettingsHit::BackgroundImageOpacitySlider;
                }
                if contains_rect(
                    widgets::toggle_rect(geometry.background_image_cover_chrome, scale_factor),
                    x,
                    y,
                ) {
                    return SettingsHit::BackgroundImageCoverChrome;
                }
                if contains_rect(
                    widgets::combobox_rect(geometry.cursor_shape_row, scale_factor),
                    x,
                    y,
                ) {
                    return SettingsHit::CursorShapeDropdown;
                }
                if contains_rect(
                    widgets::toggle_rect(geometry.cursor_blink_row, scale_factor),
                    x,
                    y,
                ) {
                    return SettingsHit::CursorBlinkToggle;
                }
                if contains_rect(widgets::combobox_rect(geometry.language_row, scale_factor), x, y)
                {
                    return SettingsHit::LanguageDropdown;
                }
                if contains_rect(geometry.density_row, x, y) {
                    return SettingsHit::DensityDropdown;
                }
                if contains_rect(widgets::toggle_rect(geometry.blur, scale_factor), x, y) {
                    return SettingsHit::BlurToggle;
                }
                if contains_rect(geometry.opacity_slider, x, y) {
                    return SettingsHit::OpacitySlider;
                }
                {
                    let (_, up, down) =
                        widgets::spinner_rects(geometry.font_size_row, scale_factor);
                    if contains_rect(up, x, y) {
                        return SettingsHit::FontSizeUp;
                    }
                    if contains_rect(down, x, y) {
                        return SettingsHit::FontSizeDown;
                    }
                }
                if contains_rect(
                    widgets::combobox_rect(geometry.cell_width_mode, scale_factor),
                    x,
                    y,
                ) {
                    return SettingsHit::CellWidthModeDropdown;
                }
                if contains_rect(widgets::toggle_rect(geometry.fetch, scale_factor), x, y) {
                    return SettingsHit::FetchToggle;
                }
                if contains_rect(widgets::toggle_rect(geometry.powerline, scale_factor), x, y) {
                    return SettingsHit::PowerlineToggle;
                }
            },
            NebulaSettingsSection::Profiles => {
                // The import row touches the Shell row at one inclusive
                // boundary in the shared hit helper; give it priority there
                // so a click on its top edge cannot open the dropdown.
                if contains_rect(
                    row_action_rect(geometry.terminal_import, scale_factor, STANDARD_ROW_ACTION_W),
                    x,
                    y,
                ) {
                    return SettingsHit::ImportTerminal;
                }
                if contains_rect(widgets::combobox_rect(geometry.shell, scale_factor), x, y) {
                    return SettingsHit::ShellCycle;
                }
                if contains_rect(geometry.startup_directory_clear, x, y) {
                    return SettingsHit::StartupDirectoryClear;
                }
                if contains_rect(geometry.startup_directory, x, y) {
                    return SettingsHit::StartupDirectory;
                }
                if contains_rect(widgets::combobox_rect(geometry.font, scale_factor), x, y) {
                    return SettingsHit::FontCycle;
                }
                if contains_rect(widgets::toggle_rect(geometry.ghost, scale_factor), x, y) {
                    return SettingsHit::GhostToggle;
                }
                if contains_rect(widgets::combobox_rect(geometry.accept, scale_factor), x, y) {
                    return SettingsHit::AcceptCycle;
                }
                if contains_rect(
                    widgets::combobox_rect(geometry.completion_style, scale_factor),
                    x,
                    y,
                ) {
                    return SettingsHit::CompletionStyleCycle;
                }
                if contains_rect(
                    row_action_rect(geometry.open_config_file, scale_factor, STANDARD_ROW_ACTION_W),
                    x,
                    y,
                ) {
                    return SettingsHit::OpenConfigFile;
                }
            },
            NebulaSettingsSection::Providers => {
                if contains_rect(geometry.provider_add, x, y) {
                    return SettingsHit::ProviderAdd;
                }
                for index in 0..geometry.provider_row_count {
                    let row = (
                        geometry.provider_row0.0,
                        geometry.provider_row0.1 + index as f32 * geometry.provider_row_h,
                        geometry.provider_row0.2,
                        geometry.provider_row_h,
                    );
                    if contains_rect(row, x, y) {
                        if x >= row.0 + row.2 - scale_factor * 76.0 {
                            return SettingsHit::ProviderEnableToggle(index);
                        }
                        return SettingsHit::ProviderRow(index);
                    }
                }
                for (index, field) in geometry.provider_fields.iter().enumerate() {
                    if contains_rect(*field, x, y) {
                        return SettingsHit::ProviderField(index);
                    }
                }
                if contains_rect(
                    widgets::toggle_rect(geometry.provider_codex_goals, scale_factor),
                    x,
                    y,
                ) {
                    return SettingsHit::ProviderCodexGoalsToggle;
                }
                if contains_rect(
                    widgets::toggle_rect(geometry.provider_codex_remote, scale_factor),
                    x,
                    y,
                ) {
                    return SettingsHit::ProviderCodexRemoteToggle;
                }
                if contains_rect(
                    row_action_rect(geometry.provider_codex_apply, scale_factor, 148.0),
                    x,
                    y,
                ) {
                    return SettingsHit::ProviderApplyCodex;
                }
                if contains_rect(geometry.provider_save, x, y) {
                    return SettingsHit::ProviderSave;
                }
                if contains_rect(geometry.provider_test, x, y) {
                    return SettingsHit::ProviderTest;
                }
                if contains_rect(geometry.provider_delete, x, y) {
                    return SettingsHit::ProviderDelete;
                }
            },
            NebulaSettingsSection::Ssh => {
                for index in 0..geometry.ssh_host_count {
                    let row = (
                        geometry.ssh_host_row0.0,
                        geometry.ssh_host_row0.1
                            + index as f32 * (geometry.ssh_host_row_h + geometry.ssh_host_gap),
                        geometry.ssh_host_row0.2,
                        geometry.ssh_host_row_h,
                    );
                    if contains_rect(ssh_host_action_rect(row, scale_factor, 0), x, y) {
                        return SettingsHit::SshHostConnect(index);
                    }
                    if contains_rect(ssh_host_action_rect(row, scale_factor, 1), x, y) {
                        return SettingsHit::SshHostEdit(index);
                    }
                    if contains_rect(ssh_host_action_rect(row, scale_factor, 2), x, y) {
                        return SettingsHit::SshHostDelete(index);
                    }
                    // 三枚图标之后才轮到行本体，顺序反了图标就永远拿不到命中。
                    if contains_rect(row, x, y) {
                        return SettingsHit::SshHostRow(index);
                    }
                }
                if contains_rect(geometry.ssh_add_host, x, y) {
                    return SettingsHit::SshAddHost;
                }
                if contains_rect(
                    row_action_rect(
                        geometry.ssh_import_config,
                        scale_factor,
                        STANDARD_ROW_ACTION_W,
                    ),
                    x,
                    y,
                ) {
                    return SettingsHit::SshImportConfig;
                }
                let (row_x, row_y, row_w, row_h) = geometry.hidden_host_row0;
                for index in 0..geometry.hidden_host_count {
                    let rect = (row_x, row_y + index as f32 * row_h, row_w, row_h);
                    if contains_rect(row_action_rect(rect, scale_factor, 80.0), x, y) {
                        return SettingsHit::RestoreHiddenSsh(index);
                    }
                }
            },
            NebulaSettingsSection::Proxy => {
                if contains_rect(
                    ssh_proxy_mode_control(geometry.ssh_proxy_mode, scale_factor),
                    x,
                    y,
                ) {
                    return SettingsHit::SshProxyModeDropdown;
                }
                if geometry.proxy_pane.mode == crate::ssh_proxy::ProxyMode::Custom {
                    let (protocol, address) =
                        ssh_proxy_manual_controls(geometry.ssh_proxy_expand, scale_factor);
                    if contains_rect(protocol, x, y) {
                        return SettingsHit::SshProxyProtocolDropdown;
                    }
                    if contains_rect(address, x, y) {
                        return SettingsHit::SshProxyInput(0);
                    }
                }
                if contains_rect(ssh_proxy_test_button(geometry.ssh_proxy_test, scale_factor), x, y)
                {
                    return SettingsHit::SshProxyTest;
                }
            },
            NebulaSettingsSection::Interaction => {
                if contains_rect(widgets::toggle_rect(geometry.copy_on_select, scale_factor), x, y)
                {
                    return SettingsHit::CopyOnSelectToggle;
                }
                if contains_rect(widgets::combobox_rect(geometry.tab_reveal, scale_factor), x, y) {
                    return SettingsHit::TabRevealDropdown;
                }
                if contains_rect(
                    widgets::combobox_rect(geometry.new_tab_position, scale_factor),
                    x,
                    y,
                ) {
                    return SettingsHit::NewTabPositionDropdown;
                }
                if contains_rect(widgets::toggle_rect(geometry.panel_resize, scale_factor), x, y) {
                    return SettingsHit::PanelResizeToggle;
                }
                if contains_rect(widgets::toggle_rect(geometry.cjk_bold, scale_factor), x, y) {
                    return SettingsHit::CjkBoldToggle;
                }
            },
            NebulaSettingsSection::Keymap => {
                if contains_rect(geometry.keymap_search, x, y) {
                    return SettingsHit::KeymapSearchField;
                }
                let (row_x, _, row_w, row_h) = geometry.keymap_row0;
                let total: usize =
                    geometry.keymap_pane.visible.iter().map(|count| *count as usize).sum();
                for slot in 0..total.min(geometry.keymap_slot_ys.len()) {
                    let rect = (row_x, geometry.keymap_slot_ys[slot], row_w, row_h);
                    if contains_rect(rect, x, y) {
                        return SettingsHit::KeymapRow(slot);
                    }
                }
                let (row_x, row_y, row_w, row_h) = geometry.keymap_readonly_row0;
                for index in 0..geometry.keymap_pane.readonly_visible as usize {
                    let rect = (row_x, row_y + index as f32 * row_h, row_w, row_h);
                    if contains_rect(rect, x, y) {
                        return SettingsHit::KeymapReadonlyRow(index);
                    }
                }
            },
            NebulaSettingsSection::Advanced => {
                if contains_rect(widgets::toggle_rect(geometry.keep_session, scale_factor), x, y) {
                    return SettingsHit::KeepSessionToggle;
                }
                if contains_rect(widgets::toggle_rect(geometry.restore_session, scale_factor), x, y)
                {
                    return SettingsHit::RestoreSessionToggle;
                }
                if contains_rect(widgets::toggle_rect(geometry.resume_ai, scale_factor), x, y) {
                    return SettingsHit::ResumeAiToggle;
                }
                if contains_rect(widgets::toggle_rect(geometry.tray, scale_factor), x, y) {
                    return SettingsHit::TrayToggle;
                }
                if SHOW_WEBDAV_SYNC_SETTINGS {
                    for (index, rect) in geometry.sync_rows.iter().enumerate() {
                        // 命中整行都算输入框：行左侧是它的 label，点标签聚焦
                        // 输入是 Windows 设置页的惯例。
                        if contains_rect(*rect, x, y) {
                            return SettingsHit::SyncInput(index);
                        }
                    }
                    if contains_rect(
                        widgets::toggle_rect(geometry.sync_auto_pull, scale_factor),
                        x,
                        y,
                    ) {
                        return SettingsHit::SyncAutoPullToggle;
                    }
                    let [push, pull] = sync_button_rects(geometry.sync_actions, scale_factor);
                    if contains_rect(push, x, y) {
                        return SettingsHit::SyncPushButton;
                    }
                    if contains_rect(pull, x, y) {
                        return SettingsHit::SyncPullButton;
                    }
                }
            },
            NebulaSettingsSection::Backup => {
                for (index, rect) in geometry.backup_rows.iter().enumerate() {
                    if contains_rect(*rect, x, y) {
                        return SettingsHit::BackupSelection(index);
                    }
                }
                let [export, restore] = backup_segment_rects(geometry.backup_segment, scale_factor);
                if contains_rect(export, x, y) {
                    return SettingsHit::BackupExport;
                }
                if contains_rect(restore, x, y) {
                    return SettingsHit::BackupRestore;
                }
                if contains_rect(
                    widgets::combobox_rect(geometry.backup_remote_protocol, scale_factor),
                    x,
                    y,
                ) {
                    return SettingsHit::BackupProtocolCycle;
                }
                let field_count = crate::backup_remote::field_count(backup_protocol);
                for (index, rect) in
                    geometry.backup_remote_fields.iter().take(field_count).enumerate()
                {
                    // 命中整行都算输入框：行左侧是它的 label，点标签聚焦
                    // 输入是 Windows 设置页的惯例（与同步行一致）。
                    if contains_rect(*rect, x, y) {
                        return SettingsHit::BackupRemoteField(index);
                    }
                }
                if backup_protocol != crate::backup_remote::BackupProtocol::Off {
                    let actions = backup_remote_actions_rect(&geometry, scale_factor, field_count);
                    let [push, pull] = sync_button_rects(actions, scale_factor);
                    if contains_rect(push, x, y) {
                        return SettingsHit::BackupRemotePush;
                    }
                    if contains_rect(pull, x, y) {
                        return SettingsHit::BackupRemotePull;
                    }
                }
            },
        }
    }

    if contains_rect(geometry.popup, x, y) { SettingsHit::Panel } else { SettingsHit::None }
}
