#[cfg(test)]
mod tests {
    use super::{
        CELL_WIDTH_MODE_OPTIONS, CellWidthMode, KeymapPaneState, NEW_TAB_POSITION_OPTIONS,
        NebulaSettingsSection, NewTabPosition, ProxyChoice, ProxyPaneState, SHOW_BACKUP_SETTINGS,
        SHOW_WEBDAV_SYNC_SETTINGS, STANDARD_ROW_ACTION_W, SettingsHit, TabRevealMotion, UiLanguage,
        advanced_content_end, background_color_popup, cell_width_mode_label, font_popup_row_count,
        font_popup_slot, new_tab_position_label, opacity_from_pointer, proxy_section_title_y,
        row_action_rect, settings_geometry, settings_hit, ssh_proxy_manual_controls,
        ssh_proxy_mode_control, ssh_proxy_test_button,
    };
    use crate::display::SizeInfo;
    use crate::display::ui::tokens::Density;
    use crate::display::ui::widgets;

    #[test]
    fn color_picker_popup_keeps_sv_hue_and_swatches_disjoint() {
        // 三个交互区不重叠：SV 面、色相条、第一格色板按序垂直排布。
        let size = SizeInfo::new(1280.0, 900.0, 8.0, 16.0, 0.0, 0.0, false);
        let geometry = settings_geometry(
            &size,
            1.0,
            (0.0, 0.0, 1280.0, 900.0),
            0.0,
            0,
            0,
            Density::Standard,
            ProxyPaneState::default(),
            KeymapPaneState::default(),
        );
        let popup = background_color_popup(&geometry, 1.0);
        assert!(popup.sv.1 + popup.sv.3 <= popup.hue.1);
        assert!(popup.hue.1 + popup.hue.3 <= popup.swatch[0].1);
        assert!(popup.swatch[11].1 + popup.swatch[11].3 <= popup.hex.1);
        // 全部落在面板内。
        assert!(popup.hex.1 + popup.hex.3 <= popup.rect.1 + popup.rect.3);
    }

    #[test]
    fn the_font_popup_reserves_its_first_row_for_the_search_field() {
        // 搜索框占掉第 0 行，选项整体下移一行。用「多算一行」而不是另开一套
        // 几何：弹层的定位、上下翻转与裁剪都还归 combobox_popup_rect 管。
        assert_eq!(font_popup_row_count(0), 1, "一个字体都没有时也要有搜索框");
        assert_eq!(font_popup_row_count(7), 8);

        assert_eq!(font_popup_slot(0), None, "第 0 行是搜索框，不是选项");
        assert_eq!(font_popup_slot(1), Some(0));
        assert_eq!(font_popup_slot(4), Some(3));
    }

    #[test]
    fn slider_pointer_maps_to_clamped_fraction() {
        let slider = (100.0, 20.0, 200.0, 36.0);
        assert_eq!(opacity_from_pointer(50.0, slider), 0.0);
        assert_eq!(opacity_from_pointer(100.0, slider), 0.0);
        assert_eq!(opacity_from_pointer(200.0, slider), 0.5);
        assert_eq!(opacity_from_pointer(300.0, slider), 1.0);
        assert_eq!(opacity_from_pointer(350.0, slider), 1.0);
    }

    #[test]
    fn hidden_webdav_group_does_not_extend_advanced_content() {
        assert!(!SHOW_WEBDAV_SYNC_SETTINGS);
        // SSH 已迁到独立页面；隐藏同步组不能继续把 Advanced 撑高。会话组
        // 现为 4 行：保留会话 / 恢复会话 / 恢复时接续 AI 对话（resume_ai）
        // / 常驻托盘图标（tray）。
        assert_eq!(advanced_content_end(146.0, 308.0, 44.0), 322.0);
    }

