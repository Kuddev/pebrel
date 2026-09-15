// ---- geometry + hit-testing ----

use crate::display::ui::widgets;
use crate::display::ui::{icons, tokens};
use crate::display::{
    chrome_settings_button_rect, contains_rect, keymap, NebulaSettingsSection, NebulaTheme,
    SizeInfo,
};
use crate::renderer::image::BackgroundImageFit;
use crate::ssh_proxy;

use super::{
    backup_remote_actions_rect, backup_segment_rects, sync_button_rects, sync_input_rect,
    ssh_proxy_expand_control, ssh_proxy_manual_controls, ssh_proxy_mode_control,
    CellWidthMode, KeymapPaneState, NewTabPosition, ProxyPaneState, SettingsDropdown,
    SHOW_BACKUP_SETTINGS, SHOW_WEBDAV_SYNC_SETTINGS, TabRevealMotion,
    ACCEPT_OPTIONS, BACKUP_PROTOCOL_OPTIONS, BACKGROUND_ALIGNMENT_OPTIONS,
    BACKGROUND_FIT_OPTIONS, CELL_WIDTH_MODE_OPTIONS, COMPLETION_STYLE_OPTIONS,
    CURSOR_SHAPE_OPTIONS, DENSITY_OPTIONS, LANGUAGE_OPTIONS, NEW_TAB_POSITION_OPTIONS,
    SSH_PROXY_MODE_OPTIONS, TAB_REVEAL_OPTIONS, MANUAL_PROXY_PROTOCOL_OPTIONS,
};

#[derive(Debug, Clone, Copy)]
pub(super) struct SettingsGeometry {
    pub(super) gear: (f32, f32, f32, f32),
    pub(super) popup: (f32, f32, f32, f32),
    pub(super) sidebar: (f32, f32, f32, f32),
    pub(super) content: (f32, f32, f32, f32),
    /// 中等宽度只保留导航图标，把空间还给正文；窄宽度进一步把设置行改为两层。
    pub(super) compact_nav: bool,
    pub(super) stacked_rows: bool,
    pub(super) nav: [(NebulaSettingsSection, f32, f32, f32, f32); 9],
    /// Navigation group labels occupy the intentional gaps before connection
    /// and system settings, so the rail never contains unexplained whitespace.
    pub(super) nav_groups: [(f32, f32, f32, f32); 2],
    pub(super) options: [(NebulaTheme, f32, f32, f32, f32); 13],
    /// Live terminal preview card at the top of Appearance: configure →
    /// immediately see (font, size, colors, wallpaper opacity, cursor).
    pub(super) preview: (f32, f32, f32, f32),
    pub(super) system_theme: (f32, f32, f32, f32),
    pub(super) shell: (f32, f32, f32, f32),
    pub(super) startup_directory: (f32, f32, f32, f32),
    pub(super) startup_directory_clear: (f32, f32, f32, f32),
    pub(super) font: (f32, f32, f32, f32),
    /// "字号" spinner row; the value box + steppers derive via `widgets`.
    pub(super) font_size_row: (f32, f32, f32, f32),
    pub(super) cell_width_mode: (f32, f32, f32, f32),
    pub(super) fetch: (f32, f32, f32, f32),
    pub(super) powerline: (f32, f32, f32, f32),
    pub(super) ghost: (f32, f32, f32, f32),
    pub(super) accept: (f32, f32, f32, f32),
    pub(super) completion_style: (f32, f32, f32, f32),
    pub(super) open_config_file: (f32, f32, f32, f32),
    pub(super) terminal_import: (f32, f32, f32, f32),
    pub(super) ssh_host_row0: (f32, f32, f32, f32),
    pub(super) ssh_host_count: usize,
    pub(super) ssh_host_row_h: f32,
    pub(super) ssh_host_gap: f32,
    pub(super) ssh_add_host: (f32, f32, f32, f32),
    pub(super) ssh_import_config: (f32, f32, f32, f32),
    pub(super) hidden_host_row0: (f32, f32, f32, f32),
    pub(super) hidden_host_count: usize,
    /// Full-width "窗口透明度" row and its draggable track.
    pub(super) language_row: (f32, f32, f32, f32),
    pub(super) density_row: (f32, f32, f32, f32),
    pub(super) opacity_row: (f32, f32, f32, f32),
    pub(super) opacity_slider: (f32, f32, f32, f32),
    /// 「窗口背景模糊」开关，紧贴透明度滑块——它只在透明时才看得出效果。
    pub(super) blur: (f32, f32, f32, f32),
    /// Cursor group: shape combobox row + blink toggle row.
    pub(super) cursor_shape_row: (f32, f32, f32, f32),
    pub(super) cursor_blink_row: (f32, f32, f32, f32),
    pub(super) background: (f32, f32, f32, f32),
    pub(super) background_image: (f32, f32, f32, f32),
    pub(super) background_image_clear: (f32, f32, f32, f32),
    pub(super) background_image_fit: (f32, f32, f32, f32),
    pub(super) background_image_alignment: (f32, f32, f32, f32),
    pub(super) background_image_cover_chrome: (f32, f32, f32, f32),
    pub(super) background_image_opacity_row: (f32, f32, f32, f32),
    pub(super) background_image_opacity_slider: (f32, f32, f32, f32),
    /// 交互: copy-on-select toggle row.
    pub(super) copy_on_select: (f32, f32, f32, f32),
    /// 交互·拖拽调节侧栏的开关行。
    pub(super) panel_resize: (f32, f32, f32, f32),
    /// 交互: CJK 粗体策略 toggle row.
    pub(super) cjk_bold: (f32, f32, f32, f32),
    pub(super) tab_reveal: (f32, f32, f32, f32),
    pub(super) new_tab_position: (f32, f32, f32, f32),
    pub(super) reset: (f32, f32, f32, f32),
    /// Top edge of the scrollable content viewport (just below the fixed
    /// header band); everything above it never scrolls.
    pub(super) content_top: f32,
    /// Total designed content height per section (scaled px, measured from
    /// `content_top`). `max_scroll = (height - viewport).max(0)`.
    pub(super) appearance_h: f32,
    pub(super) profiles_h: f32,
    pub(super) providers_h: f32,
    pub(super) interaction_h: f32,
    pub(super) keymap_h: f32,
    /// 按键映射页动态几何：搜索框 / 冲突提示条 / 分组行。`keymap_slot_ys`
    /// 是过滤后各可见行的 y（前「可见总数」个有效）；`keymap_title_ys` 是
    /// 各组标题的 y（NaN = 该组被滤空；下标 5 = 固定快捷键组）。
    pub(super) keymap_search: (f32, f32, f32, f32),
    pub(super) keymap_note: (f32, f32, f32, f32),
    pub(super) keymap_pane: KeymapPaneState,
    pub(super) keymap_slot_ys: [f32; 32],
    pub(super) keymap_title_ys: [f32; 6],
    pub(super) keymap_hint_y: f32,
    /// 行矩形模板：x/w/h 通用，可见槽 `i` 的 y 在 `keymap_slot_ys[i]`。
    pub(super) keymap_row0: (f32, f32, f32, f32),
    /// First row of the read-only shortcut group below the editable block.
    pub(super) keymap_readonly_row0: (f32, f32, f32, f32),
    pub(super) keymap_row_h: f32,
    pub(super) advanced_h: f32,
    pub(super) ssh_h: f32,
    pub(super) provider_add: (f32, f32, f32, f32),
    pub(super) provider_row0: (f32, f32, f32, f32),
    pub(super) provider_row_h: f32,
    pub(super) provider_row_count: usize,
    pub(super) provider_fields: [(f32, f32, f32, f32); 6],
    pub(super) provider_codex_goals: (f32, f32, f32, f32),
    pub(super) provider_codex_remote: (f32, f32, f32, f32),
    pub(super) provider_codex_apply: (f32, f32, f32, f32),
    pub(super) provider_save: (f32, f32, f32, f32),
    pub(super) provider_test: (f32, f32, f32, f32),
    pub(super) provider_delete: (f32, f32, f32, f32),
    pub(super) proxy_h: f32,
    pub(super) keep_session: (f32, f32, f32, f32),
    /// 高级·会话：启动时恢复上次的标签（崩溃/强杀后同样走这条路）。
    pub(super) restore_session: (f32, f32, f32, f32),
    /// 高级·会话：冷恢复自动接续 AI 对话。
    pub(super) resume_ai: (f32, f32, f32, f32),
    /// 高级：常驻系统托盘图标。
    pub(super) tray: (f32, f32, f32, f32),
    /// 网络页：主模式仍是下拉框；指定代理分支按 HTML 原型排成扫描标题、
    /// 一张连续列表卡、选中项展开、绕过列表与每主机覆盖。
    pub(super) ssh_proxy_mode: (f32, f32, f32, f32),
    pub(super) ssh_proxy_scan_head: (f32, f32, f32, f32),
    pub(super) ssh_proxy_scan_button: (f32, f32, f32, f32),
    pub(super) ssh_proxy_list: (f32, f32, f32, f32),
    pub(super) ssh_proxy_found_row0: (f32, f32, f32, f32),
    pub(super) ssh_proxy_other_rows: [(f32, f32, f32, f32); 3],
    pub(super) ssh_proxy_expand: (f32, f32, f32, f32),
    pub(super) ssh_proxy_bypass: (f32, f32, f32, f32),
    pub(super) ssh_proxy_test: (f32, f32, f32, f32),
    pub(super) ssh_proxy_override_row0: (f32, f32, f32, f32),
    pub(super) ssh_proxy_inherit: (f32, f32, f32, f32),
    pub(super) proxy_pane: ProxyPaneState,
    pub(super) sync_rows: [(f32, f32, f32, f32); 4],
    pub(super) sync_auto_pull: (f32, f32, f32, f32),
    pub(super) sync_actions: (f32, f32, f32, f32),
    /// Backups follow the HTML prototype: an always-visible auto-backup card,
    /// a compact export/restore segmented control, then grouped rows.
    pub(super) backup_auto: (f32, f32, f32, f32),
    pub(super) backup_segment: (f32, f32, f32, f32),
    pub(super) backup_groups: [(f32, f32, f32, f32); 4],
    pub(super) backup_rows: [(f32, f32, f32, f32); 9],
    pub(super) backup_actions: (f32, f32, f32, f32),
    /// 远程备份组：协议下拉行、5 个输入槽行（按协议裁剪可见数）、动作行。
    pub(super) backup_remote_protocol: (f32, f32, f32, f32),
    pub(super) backup_remote_fields: [(f32, f32, f32, f32); 5],
    pub(super) backup_remote_actions: (f32, f32, f32, f32),
    pub(super) backup_h: f32,
}

