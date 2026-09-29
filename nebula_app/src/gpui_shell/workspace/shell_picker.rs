//! Shell/profile palette rows and their shared icon fallback.

use super::{
    WorkspacePaletteAction, WorkspacePaletteHintStyle, WorkspacePaletteRow, ssh_host_icon_ids,
};

/// shell 行的回落字形：没有品牌贴图时用 `shell_detect::icon_for_id` 的
/// id-keyed Nerd Font 字形（与设置页下拉、命令面板同一口径）。
///
/// `has_brand` 为真时返回 `None`——贴图已经画了，两个都留同行会出现两个图标。
pub(super) fn fallback_shell_glyph(id: &str, has_brand: bool) -> Option<char> {
    if has_brand {
        return None;
    }
    crate::shell_detect::icon_for_id(id).chars().next()
}

/// SSH 主机的展示名与次要信息，与旧壳 `SshRow` 同口径：
///
/// - 用户在 SSH 设置里起过"主机名称"：label 显示别名，hint 给真实连接地址；
/// - 没起名：回落地址本身当 label，hint 保持 "SSH" 类型标签，避免重复。
pub(super) fn ssh_host_display(label: Option<&str>, host: &str) -> (String, String) {
    match label {
        Some(label) if !label.trim().is_empty() => (label.trim().to_owned(), host.to_owned()),
        _ => (host.to_owned(), "SSH".to_owned()),
    }
}

/// 一行 profile 右侧「开在哪儿」的显示串。
///
/// 这一列回答的是「这一行会开在哪儿」，不是「用哪个可执行文件」：用户给项目建的
/// 入口，看到 `D:\huozigemima` 或 `/home/me/project` 才认得出是哪一行。所以优先
/// `cwd`。
///
/// WSL 入口的 `cwd` 是空的（目录随 `--cd` 直接带给来宾，宿主侧没有对应目录，见
/// `quick_access_profile_for`），这时显示 `Ubuntu:/home/me/project`——和 Git 视图里
/// [`crate::shell_detect::WslCwd`] 的写法一致，也就是终端真正会落到的地方。少了这
/// 一步，WSL 行会显示 `C:\Windows\System32\wsl.exe`，等于什么都没说。
///
/// 两条都取不到（`scan_directory` 导入的 profile 恒无 `cwd`）才回落到命令路径：
/// 右列宁可显示可执行文件，也不能空着。
///
/// 侧栏「快速访问」与 Ctrl+K 选择器共用本函数，两处的这一列因此不会各说各话。
pub(super) fn profile_location(profile: &crate::config::ui_config::Profile) -> String {
    if let Some(cwd) = profile.cwd.as_ref() {
        return cwd.to_string_lossy().into_owned();
    }
    if let Some(distro) = crate::shell_detect::wsl_launch_distro(&profile.command, &profile.args) {
        if let Some(guest) = crate::shell_detect::wsl_launch_guest(&profile.command, &profile.args)
        {
            return format!("{distro}:{guest}");
        }
    }
    profile.command.clone()
}

/// 新建终端弹窗的行：已检测 shell + SSH 主机，分组对照旧壳
/// `CommandPalette::open_profiles`（推荐 / 所有 Shell / SSH 主机）。
/// 三点菜单与 Ctrl+K 打开的是这份列表，不是通用命令面板。
pub(super) fn shell_palette_rows(
    shells: Vec<crate::shell_detect::DetectedShell>,
    profiles: Vec<crate::config::ui_config::Profile>,
    ssh_hosts: impl IntoIterator<Item = (String, String)>,
    default_shell_id: &str,
    language: crate::display::UiLanguage,
    scale_factor: f32,
) -> Vec<WorkspacePaletteRow> {
    const SHELL_ICON_PX: f32 = 22.0;
    let recommended = language.pick("推荐", "Recommended");
    let all_shells = language.pick("所有 Shell", "All shells");
    let ssh_group = language.pick("SSH 主机", "SSH hosts");
    let mut rows: Vec<WorkspacePaletteRow> = shells
        .into_iter()
        .map(|shell| {
            let is_default = shell.id == default_shell_id;
            let icon = crate::gpui_shell::widgets::shell_brand_image(
                &shell.id,
                SHELL_ICON_PX,
                scale_factor,
            );
            // 没有品牌贴图的 shell（zsh、csh、ksh…）不能就这么空着：回落到
            // 按 id 取字的 Nerd Font 字形，与设置页下拉同一口径。
            let icon_glyph = fallback_shell_glyph(&shell.id, icon.is_some());
            WorkspacePaletteRow {
                group_order: if is_default { 0 } else { 1 },
                group: if is_default { recommended.to_owned() } else { all_shells.to_owned() },
                label: shell.name.clone(),
                hint: shell.program.clone(),
                hint_style: WorkspacePaletteHintStyle::Metadata,
                search: format!("{} {} shell profile", shell.name, shell.id).to_lowercase(),
                icon,
                icon_glyph,
                icon_path: None,
                action: WorkspacePaletteAction::LaunchShell(shell),
            }
        })
        .collect();
    rows.extend(profiles.into_iter().filter_map(|profile| {
        let id = profile.settings_id()?;
        let is_default = id.eq_ignore_ascii_case(default_shell_id);
        let icon_id = profile.shell_id.as_deref().unwrap_or(&id);
        let icon =
            crate::gpui_shell::widgets::shell_brand_image(icon_id, SHELL_ICON_PX, scale_factor);
        // 借用在 `profile` 被移进 action 之前结束。
        let icon_glyph = fallback_shell_glyph(icon_id, icon.is_some());
        let label = profile.name.clone();
        let hint = profile_location(&profile);
        Some(WorkspacePaletteRow {
            group_order: if is_default { 0 } else { 1 },
            group: if is_default { recommended.to_owned() } else { all_shells.to_owned() },
            // 目录也进搜索串：项目名记不住时，敲目录名是最自然的找法。命令路径不
            // 再单列一份——`profile_location` 取不到 cwd 时给出的就是它，重复只会
            // 让同一个路径在搜索串里出现两次。
            search: format!("{} {} {} shell profile", profile.name, id, hint).to_lowercase(),
            label,
            hint,
            hint_style: WorkspacePaletteHintStyle::Metadata,
            action: WorkspacePaletteAction::LaunchProfile(profile),
            icon_glyph,
            icon,
            icon_path: None,
        })
    }));
    if let Some(position) = rows.iter().position(|row| row.group_order == 0) {
        let default_row = rows.remove(position);
        rows.insert(0, default_row);
    }
    let ssh_icons = ssh_host_icon_ids(&crate::display::nebula_data_dir());
    rows.extend(ssh_hosts.into_iter().map(|(host, label)| {
        let glyph =
            crate::display::ui::os_icons::resolve(ssh_icons.get(&host).map(String::as_str)).glyph;
        // 空串 = 没起名：`ssh_host_display` 回落地址本身，hint 保持 "SSH"。
        let named = (!label.is_empty()).then_some(label.as_str());
        let (label, hint) = ssh_host_display(named, &host);
        let search = format!("{label} {host} ssh host remote lianjie 连接").to_lowercase();
        WorkspacePaletteRow {
            group_order: 2,
            group: ssh_group.to_owned(),
            label,
            hint,
            hint_style: WorkspacePaletteHintStyle::Metadata,
            search,
            action: WorkspacePaletteAction::LaunchSshHost(host),
            icon: None,
            icon_glyph: Some(glyph),
            icon_path: None,
        }
    }));
    rows
}