    #[test]
    fn backup_settings_entry_is_exposed_with_remote_destinations() {
        // 2026-08-13：远程备份（目录/WebDAV/S3/SFTP）接入后备份页开放。
        assert!(SHOW_BACKUP_SETTINGS);
    }

    #[test]
    fn backup_remote_actions_track_visible_field_count() {
        let size = SizeInfo::new(1600.0, 1000.0, 8.0, 16.0, 0.0, 0.0, false);
        let area = (0.0, 40.0, 1600.0, 960.0);
        let geometry = settings_geometry(
            &size,
            1.0,
            area,
            0.0,
            0,
            0,
            Density::Standard,
            ProxyPaneState::default(),
            KeymapPaneState::default(),
        );
        // 满配（S3 = 5 字段）时动作行与几何里的静态槽位重合。
        let full = super::backup_remote_actions_rect(&geometry, 1.0, 5);
        assert!((full.1 - geometry.backup_remote_actions.1).abs() < 0.5);
        // 字段更少的协议动作行上移，且始终排在最后一个可见行之后。
        let folder = super::backup_remote_actions_rect(&geometry, 1.0, 1);
        assert!(folder.1 < full.1);
        let first_field = geometry.backup_remote_fields[0];
        assert!(folder.1 >= first_field.1 + first_field.3);
    }

    #[test]
    fn tab_reveal_motion_defaults_compatibly_and_round_trips() {
        assert_eq!(TabRevealMotion::default(), TabRevealMotion::Slide);
        assert_eq!(TabRevealMotion::parse("slide"), Some(TabRevealMotion::Slide));
        assert_eq!(TabRevealMotion::parse("INSTANT"), Some(TabRevealMotion::Instant));
        assert_eq!(TabRevealMotion::parse("unknown").unwrap_or_default(), TabRevealMotion::Slide);
        for value in [TabRevealMotion::Slide, TabRevealMotion::Instant] {
            assert_eq!(TabRevealMotion::parse(value.settings_value()), Some(value));
        }
    }

    #[test]
    fn settings_actions_keep_their_hit_area_on_the_button() {
        let row = (100.0, 200.0, 500.0, 44.0);
        let button = row_action_rect(row, 1.0, STANDARD_ROW_ACTION_W);
        assert!(button.0 > row.0);
        assert!(button.0 + button.2 <= row.0 + row.2);
        assert!(!super::contains_rect(button, row.0 + 8.0, row.1 + row.3 * 0.5));
        assert!(
            super::contains_rect(button, button.0 + button.2 * 0.5, button.1 + button.3 * 0.5,)
        );

        let toggle = widgets::toggle_rect(row, 1.0);
        assert!(!super::contains_rect(toggle, row.0 + 8.0, row.1 + row.3 * 0.5));
        assert!(
            super::contains_rect(toggle, toggle.0 + toggle.2 * 0.5, toggle.1 + toggle.3 * 0.5,)
        );
    }

    #[test]
    fn profile_import_and_open_actions_share_one_width() {
        let import = row_action_rect((100.0, 200.0, 500.0, 44.0), 1.5, STANDARD_ROW_ACTION_W);
        let open = row_action_rect((100.0, 400.0, 500.0, 44.0), 1.5, STANDARD_ROW_ACTION_W);
        assert_eq!(import.2, open.2);
        assert_eq!(import.2, STANDARD_ROW_ACTION_W * 1.5);
    }