/// Scrollable-content viewport height for the Settings tab.
pub(super) fn settings_viewport_h(popup_h: f32, scale_factor: f32) -> f32 {
    popup_h - 72.0 * scale_factor
}

pub(super) fn advanced_content_end(advanced_y0: f32, sync_y0: f32, row_h: f32) -> f32 {
    if SHOW_WEBDAV_SYNC_SETTINGS { sync_y0 + 7.0 * row_h } else { advanced_y0 + 4.0 * row_h }
}

/// Max scroll offset for `section` at the current window size. The input
/// layer clamps its accumulated wheel delta with this. Dropdown popups float
/// over rows, so they never change a section's content height.
pub(crate) fn settings_max_scroll(
    size_info: &SizeInfo,
    scale_factor: f32,
    area: (f32, f32, f32, f32),
    section: NebulaSettingsSection,
    hidden_host_count: usize,
    ssh_host_count: usize,
    density: crate::display::ui::tokens::Density,
    proxy: ProxyPaneState,
    keymap_pane: KeymapPaneState,
    provider_count: usize,
) -> f32 {
    let mut geometry = settings_geometry(
        size_info,
        scale_factor,
        area,
        0.0,
        hidden_host_count,
        ssh_host_count,
        density,
        proxy,
        keymap_pane,
    );
    fit_provider_rows(&mut geometry, provider_count);
    let (_, _, _, ph) = geometry.popup;
    let content_h = match section {
        NebulaSettingsSection::Appearance => geometry.appearance_h,
        NebulaSettingsSection::Profiles => geometry.profiles_h,
        NebulaSettingsSection::Providers => geometry.providers_h,
        NebulaSettingsSection::Ssh => geometry.ssh_h,
        NebulaSettingsSection::Proxy => geometry.proxy_h,
        NebulaSettingsSection::Interaction => geometry.interaction_h,
        NebulaSettingsSection::Keymap => geometry.keymap_h,
        NebulaSettingsSection::Advanced => geometry.advanced_h,
        NebulaSettingsSection::Backup => geometry.backup_h,
    };
    (content_h - settings_viewport_h(ph, scale_factor)).max(0.0)
}