    #[test]
    fn settings_geometry_releases_navigation_width_before_stacking_rows() {
        let size = proxy_test_size();
        let geometry = |width| {
            settings_geometry(
                &size,
                1.0,
                (0.0, 0.0, width, 900.0),
                0.0,
                0,
                0,
                Density::Standard,
                ProxyPaneState::default(),
                KeymapPaneState::default(),
            )
        };

        let wide = geometry(1200.0);
        assert!(!wide.compact_nav);
        assert!(!wide.stacked_rows);
        assert_eq!(wide.sidebar.2, 196.0);

        let medium = geometry(800.0);
        assert!(medium.compact_nav);
        assert!(!medium.stacked_rows);
        assert_eq!(medium.sidebar.2, 64.0);
        assert!(medium.content.0 + medium.content.2 <= medium.popup.0 + medium.popup.2);

        let narrow = geometry(600.0);
        assert!(narrow.compact_nav);
        assert!(narrow.stacked_rows);
        assert!(narrow.shell.3 >= 72.0);
        let shell_control = widgets::combobox_rect(narrow.shell, 1.0);
        assert!(shell_control.0 >= narrow.shell.0);
        assert!(shell_control.0 + shell_control.2 <= narrow.shell.0 + narrow.shell.2);
        assert!(shell_control.1 > narrow.shell.1 + 24.0);
    }

    fn proxy_test_size() -> SizeInfo {
        SizeInfo::new(1400.0, 1000.0, 10.0, 20.0, 0.0, 0.0, false)
    }

    #[test]
    fn provider_geometry_tracks_every_persisted_custom_entry() {
        let size = proxy_test_size();
        let area = (0.0, 0.0, 1200.0, 1800.0);
        let provider_count = 14;
        let mut geometry = settings_geometry(
            &size,
            1.0,
            area,
            0.0,
            0,
            0,
            Density::Standard,
            ProxyPaneState::default(),
            KeymapPaneState::default(),
        );
        let old_field_y = geometry.provider_fields[0].1;
        super::fit_provider_rows(&mut geometry, provider_count);
        assert_eq!(geometry.provider_row_count, provider_count);
        assert_eq!(geometry.provider_fields[0].1, old_field_y + geometry.provider_row_h * 8.0);

        let last_row = (
            geometry.provider_row0.0,
            geometry.provider_row0.1 + 13.0 * geometry.provider_row_h,
            geometry.provider_row0.2,
            geometry.provider_row_h,
        );
        let hit = settings_hit(
            &size,
            1.0,
            area,
            last_row.0 + 20.0,
            last_row.1 + last_row.3 * 0.5,
            true,
            NebulaSettingsSection::Providers,
            0.0,
            None,
            0,
            0,
            0,
            0,
            0,
            Density::Standard,
            ProxyPaneState::default(),
            KeymapPaneState::default(),
            provider_count,
            Default::default(),
        );
        assert_eq!(hit, SettingsHit::ProviderRow(13));
    }

    /// 网络页收口成单张紧凑卡后的几何合同：只有**模式 = 自定义**才展开
    /// 地址行（撑高滚动区）；旧扫描/发现列表/覆盖行已经全部收成零尺寸，
    /// 绘制与命中都不得再消费它们——这条测试原来验证的是旧多组件布局，
    /// 重设计时一并改写。
    #[test]
    fn custom_proxy_mode_expands_the_card_and_legacy_scan_geometry_stays_zero() {
        let size = proxy_test_size();
        let area = (0.0, 0.0, 1200.0, 900.0);
        let custom = ProxyPaneState {
            mode: crate::ssh_proxy::ProxyMode::Custom,
            choice: ProxyChoice::Manual,
            found_count: 3,
            override_count: 2,
            ..Default::default()
        };
        let custom_geometry = settings_geometry(
            &size,
            1.0,
            area,
            0.0,
            0,
            4,
            Density::Standard,
            custom,
            KeymapPaneState::default(),
        );
        let off_geometry = settings_geometry(
            &size,
            1.0,
            area,
            0.0,
            0,
            4,
            Density::Standard,
            ProxyPaneState::default(),
            KeymapPaneState::default(),
        );
        // 自定义模式追加地址行，页面必须比关闭模式高。
        assert!(custom_geometry.proxy_h > off_geometry.proxy_h);
        // 展开行紧跟模式行之后。
        assert!(custom_geometry.ssh_proxy_expand.1 > custom_geometry.ssh_proxy_mode.1);
        // 旧结构零尺寸（即便 found/override 计数非零也不得复活）。
        for legacy in [
            custom_geometry.ssh_proxy_list,
            custom_geometry.ssh_proxy_scan_button,
            custom_geometry.ssh_proxy_scan_head,
            custom_geometry.ssh_proxy_found_row0,
            custom_geometry.ssh_proxy_override_row0,
            custom_geometry.ssh_proxy_other_rows[0],
        ] {
            assert_eq!((legacy.2, legacy.3), (0.0, 0.0), "legacy proxy geometry must stay zero");
        }
    }