pub(super) fn settings_geometry(
    size_info: &SizeInfo,
    scale_factor: f32,
    area: (f32, f32, f32, f32),
    scroll: f32,
    hidden_host_count: usize,
    ssh_host_count: usize,
    density: crate::display::ui::tokens::Density,
    proxy: ProxyPaneState,
    keymap_pane: KeymapPaneState,
) -> SettingsGeometry {
    let s = |v: f32| v * scale_factor;
    let gear = chrome_settings_button_rect(size_info, scale_factor);

    // The settings surface is the active tab's content card. Keeping the
    // geometry rooted in that card makes sidebar/drawer animations and DPI
    // changes follow the exact same bounds as terminal and document tabs.
    let (popup_x, popup_y, popup_w, popup_h) = area;
    // 断点依据设置卡片的逻辑宽度，不依据物理像素或字体大小，避免 DPI 缩放
    // 意外改变交互布局。
    let logical_popup_w = popup_w / scale_factor.max(f32::EPSILON);
    let compact_nav = logical_popup_w < 900.0;
    let stacked_rows = logical_popup_w < 650.0;
    // Match the reference settings shell when wide; an icon-only rail gives
    // the content a usable minimum width at medium and narrow sizes.
    let sidebar_w =
        if compact_nav { s(64.0).min(popup_w * 0.30) } else { s(196.0).min(popup_w * 0.30) };
    let sidebar = (popup_x, popup_y, sidebar_w, popup_h);
    let content_gap = if compact_nav { s(8.0) } else { s(16.0) };
    let content_x = popup_x + sidebar_w + content_gap;
    let content_w = (popup_w - sidebar_w - content_gap).max(s(1.0));
    let content = (content_x, popup_y, content_w, popup_h);

    // The header band (big section title) is fixed; everything below it
    // scrolls by `scroll` px. `at` maps a design-space Y to screen space.
    let content_top = popup_y + s(72.0);
    let at = |design_y: f32| popup_y + s(design_y) - scroll;

    // ---- vertical rhythm (design px from the popup top) ----
    // Mirrors the HTML design sheet's breathing room: a group title hangs
    // 42px above its first row (title + 16px gap); rows inside a group are
    // CONTIGUOUS — one hairline frame around the block, hairline separators
    // between rows — and a finished group leaves 32px before the next title,
    // so `74 = 32 (section gap) + 42 (hanging title)`.
    // 行高与组间距按密度降档：紧凑用阶梯上既有的 COMPACT_ROW，组间距
    // 随行高等量收窄，不引入新数值。
    let row_advance = crate::display::ui::tokens::control::settings_row(density);
    let row_advance = if stacked_rows {
        // 两层设置行必须同时容纳标签和通栏控件；密度 token 仍作为下限，
        // 让紧凑外观与既有主题保持一致。
        row_advance.max(72.0)
    } else {
        row_advance
    };
    #[allow(non_snake_case)]
    let ROW_H: f32 = row_advance;
    #[allow(non_snake_case)]
    let GROUP_ADVANCE: f32 = 74.0 - (44.0 - row_advance);
    // 预览卡设计高度：两行示例文本 + 光标演示的呼吸空间。
    const PREVIEW_H: f32 = 150.0;
    let content_inset = s(if compact_nav { 16.0 } else { 24.0 });

    // Live preview leads the page: configure → see it immediately.
    let preview_y0 = 146.0;
    let preview = (
        content_x + content_inset,
        at(preview_y0),
        (content_w - 2.0 * content_inset).max(s(1.0)),
        s(PREVIEW_H),
    );

    // 主题卡宽屏四列、窄屏两列；实际行数继续参与后续区块的 Y 坐标计算，
    // 确保绘制、命中与滚动测试同步移动。
    let card_gap = s(20.0);
    let card_columns = if stacked_rows { 2.0 } else { 4.0 };
    let card_inner_w = (content_w - 2.0 * content_inset).max(s(1.0));
    let card_w =
        ((card_inner_w - (card_columns - 1.0) * card_gap) / card_columns).max(s(1.0)).min(s(170.0));
    let card_h = s(64.0);
    let card_y0 = preview_y0 + PREVIEW_H + GROUP_ADVANCE;
    let card_x = content_x + content_inset;
    let card_row_pitch = card_h + s(48.0);
    let card = |i: f32| card_x + (i % card_columns) * (card_w + card_gap);
    let card_slot_y = |i: f32| at(card_y0) + (i / card_columns).floor() * card_row_pitch;

    let row_x = content_x + content_inset;
    let row_w = (content_w - 2.0 * content_inset).max(s(1.0));
    let row_h = s(ROW_H);

    // Appearance: preview, cards, colors, cursor and interface groups.
    let card_rows = (nebula_settings::ThemeName::BUILTIN.len() as f32 / card_columns).ceil();
    let system_theme_y0 = card_y0 + card_rows * (64.0 + 48.0) + GROUP_ADVANCE;
    let color_y0 = system_theme_y0 + ROW_H + GROUP_ADVANCE;
    // Background-image controls: path, stretch,
    // alignment and an independent image-opacity slider.
    let background_image_y0 = color_y0 + ROW_H;
    let background_image_fit_y0 = background_image_y0 + ROW_H;
    let background_image_alignment_y0 = background_image_fit_y0 + ROW_H;
    let background_image_opacity_y0 = background_image_alignment_y0 + ROW_H;
    let background_image_cover_chrome_y0 = background_image_opacity_y0 + ROW_H;
    // 光标组：形状下拉 + 闪烁开关。
    let cursor_y0 = color_y0 + 6.0 * ROW_H + GROUP_ADVANCE;
    let iface_y0 = cursor_y0 + 2.0 * ROW_H + GROUP_ADVANCE;
    let density_y0 = iface_y0 + ROW_H;
    let opacity_y0 = density_y0 + ROW_H;
    // 模糊开关跟在透明度后面：它修饰的正是透明度透出来的那层东西，隔开就
    // 读不出这层因果了。
    let blur_y0 = opacity_y0 + ROW_H;
    // Terminal presentation belongs to Appearance: font sizing, the startup
    // welcome and the prompt decoration all change what a new terminal looks
    // like, while Profiles remains focused on what executable is launched.
    let terminal_appearance_y0 = blur_y0 + ROW_H + GROUP_ADVANCE;
    let appearance_h = s(terminal_appearance_y0 + 4.0 * ROW_H + 32.0 - 72.0);
    // 宽命中区包住细轨道，拖拽时无需精确点中 4px 线条。
    let opacity_row = (row_x, at(opacity_y0), row_w, row_h);
    let slider_x = if stacked_rows { row_x + s(16.0) } else { row_x + row_w - s(212.0) };
    let slider_w = if stacked_rows {
        (row_w - s(32.0)).max(s(24.0))
    } else {
        s(188.0).min(row_w * 0.42).max(s(96.0).min(row_w.max(s(1.0))))
    };
    let slider_y = if stacked_rows { s(34.0) } else { s(4.0) };
    let opacity_slider = (slider_x, at(opacity_y0) + slider_y, slider_w, s(36.0));
    let background_image_opacity_row = (row_x, at(background_image_opacity_y0), row_w, row_h);
    let background_image_opacity_slider =
        (slider_x, at(background_image_opacity_y0) + slider_y, slider_w, s(36.0));

    // Sidebar navigation rows. The rects line up with the active-row
    // highlight drawn while rendering. The reference uses 32px rows with a
    // 2px gap; the icon and label carry the internal breathing instead.
    let nav_x = popup_x + s(10.0);
    let nav_w = sidebar_w - s(20.0);
    let nav_h = s(32.0);
    let nav_gap = s(2.0);
    let nav_y0 = popup_y + s(88.0);
    let nav_slot = |i: f32| nav_y0 + i * (nav_h + nav_gap);
    let connections_group_y = nav_slot(5.0);
    let ssh_nav_y = if compact_nav { nav_slot(5.0) } else { connections_group_y + s(24.0) };
    let proxy_nav_y = ssh_nav_y + nav_h + nav_gap;
    let system_group_y = proxy_nav_y + nav_h + nav_gap;
    let advanced_nav_y = if compact_nav { nav_slot(7.0) } else { system_group_y + s(24.0) };
    let backup_nav_y = advanced_nav_y + nav_h + nav_gap;
    let nav = [
        (NebulaSettingsSection::Appearance, nav_x, nav_slot(0.0), nav_w, nav_h),
        (NebulaSettingsSection::Profiles, nav_x, nav_slot(1.0), nav_w, nav_h),
        (NebulaSettingsSection::Providers, nav_x, nav_slot(2.0), nav_w, nav_h),
        (NebulaSettingsSection::Interaction, nav_x, nav_slot(3.0), nav_w, nav_h),
        (NebulaSettingsSection::Keymap, nav_x, nav_slot(4.0), nav_w, nav_h),
        (NebulaSettingsSection::Ssh, nav_x, ssh_nav_y, nav_w, nav_h),
        (NebulaSettingsSection::Proxy, nav_x, proxy_nav_y, nav_w, nav_h),
        (NebulaSettingsSection::Advanced, nav_x, advanced_nav_y, nav_w, nav_h),
        (NebulaSettingsSection::Backup, nav_x, backup_nav_y, nav_w, nav_h),
    ];
    let nav_groups =
        [(nav_x, connections_group_y, nav_w, s(24.0)), (nav_x, system_group_y, nav_w, s(24.0))];

    // Profiles: dropdown popups FLOAT over later rows (Windows 11 combobox),
    // so every row keeps a fixed offset — no picker shove, no scroll jumps.
    let shell_y0 = 146.0;
    // Import belongs to the Terminal group, immediately below the default
    // Shell row, instead of being buried in the configuration section.
    let terminal_import_y0 = shell_y0 + ROW_H;
    let startup_directory_y0 = terminal_import_y0 + ROW_H;
    let font_y0 = startup_directory_y0 + ROW_H;
    let ghost_y0 = font_y0 + ROW_H + GROUP_ADVANCE;
    let open_y0 = ghost_y0 + 3.0 * ROW_H + GROUP_ADVANCE;
    let profiles_h = s(open_y0 + ROW_H + 32.0 - 72.0);

    // AI providers intentionally use a denser list + editor flow inspired by
    // CC Switch: the left-side list keeps switching cheap, while the active
    // provider exposes base URL/model/key fields without a second window.
    const PROVIDER_ROW_H: f32 = 58.0;
    let providers_y0 = 146.0;
    let provider_title_y = at(providers_y0 - 48.0);
    let provider_add_w = s(112.0).min(row_w * 0.42).max(s(34.0));
    let provider_add = (
        row_x + row_w - provider_add_w,
        widgets::centered_y(provider_title_y, s(32.0), s(30.0)),
        provider_add_w,
        s(30.0),
    );
    let provider_row0_y = providers_y0;
    let provider_row0 = (row_x, at(provider_row0_y), row_w, s(PROVIDER_ROW_H));
    let provider_form_y0 = provider_row0_y + PROVIDER_ROW_H * 6.0 + GROUP_ADVANCE;
    let provider_field = |i: f32| (row_x, at(provider_form_y0 + i * ROW_H), row_w, row_h);
    let provider_fields = [
        provider_field(0.0),
        provider_field(1.0),
        provider_field(2.0),
        provider_field(3.0),
        provider_field(4.0),
        provider_field(5.0),
    ];
    let provider_codex_goals = provider_field(6.0);
    let provider_codex_remote = provider_field(7.0);
    let provider_codex_apply = provider_field(8.0);
    let provider_actions_y = provider_form_y0 + 9.0 * ROW_H + GROUP_ADVANCE;
    let provider_action_w = s(112.0).min(row_w * 0.30).max(s(34.0));
    let provider_save =
        (row_x + row_w - provider_action_w, at(provider_actions_y), provider_action_w, row_h);
    let provider_test = (
        provider_save.0 - provider_action_w - s(10.0),
        at(provider_actions_y),
        provider_action_w,
        row_h,
    );
    let provider_delete = (row_x, at(provider_actions_y), provider_action_w, row_h);
    let providers_h = s(provider_actions_y + ROW_H + 32.0 - 72.0);

    // 交互: 剪贴板行为、标签行为（展开、新标签位置）与拖拽调节一组，
    // 文本渲染（CJK 粗体策略）另一组。
    let interaction_y0 = 146.0;
    let cjk_bold_y0 = interaction_y0 + ROW_H * 4.0 + GROUP_ADVANCE;
    let interaction_h = s(cjk_bold_y0 + ROW_H + 32.0 - 72.0);
    let copy_on_select = (row_x, at(interaction_y0), row_w, row_h);
    let tab_reveal = (row_x, at(interaction_y0 + ROW_H), row_w, row_h);
    let new_tab_position = (row_x, at(interaction_y0 + ROW_H * 2.0), row_w, row_h);
    let panel_resize = (row_x, at(interaction_y0 + ROW_H * 3.0), row_w, row_h);
    let cjk_bold = (row_x, at(cjk_bold_y0), row_w, row_h);

    // 按键映射没有普通设置页的首个"悬挂分组标题"，搜索框直接占用那块
    // 空间；继续沿用 146px 起点会无端多出 42px 空白。
    let keymap_y0 = 104.0;
    let keymap_search = (row_x, at(keymap_y0), row_w, s(34.0));
    let mut keymap_cursor = keymap_y0 + 34.0 + 12.0;
    let keymap_note = (row_x, at(keymap_cursor), row_w, s(50.0));
    if keymap_pane.clash {
        keymap_cursor += 50.0 + 12.0;
    }
    let mut keymap_slot_ys = [0.0f32; 32];
    let mut keymap_title_ys = [f32::NAN; 6];
    let mut keymap_slot = 0usize;
    let mut keymap_first_group = true;
    let keymap_rows_top = keymap_cursor + 42.0;
    for group in 0..keymap::GROUPS.len() {
        let rows = keymap_pane.visible[group] as usize;
        if rows == 0 {
            continue;
        }
        keymap_cursor += if keymap_first_group { 42.0 } else { GROUP_ADVANCE };
        keymap_first_group = false;
        keymap_title_ys[group] = at(keymap_cursor - 42.0);
        for _ in 0..rows {
            if keymap_slot < keymap_slot_ys.len() {
                keymap_slot_ys[keymap_slot] = at(keymap_cursor);
            }
            keymap_slot += 1;
            keymap_cursor += ROW_H;
        }
    }
    let keymap_row0 = (
        row_x,
        if keymap_slot > 0 { keymap_slot_ys[0] } else { at(keymap_rows_top) },
        row_w,
        row_h,
    );
    if keymap_pane.readonly_visible > 0 {
        keymap_cursor += if keymap_first_group { 42.0 } else { GROUP_ADVANCE };
        keymap_title_ys[5] = at(keymap_cursor - 42.0);
    }
    let keymap_readonly_row0 = (row_x, at(keymap_cursor), row_w, row_h);
    keymap_cursor += keymap_pane.readonly_visible as f32 * ROW_H;
    let keymap_hint_y = at(keymap_cursor + 10.0);
    let keymap_h = s(keymap_cursor + 10.0 + 26.0 + 32.0 - 72.0);

    // Advanced: session residency, then the gated WebDAV sync group (spec 003).
    // 隐藏期间同步几何保留，方便后续继续完善，但页面高度只计算可见内容。
    let advanced_y0 = 146.0;
    let keep_session = (row_x, at(advanced_y0), row_w, row_h);
    let restore_session = (row_x, at(advanced_y0 + ROW_H), row_w, row_h);
    let resume_ai = (row_x, at(advanced_y0 + ROW_H * 2.0), row_w, row_h);
    let tray = (row_x, at(advanced_y0 + ROW_H * 3.0), row_w, row_h);
    let sync_y0 = advanced_y0 + ROW_H * 4.0 + GROUP_ADVANCE;
    let sync_row = |i: f32| (row_x, at(sync_y0 + i * ROW_H), row_w, row_h);
    let sync_rows = [sync_row(0.0), sync_row(1.0), sync_row(2.0), sync_row(3.0)];
    let sync_auto_pull = sync_row(4.0);
    let sync_actions = sync_row(5.0);
    let advanced_h = s(advanced_content_end(advanced_y0, sync_y0, ROW_H) + 32.0 - 72.0);

    // SSH 独立页：标题栏直接提供添加动作，正文只保留紧凑主机卡片、导入与
    // 隐藏主机恢复。代理拥有独立页面，不能再参与 SSH 页的高度或滚动计算。
    const SSH_HOST_ROW_H: f32 = 58.0;
    const SSH_HOST_GAP: f32 = 8.0;
    let ssh_host_y0 = 146.0;
    let ssh_host_row = |i: f32| {
        (row_x, at(ssh_host_y0 + i * (SSH_HOST_ROW_H + SSH_HOST_GAP)), row_w, s(SSH_HOST_ROW_H))
    };
    let ssh_host_row0 = ssh_host_row(0.0);
    let ssh_import_y0 = ssh_host_y0 + ssh_host_count as f32 * (SSH_HOST_ROW_H + SSH_HOST_GAP)
        - if ssh_host_count > 0 { SSH_HOST_GAP } else { 0.0 }
        + 16.0;
    let title_bar_y = at(ssh_host_y0 - 48.0);
    let title_bar_h = s(32.0);
    let add_button_w = s(112.0).min(row_w * 0.42).max(s(34.0));
    let add_button_h = s(30.0);
    let ssh_add_host = (
        row_x + row_w - add_button_w,
        widgets::centered_y(title_bar_y, title_bar_h, add_button_h),
        add_button_w,
        add_button_h,
    );
    let ssh_import_config = (row_x, at(ssh_import_y0), row_w, row_h);
    let hidden_y0 = ssh_import_y0 + ROW_H + GROUP_ADVANCE;
    let ssh_end = if hidden_host_count == 0 {
        ssh_import_y0 + ROW_H
    } else {
        hidden_y0 + hidden_host_count as f32 * ROW_H
    };
    let ssh_h = s(ssh_end + 32.0 - 72.0);

    // 出网测试是网络页的第一项，用户打开页面即可验证当前设置；三种模式
    // 的选择与地址控件排在测试横幅之后，避免把功能入口埋在页面最底部。
    let proxy_test_y = 146.0;
    let ssh_proxy_test = (row_x, at(proxy_test_y), row_w, s(54.0));
    // 网络代理只保留一张紧凑设置卡：所有模式都有方式行；自定义代理再
    // 展开地址与直连地址两行。旧跳板/命令只保留兼容解析，不再暴露第二套入口。
    let proxy_y0 = proxy_test_y + 54.0 + 18.0;
    let ssh_proxy_mode = (row_x, at(proxy_y0), row_w, row_h);
    let pane_y0 = proxy_y0 + ROW_H;
    let ssh_proxy_expand = (row_x, at(pane_y0), row_w, row_h);
    let bypass_y = pane_y0 + ROW_H;
    let ssh_proxy_bypass = (row_x, at(bypass_y), row_w, row_h);
    let pane_end =
        if proxy.mode == ssh_proxy::ProxyMode::Custom { pane_y0 + ROW_H } else { pane_y0 };
    // 旧扫描/跳板/覆盖能力只保留在兼容后端；这些零尺寸几何用于渐进收口
    // 内部结构，任何绘制与命中都不再消费它们。
    let hidden_proxy_rect = (row_x, at(pane_y0), 0.0, 0.0);
    let ssh_proxy_scan_head = hidden_proxy_rect;
    let ssh_proxy_scan_button = hidden_proxy_rect;
    let ssh_proxy_list = hidden_proxy_rect;
    let ssh_proxy_found_row0 = hidden_proxy_rect;
    let ssh_proxy_other_rows = [hidden_proxy_rect; 3];
    let ssh_proxy_override_row0 = hidden_proxy_rect;
    let ssh_proxy_inherit = hidden_proxy_rect;
    let proxy_h = s(pane_end + 32.0 - 72.0);

    // Backup prototype: automatic-backup summary, export/restore segmented
    // action, then one grouped manifest card. Backup rows are taller because
    // every category carries a description and size, unlike the generic
    // single-line setting rows above.
    const BACKUP_ROW_H: f32 = 52.0;
    let backup_auto = (row_x, at(126.0), row_w, s(68.0));
    let backup_segment = (row_x, at(212.0), s(300.0).min(row_w), s(38.0));
    let backup_group = |y: f32| (row_x, at(y), row_w, s(24.0));
    let backup_groups =
        [backup_group(300.0), backup_group(480.0), backup_group(556.0), backup_group(632.0)];
    let backup_row = |y: f32| (row_x, at(y), row_w, s(BACKUP_ROW_H));
    let backup_rows = [
        backup_row(324.0), // appearance
        backup_row(376.0), // config
        backup_row(504.0), // SSH
        backup_row(428.0), // sync
        backup_row(580.0), // assistant
        backup_row(656.0), // session
        backup_row(708.0), // directory history
        backup_row(760.0), // command history
        backup_row(812.0), // fonts
    ];
    let backup_actions = backup_segment;
    // 远程备份组接在清单卡之后（清单行位一个不动）：协议下拉 + 至多 5 个
    // 输入行 + 动作行。行位按字段最多的协议（S3=5）预留；字段更少的协议
    // 隐藏尾部行，动作行经 `backup_remote_actions_rect` 上移贴住可见行。
    let backup_remote_y0 = 944.0;
    let backup_remote_protocol = (row_x, at(backup_remote_y0), row_w, row_h);
    let backup_remote_field =
        |index: usize| (row_x, at(backup_remote_y0 + (1.0 + index as f32) * ROW_H), row_w, row_h);
    let backup_remote_fields = [
        backup_remote_field(0),
        backup_remote_field(1),
        backup_remote_field(2),
        backup_remote_field(3),
        backup_remote_field(4),
    ];
    let backup_remote_actions_y = backup_remote_y0 + 6.0 * ROW_H + 12.0;
    let backup_remote_actions = (row_x, at(backup_remote_actions_y), s(300.0).min(row_w), s(38.0));
    let backup_h = s(backup_remote_actions_y + 38.0 + 64.0 - 72.0);

    SettingsGeometry {
        gear,
        popup: (popup_x, popup_y, popup_w, popup_h),
        sidebar,
        content,
        compact_nav,
        stacked_rows,
        nav,
        nav_groups,
        options: std::array::from_fn(|index| {
            let name = nebula_settings::ThemeName::BUILTIN[index];
            let theme = NebulaTheme::from_prompt_name(name.prompt_name()).unwrap();
            (theme, card(index as f32), card_slot_y(index as f32), card_w, card_h)
        }),
        preview,
        system_theme: (row_x, at(system_theme_y0), row_w, row_h),
        background: (row_x, at(color_y0), row_w, row_h),
        background_image: (row_x, at(background_image_y0), row_w, row_h),
        background_image_clear: (
            row_x + row_w - s(48.0),
            at(background_image_y0) + if stacked_rows { s(33.0) } else { s(5.0) },
            s(36.0),
            s(34.0),
        ),
        background_image_fit: (row_x, at(background_image_fit_y0), row_w, row_h),
        background_image_alignment: (row_x, at(background_image_alignment_y0), row_w, row_h),
        background_image_cover_chrome: (row_x, at(background_image_cover_chrome_y0), row_w, row_h),
        background_image_opacity_row,
        background_image_opacity_slider,
        cursor_shape_row: (row_x, at(cursor_y0), row_w, row_h),
        cursor_blink_row: (row_x, at(cursor_y0 + ROW_H), row_w, row_h),
        language_row: (row_x, at(iface_y0), row_w, row_h),
        density_row: (row_x, at(density_y0), row_w, row_h),
        opacity_row,
        opacity_slider,
        blur: (row_x, at(blur_y0), row_w, row_h),
        shell: (row_x, at(shell_y0), row_w, row_h),
        startup_directory: (row_x, at(startup_directory_y0), row_w, row_h),
        startup_directory_clear: (
            row_x + row_w - s(82.0),
            at(startup_directory_y0) + if stacked_rows { s(33.0) } else { s(5.0) },
            s(72.0),
            s(34.0),
        ),
        font: (row_x, at(font_y0), row_w, row_h),
        font_size_row: (row_x, at(terminal_appearance_y0), row_w, row_h),
        cell_width_mode: (row_x, at(terminal_appearance_y0 + ROW_H), row_w, row_h),
        fetch: (row_x, at(terminal_appearance_y0 + 2.0 * ROW_H), row_w, row_h),
        powerline: (row_x, at(terminal_appearance_y0 + 3.0 * ROW_H), row_w, row_h),
        ghost: (row_x, at(ghost_y0), row_w, row_h),
        accept: (row_x, at(ghost_y0 + ROW_H), row_w, row_h),
        completion_style: (row_x, at(ghost_y0 + 2.0 * ROW_H), row_w, row_h),
        open_config_file: (row_x, at(open_y0), row_w, row_h),
        terminal_import: (row_x, at(terminal_import_y0), row_w, row_h),
        ssh_host_row0,
        ssh_host_count,
        ssh_host_row_h: s(SSH_HOST_ROW_H),
        ssh_host_gap: s(SSH_HOST_GAP),
        ssh_add_host,
        ssh_import_config,
        hidden_host_row0: (row_x, at(hidden_y0), row_w, row_h),
        hidden_host_count,
        copy_on_select,
        panel_resize,
        cjk_bold,
        tab_reveal,
        new_tab_position,
        reset: if stacked_rows {
            (popup_x + popup_w - s(58.0), popup_y + s(24.0), s(38.0), s(38.0))
        } else {
            (popup_x + popup_w - s(170.0), popup_y + s(24.0), s(150.0), s(42.0))
        },
        content_top,
        appearance_h,
        profiles_h,
        providers_h,
        ssh_h,
        provider_add,
        provider_row0,
        provider_row_h: s(PROVIDER_ROW_H),
        provider_row_count: 6,
        provider_fields,
        provider_codex_goals,
        provider_codex_remote,
        provider_codex_apply,
        provider_save,
        provider_test,
        provider_delete,
        proxy_h,
        interaction_h,
        keymap_h,
        keymap_search,
        keymap_note,
        keymap_pane,
        keymap_slot_ys,
        keymap_title_ys,
        keymap_hint_y,
        keymap_row0,
        keymap_readonly_row0,
        keymap_row_h: row_h,
        advanced_h,
        keep_session,
        restore_session,
        resume_ai,
        tray,
        ssh_proxy_mode,
        ssh_proxy_scan_head,
        ssh_proxy_scan_button,
        ssh_proxy_list,
        ssh_proxy_found_row0,
        ssh_proxy_other_rows,
        ssh_proxy_expand,
        ssh_proxy_bypass,
        ssh_proxy_test,
        ssh_proxy_override_row0,
        ssh_proxy_inherit,
        proxy_pane: proxy,
        sync_rows,
        sync_auto_pull,
        sync_actions,
        backup_auto,
        backup_segment,
        backup_groups,
        backup_rows,
        backup_actions,
        backup_remote_protocol,
        backup_remote_fields,
        backup_remote_actions,
        backup_h,
    }
}