    #[test]
    fn proxy_section_title_stays_above_the_test_banner() {
        let size = proxy_test_size();
        let geometry = settings_geometry(
            &size,
            1.0,
            (0.0, 0.0, 1200.0, 900.0),
            0.0,
            0,
            0,
            Density::Standard,
            ProxyPaneState::default(),
            KeymapPaneState::default(),
        );
        let title_y = proxy_section_title_y(geometry.ssh_proxy_test.1, 1.0);
        assert!(title_y >= geometry.content_top);
        assert!(title_y + 26.0 <= geometry.ssh_proxy_test.1);
    }

    /// 紧凑代理卡的命中合同：自定义模式下可点的只有模式下拉、协议下拉、
    /// 地址输入和出网测试按钮四个控件；旧扫描按钮/发现行是零尺寸幽灵几何，
    /// 永远命不中。原测试断言旧布局的 Rescan/LinkPick，重设计时一并改写。
    #[test]
    fn custom_proxy_hit_test_exposes_mode_protocol_address_and_test_only() {
        let size = proxy_test_size();
        let area = (0.0, 0.0, 1200.0, 900.0);
        let proxy = ProxyPaneState {
            mode: crate::ssh_proxy::ProxyMode::Custom,
            choice: ProxyChoice::Manual,
            found_count: 2,
            ..Default::default()
        };
        let geometry = settings_geometry(
            &size,
            1.0,
            area,
            0.0,
            0,
            4,
            Density::Standard,
            proxy,
            KeymapPaneState::default(),
        );
        let hit = |rect: (f32, f32, f32, f32)| {
            settings_hit(
                &size,
                1.0,
                area,
                rect.0 + rect.2 * 0.5,
                rect.1 + rect.3 * 0.5,
                true,
                NebulaSettingsSection::Proxy,
                0.0,
                None,
                0,
                0,
                0,
                0,
                4,
                Density::Standard,
                proxy,
                KeymapPaneState::default(),
                6,
                Default::default(),
            )
        };
        // 命中矩形与绘制矩形同源：用 hit-test 内部同款控件几何函数取靶。
        assert_eq!(
            hit(ssh_proxy_mode_control(geometry.ssh_proxy_mode, 1.0)),
            SettingsHit::SshProxyModeDropdown
        );
        let (protocol, address) = ssh_proxy_manual_controls(geometry.ssh_proxy_expand, 1.0);
        assert_eq!(hit(protocol), SettingsHit::SshProxyProtocolDropdown);
        assert_eq!(hit(address), SettingsHit::SshProxyInput(0));
        assert_eq!(
            hit(ssh_proxy_test_button(geometry.ssh_proxy_test, 1.0)),
            SettingsHit::SshProxyTest
        );
    }