/// Provider rows are backed by the persisted collection, while the rest of
/// Settings geometry is static. Moving the editor block here keeps hit tests,
/// rendering and scroll bounds on the same calculation without threading a
/// provider count through every unrelated geometry helper.
pub(super) fn fit_provider_rows(geometry: &mut SettingsGeometry, provider_count: usize) {
    let old_count = geometry.provider_row_count;
    let delta = provider_count as f32 - old_count as f32;
    let offset = delta * geometry.provider_row_h;
    geometry.provider_row_count = provider_count;
    for field in &mut geometry.provider_fields {
        field.1 += offset;
    }
    geometry.provider_codex_goals.1 += offset;
    geometry.provider_codex_remote.1 += offset;
    geometry.provider_codex_apply.1 += offset;
    geometry.provider_save.1 += offset;
    geometry.provider_test.1 += offset;
    geometry.provider_delete.1 += offset;
    geometry.providers_h = (geometry.providers_h + offset).max(0.0);
}

pub(crate) fn opacity_slider_rect(
    size_info: &SizeInfo,
    scale_factor: f32,
    area: (f32, f32, f32, f32),
    scroll: f32,
    target: super::SettingsOpacityTarget,
    density: crate::display::ui::tokens::Density,
) -> (f32, f32, f32, f32) {
    let geometry = settings_geometry(
        size_info,
        scale_factor,
        area,
        scroll,
        0,
        0,
        density,
        ProxyPaneState::default(),
        KeymapPaneState::default(),
    );
    match target {
        super::SettingsOpacityTarget::Terminal => geometry.opacity_slider,
        super::SettingsOpacityTarget::BackgroundImage => geometry.background_image_opacity_slider,
    }
}

pub(crate) fn opacity_from_pointer(pointer_x: f32, slider: (f32, f32, f32, f32)) -> f32 {
    ((pointer_x - slider.0) / slider.2.max(1.0)).clamp(0.0, 1.0)
}

/// 调色盘拖拽要用的 SV 面与色相条矩形。与 `opacity_slider_rect` 同款：
/// 外观页几何不依赖 shell/font/proxy/keymap 状态，默认值重建即可，绘制、
/// 命中与拖拽三方共用同一几何来源。
pub(crate) fn background_color_picker_rects(
    size_info: &SizeInfo,
    scale_factor: f32,
    area: (f32, f32, f32, f32),
    scroll: f32,
    density: crate::display::ui::tokens::Density,
) -> ((f32, f32, f32, f32), (f32, f32, f32, f32)) {
    let geometry = settings_geometry(
        size_info,
        scale_factor,
        area,
        scroll,
        0,
        0,
        density,
        ProxyPaneState::default(),
        KeymapPaneState::default(),
    );
    let popup = background_color_popup(&geometry, scale_factor);
    (popup.sv, popup.hue)
}

/// 主机行右缘的动作槽。绘制与命中共用这一处推导，是"看得见的按钮点不中"的
/// 唯一防线。
///
/// 2026-08-11 对齐原型：**主动作「连接」是文字按钮**（白底+描边+"连接"二字），
/// 编辑/删除是方形图标槽。原型里唯一带底的就是主动作——一行五个等价图标会让
/// 人逐个悬停去猜哪个是"进去"，文字消掉这次猜测。三个槽都只在 hover 时显形，
/// 静态那一行只剩身份信息。命中区比墨迹宽，指针容差不被视觉尺寸绑死。
pub(super) fn ssh_host_action_rect(
    row: (f32, f32, f32, f32),
    scale: f32,
    action: usize,
) -> (f32, f32, f32, f32) {
    let s = |v: f32| v * scale;
    let (x, y, w, h) = row;
    let gap = s(4.0);
    let right = x + w - s(14.0);
    let slot = s(26.0).min((h - s(10.0)).max(s(18.0)));
    let connect_w = s(52.0);
    // 从右往左排：删除、编辑、连接。连接最宽，所以单独算。
    match action {
        2 => {
            let bx = right - slot;
            (bx, widgets::centered_y(y, h, slot), slot, slot)
        },
        1 => {
            let bx = right - slot * 2.0 - gap;
            (bx, widgets::centered_y(y, h, slot), slot, slot)
        },
        _ => {
            let bh = s(26.0).min((h - s(10.0)).max(s(18.0)));
            let bx = right - slot * 2.0 - gap * 2.0 - connect_w;
            (bx, widgets::centered_y(y, h, bh), connect_w, bh)
        },
    }
}