    #[test]
    fn manual_proxy_expand_has_separate_protocol_and_address_hit_targets() {
        let size = proxy_test_size();
        let area = (0.0, 0.0, 1200.0, 900.0);
        let proxy = ProxyPaneState {
            mode: crate::ssh_proxy::ProxyMode::Custom,
            choice: ProxyChoice::Manual,
            found_count: 1,
            ..Default::default()
        };
        let geometry = settings_geometry(
            &size,
            1.0,
            area,
            0.0,
            0,
            4,
            Density::Standard,
            proxy,
            KeymapPaneState::default(),
        );
        let hit = |rect: (f32, f32, f32, f32)| {
            settings_hit(
                &size,
                1.0,
                area,
                rect.0 + rect.2 * 0.5,
                rect.1 + rect.3 * 0.5,
                true,
                NebulaSettingsSection::Proxy,
                0.0,
                None,
                0,
                0,
                0,
                0,
                4,
                Density::Standard,
                proxy,
                KeymapPaneState::default(),
                6,
                Default::default(),
            )
        };
        let (protocol, address) = ssh_proxy_manual_controls(geometry.ssh_proxy_expand, 1.0);
        assert_eq!(hit(protocol), SettingsHit::SshProxyProtocolDropdown);
        assert_eq!(hit(address), SettingsHit::SshProxyInput(0));
        assert!(protocol.0 + protocol.2 < address.0, "两个控件之间必须保留间距");
    }

    #[test]
    fn new_tab_position_defaults_compatibly_and_round_trips() {
        assert_eq!(NewTabPosition::default(), NewTabPosition::AfterCurrent);
        assert_eq!(NewTabPosition::parse("after_current"), Some(NewTabPosition::AfterCurrent));
        assert_eq!(NewTabPosition::parse("END"), Some(NewTabPosition::End));
        assert_eq!(
            NewTabPosition::parse("unknown").unwrap_or_default(),
            NewTabPosition::AfterCurrent
        );
        for value in [NewTabPosition::AfterCurrent, NewTabPosition::End] {
            assert_eq!(NewTabPosition::parse(value.settings_value()), Some(value));
        }
    }

    #[test]
    fn cell_width_mode_defaults_compatibly_and_round_trips() {
        assert_eq!(CellWidthMode::default(), CellWidthMode::Compact);
        assert_eq!(CellWidthMode::parse("compact"), Some(CellWidthMode::Compact));
        assert_eq!(CellWidthMode::parse("RELAXED"), Some(CellWidthMode::Relaxed));
        assert_eq!(CellWidthMode::parse("unknown").unwrap_or_default(), CellWidthMode::Compact);
        for value in [CellWidthMode::Compact, CellWidthMode::Relaxed] {
            assert_eq!(CellWidthMode::parse(value.settings_value()), Some(value));
        }
    }

    #[test]
    fn new_tab_position_dropdown_offers_both_choices_with_the_compatible_one_first() {
        // 列表顺序即下拉顺序，也是持久化索引的来源。把兼容默认放在首位是
        // 合同的一部分——重排这个数组会让升级用户的选择悄悄改变。
        assert_eq!(NEW_TAB_POSITION_OPTIONS.len(), 2);
        assert_eq!(NEW_TAB_POSITION_OPTIONS[0], NewTabPosition::default());
        assert_eq!(NEW_TAB_POSITION_OPTIONS[1], NewTabPosition::End);
        // 下拉与标签共用同一张表：每个选项都要能渲染出各自的文案。
        for option in NEW_TAB_POSITION_OPTIONS {
            assert!(!new_tab_position_label(option, UiLanguage::ZhCn).is_empty());
            assert!(!new_tab_position_label(option, UiLanguage::EnUs).is_empty());
        }
    }

    #[test]
    fn cell_width_mode_dropdown_offers_both_choices_with_the_compatible_one_first() {
        // 列表顺序即下拉顺序，也是持久化索引的来源；兼容默认必须排第一。
        assert_eq!(CELL_WIDTH_MODE_OPTIONS.len(), 2);
        assert_eq!(CELL_WIDTH_MODE_OPTIONS[0], CellWidthMode::default());
        assert_eq!(CELL_WIDTH_MODE_OPTIONS[1], CellWidthMode::Relaxed);
        for option in CELL_WIDTH_MODE_OPTIONS {
            assert!(!cell_width_mode_label(option, UiLanguage::ZhCn).is_empty());
            assert!(!cell_width_mode_label(option, UiLanguage::EnUs).is_empty());
        }
    }
}