/// Compact trailing command button inside an otherwise non-clickable settings row.
pub(super) const STANDARD_ROW_ACTION_W: f32 = 112.0;

pub(super) fn row_action_rect(row: (f32, f32, f32, f32), scale: f32, logical_w: f32) -> (f32, f32, f32, f32) {
    let s = |v: f32| v * scale;
    let (x, y, w, h) = row;
    let button_w = s(logical_w).min(w * 0.42);
    let button_h = s(30.0).min(h);
    let button_y = if h >= s(56.0) { y + h - s(38.0) } else { widgets::centered_y(y, h, button_h) };
    (x + w - s(16.0) - button_w, button_y, button_w, button_h)
}

/// Appearance 预览卡的壁纸绘制矩形：`(fit 目标, 实际允许触碰的裁剪带)`。
/// 裁剪带是预览与设置卡内容区的竖向交集——预览滚到 header 之下时壁纸不
/// 能跟着涂出去。完全滚出可视区时返回 `None`。
pub(crate) fn appearance_preview_wallpaper_rects(
    size_info: &SizeInfo,
    scale_factor: f32,
    area: (f32, f32, f32, f32),
    scroll: f32,
    hidden_hosts: usize,
    density: crate::display::ui::tokens::Density,
) -> Option<((f32, f32, f32, f32), (f32, f32, f32, f32))> {
    let geometry = settings_geometry(
        size_info,
        scale_factor,
        area,
        scroll,
        hidden_hosts,
        0,
        density,
        ProxyPaneState::default(),
        KeymapPaneState::default(),
    );
    let (vx, vy, vw, vh) = geometry.preview;
    let (_, content_y, _, _) = geometry.content;
    let (_, py, _, ph) = geometry.popup;
    let top = vy.max(content_y);
    let bottom = (vy + vh).min(py + ph);
    if bottom <= top || vw <= 0.0 {
        return None;
    }
    Some(((vx, vy, vw, vh), (vx, top, vw, bottom - top)))
}

/// The combobox anchor rect + option count for `dropdown`, IF it belongs to
/// the active section. Hit-testing, popup quads and popup text all resolve
/// the floating list through this one helper so the three can never disagree.
/// 背景色浮层的几何：真调色盘（SV 面 + 色相条）、12 个预设色板格、16 进制
/// 输入框。绘制与命中测试共用这一个来源（组件化范式：几何同源，控件与
/// 点击区不漂移）。
pub(crate) struct BackgroundColorPopup {
    pub(super) rect: (f32, f32, f32, f32),
    /// 饱和度（→右增）/ 明度（→下减）取色面，底色跟随当前色相。
    pub(super) sv: (f32, f32, f32, f32),
    /// 色相横条（0–360°）。
    pub(super) hue: (f32, f32, f32, f32),
    pub(super) swatch: [(f32, f32, f32, f32); 12],
    pub(super) hex: (f32, f32, f32, f32),
}

pub(crate) fn background_color_popup(
    geometry: &SettingsGeometry,
    scale: f32,
) -> BackgroundColorPopup {
    let s = |v: f32| v * scale;
    let (ax, ay, aw, ah) = widgets::combobox_rect(geometry.background, scale);
    const COLS: usize = 6;
    let cell = s(30.0);
    let gap = s(8.0);
    let pad = s(12.0);
    let grid_w = COLS as f32 * cell + (COLS - 1) as f32 * gap;
    let grid_h = 2.0 * cell + gap;
    let sv_h = s(128.0);
    let hue_h = s(14.0);
    let hex_h = s(34.0);
    let w = (grid_w + 2.0 * pad).max(aw);
    let h = pad + sv_h + gap + hue_h + gap + grid_h + gap + hex_h + pad;
    // 与 combobox 浮层同规则：锚行右缘对齐，紧贴行下方展开。
    let x = ax + aw - w;
    let y = ay + ah + s(6.0);
    let sv = (x + pad, y + pad, w - 2.0 * pad, sv_h);
    let hue = (x + pad, y + pad + sv_h + gap, w - 2.0 * pad, hue_h);
    let grid_y = y + pad + sv_h + gap + hue_h + gap;
    let mut swatch = [(0.0, 0.0, 0.0, 0.0); 12];
    for (i, rect) in swatch.iter_mut().enumerate() {
        let row = i / COLS;
        let col = i % COLS;
        *rect =
            (x + pad + col as f32 * (cell + gap), grid_y + row as f32 * (cell + gap), cell, cell);
    }
    let hex = (x + pad, grid_y + grid_h + gap, w - 2.0 * pad, hex_h);
    BackgroundColorPopup { rect: (x, y, w, h), sv, hue, swatch, hex }
}

/// 字体弹层的总行数：候选行 + 顶部那个搜索框。
///
/// 一个字体都筛不出来时仍然是 1 行——那时搜索框是弹层里唯一的东西，也正是
/// 用户要用来改查询串的那一个。
pub(crate) fn font_popup_row_count(font_rows: usize) -> usize {
    font_rows + 1
}

const FONT_POPUP_MAX_VISIBLE_ROWS: usize = 8;

pub(super) fn font_popup_window(total_rows: usize, requested_scroll: usize) -> (usize, usize) {
    let candidates = total_rows.saturating_sub(1);
    let candidate_visible = candidates.min(FONT_POPUP_MAX_VISIBLE_ROWS.saturating_sub(1));
    let max_scroll = candidates.saturating_sub(candidate_visible);
    (requested_scroll.min(max_scroll), 1 + candidate_visible.min(candidates))
}

pub(super) fn popup_visible_index(
    dropdown: SettingsDropdown,
    absolute: Option<usize>,
    offset: usize,
    visible: usize,
) -> Option<usize> {
    let absolute = absolute?;
    if dropdown != SettingsDropdown::Font {
        return Some(absolute);
    }
    if absolute == 0 {
        Some(0)
    } else if absolute >= 1 + offset && absolute < 1 + offset + visible.saturating_sub(1) {
        Some(absolute - offset)
    } else {
        None
    }
}

/// 弹层第 `row` 行对应第几个候选。`None` = 那是搜索框。
pub(crate) fn font_popup_slot(row: usize) -> Option<usize> {
    row.checked_sub(1)
}

pub(super) fn dropdown_anchor(
    geometry: &SettingsGeometry,
    section: NebulaSettingsSection,
    dropdown: SettingsDropdown,
    shell_count: usize,
    font_count: usize,
    scale: f32,
) -> Option<((f32, f32, f32, f32), usize)> {
    use NebulaSettingsSection as Section;
    let anchor = |row| widgets::combobox_rect(row, scale);
    match (section, dropdown) {
        (Section::Profiles, SettingsDropdown::Shell) => Some((anchor(geometry.shell), shell_count)),
        (Section::Profiles, SettingsDropdown::Font) => {
            Some((anchor(geometry.font), font_popup_row_count(font_count)))
        },
        (Section::Profiles, SettingsDropdown::Accept) => {
            Some((anchor(geometry.accept), ACCEPT_OPTIONS.len()))
        },
        (Section::Profiles, SettingsDropdown::CompletionStyle) => {
            Some((anchor(geometry.completion_style), COMPLETION_STYLE_OPTIONS.len()))
        },
        (Section::Backup, SettingsDropdown::BackupProtocol) => {
            Some((anchor(geometry.backup_remote_protocol), BACKUP_PROTOCOL_OPTIONS.len()))
        },
        (Section::Appearance, SettingsDropdown::CellWidthMode) => {
            Some((anchor(geometry.cell_width_mode), CELL_WIDTH_MODE_OPTIONS.len()))
        },
        (Section::Interaction, SettingsDropdown::TabReveal) => {
            Some((anchor(geometry.tab_reveal), TAB_REVEAL_OPTIONS.len()))
        },
        (Section::Proxy, SettingsDropdown::SshProxyMode) => Some((
            ssh_proxy_mode_control(geometry.ssh_proxy_mode, scale),
            SSH_PROXY_MODE_OPTIONS.len(),
        )),
        (Section::Proxy, SettingsDropdown::SshProxyProtocol) => Some((
            ssh_proxy_manual_controls(geometry.ssh_proxy_expand, scale).0,
            MANUAL_PROXY_PROTOCOL_OPTIONS.len(),
        )),
        // 跳板主机下拉挂在展开行上；空列表也给一行（占位提示，点了无动作）。
        (Section::Proxy, SettingsDropdown::SshJumpHost) => Some((
            ssh_proxy_expand_control(geometry.ssh_proxy_expand, scale),
            geometry.ssh_host_count.max(1),
        )),
        (Section::Interaction, SettingsDropdown::NewTabPosition) => {
            Some((anchor(geometry.new_tab_position), NEW_TAB_POSITION_OPTIONS.len()))
        },
        (Section::Appearance, SettingsDropdown::BackgroundFit) => {
            Some((anchor(geometry.background_image_fit), BACKGROUND_FIT_OPTIONS.len()))
        },
        (Section::Appearance, SettingsDropdown::BackgroundAlignment) => {
            Some((anchor(geometry.background_image_alignment), BACKGROUND_ALIGNMENT_OPTIONS.len()))
        },
        (Section::Appearance, SettingsDropdown::Language) => {
            Some((anchor(geometry.language_row), LANGUAGE_OPTIONS.len()))
        },
        (Section::Appearance, SettingsDropdown::Density) => {
            Some((anchor(geometry.density_row), DENSITY_OPTIONS.len()))
        },
        (Section::Appearance, SettingsDropdown::CursorShape) => {
            Some((anchor(geometry.cursor_shape_row), CURSOR_SHAPE_OPTIONS.len()))
        },
        _ => None,
    }
}

/// Hit-test the top-left settings button and its popup. `scroll` must be the
/// same offset the renderer used, so hits land on what the user actually sees;
/// rows scrolled out of the content viewport don't respond.
#[allow(clippy::too_many_arguments)]
/// 字体弹层里搜索框的矩形。命中与渲染各算一遍会漂，所以两边都问这里。
///
/// 返回 `None` 表示当前没有展开字体弹层。
pub fn font_search_field_rect(
    size_info: &SizeInfo,
    scale_factor: f32,
    area: (f32, f32, f32, f32),
    section: NebulaSettingsSection,
    scroll: f32,
    dropdown: Option<SettingsDropdown>,
    font_count: usize,
    popup_scroll: usize,
    hidden_host_count: usize,
    ssh_host_count: usize,
    density: crate::display::ui::tokens::Density,
) -> Option<(f32, f32, f32, f32)> {
    if dropdown != Some(SettingsDropdown::Font) {
        return None;
    }
    let geometry = settings_geometry(
        size_info,
        scale_factor,
        area,
        scroll,
        hidden_host_count,
        ssh_host_count,
        density,
        ProxyPaneState::default(),
        KeymapPaneState::default(),
    );
    let s = |v: f32| v * scale_factor;
    let (_, py, _, ph) = geometry.popup;
    let (anchor, total) =
        dropdown_anchor(&geometry, section, SettingsDropdown::Font, 0, font_count, scale_factor)?;
    let (_, count) = font_popup_window(total, popup_scroll);
    let popup = widgets::combobox_popup_rect(
        anchor,
        count,
        scale_factor,
        geometry.content_top,
        py + ph - s(6.0),
    );
    Some(widgets::popup_row_rect(popup, 0, scale_factor))
}

/// 字体弹层的共享滚动条几何与最大候选偏移；与
/// [`push_popup_quads`] 的绘制参数同源，track/thumb 命中和拖拽都用它。
#[allow(clippy::too_many_arguments)]
pub(crate) fn font_popup_scrollbar(
    size_info: &SizeInfo,
    scale_factor: f32,
    area: (f32, f32, f32, f32),
    section: NebulaSettingsSection,
    scroll: f32,
    dropdown: Option<SettingsDropdown>,
    font_count: usize,
    popup_scroll: usize,
    hidden_host_count: usize,
    ssh_host_count: usize,
    density: crate::display::ui::tokens::Density,
) -> Option<(widgets::OverlayScrollbar, usize)> {
    if dropdown != Some(SettingsDropdown::Font) {
        return None;
    }
    let geometry = settings_geometry(
        size_info,
        scale_factor,
        area,
        scroll,
        hidden_host_count,
        ssh_host_count,
        density,
        ProxyPaneState::default(),
        KeymapPaneState::default(),
    );
    let s = |v: f32| v * scale_factor;
    let (_, py, _, ph) = geometry.popup;
    let (anchor, total) =
        dropdown_anchor(&geometry, section, SettingsDropdown::Font, 0, font_count, scale_factor)?;
    let (offset, count) = font_popup_window(total, popup_scroll);
    let popup = widgets::combobox_popup_rect(
        anchor,
        count,
        scale_factor,
        geometry.content_top,
        py + ph - s(6.0),
    );
    let total_h = total as f32 * widgets::POPUP_ROW_H * scale_factor;
    let viewport_h = count as f32 * widgets::POPUP_ROW_H * scale_factor;
    let bar = widgets::overlay_scrollbar(
        popup,
        viewport_h,
        total_h,
        offset as f32 * widgets::POPUP_ROW_H * scale_factor,
        scale_factor,
    )?;
    Some((bar, total - count))
}

/// 代理输入框（0=地址 1=绕过）的输入矩形；与渲染共用
/// [`sync_input_rect`]，供鼠标点击换算 caret 落点。
pub fn ssh_proxy_input_rect(
    size_info: &SizeInfo,
    scale_factor: f32,
    area: (f32, f32, f32, f32),
    scroll: f32,
    hidden_host_count: usize,
    ssh_host_count: usize,
    density: crate::display::ui::tokens::Density,
    proxy: ProxyPaneState,
    index: usize,
) -> (f32, f32, f32, f32) {
    let geometry = settings_geometry(
        size_info,
        scale_factor,
        area,
        scroll,
        hidden_host_count,
        ssh_host_count,
        density,
        proxy,
        KeymapPaneState::default(),
    );
    match index {
        0 => ssh_proxy_manual_controls(geometry.ssh_proxy_expand, scale_factor).1,
        1 => sync_input_rect(geometry.ssh_proxy_bypass, scale_factor),
        _ => ssh_proxy_expand_control(geometry.ssh_proxy_expand, scale_factor),
    }
}

/// 按键映射页搜索框矩形；与渲染同一份 [`settings_geometry`]，供鼠标
/// 点击换算 caret 落点。
pub fn keymap_search_rect(
    size_info: &SizeInfo,
    scale_factor: f32,
    area: (f32, f32, f32, f32),
    scroll: f32,
    hidden_host_count: usize,
    ssh_host_count: usize,
    density: crate::display::ui::tokens::Density,
    keymap_pane: KeymapPaneState,
) -> (f32, f32, f32, f32) {
    settings_geometry(
        size_info,
        scale_factor,
        area,
        scroll,
        hidden_host_count,
        ssh_host_count,
        density,
        ProxyPaneState::default(),
        keymap_pane,
    )
    .keymap_search
}

/// Active provider field rectangle, shared by pointer placement and IME
/// anchoring with the render pass.
pub fn provider_input_rect(
    size_info: &SizeInfo,
    scale_factor: f32,
    area: (f32, f32, f32, f32),
    scroll: f32,
    hidden_host_count: usize,
    ssh_host_count: usize,
    density: crate::display::ui::tokens::Density,
    provider_count: usize,
    index: usize,
) -> Option<(f32, f32, f32, f32)> {
    let mut geometry = settings_geometry(
        size_info,
        scale_factor,
        area,
        scroll,
        hidden_host_count,
        ssh_host_count,
        density,
        ProxyPaneState::default(),
        KeymapPaneState::default(),
    );
    fit_provider_rows(&mut geometry, provider_count);
    geometry.provider_fields.get(index).copied().map(|row| sync_input_rect(row, scale_factor))
}
