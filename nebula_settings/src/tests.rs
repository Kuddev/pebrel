    #[test]
    fn environment_refresh_defaults_and_round_trips() {
        for text in ["", "refresh_environment=\n", "refresh_environment=invalid\n"] {
            assert!(RuntimeSettings::from_raw(&RawSettings::from_text(text)).refresh_environment);
        }
        let original = "theme=Nord\nrefresh_environment=1\ncustom=keep\n";
        let disabled = apply_updates(original, &[("refresh_environment", "0".into())]);
        assert!(!RuntimeSettings::from_raw(&RawSettings::from_text(&disabled)).refresh_environment);
        assert!(disabled.contains("custom=keep"));
        let enabled = apply_updates(&disabled, &[("refresh_environment", "1".into())]);
        assert!(RuntimeSettings::from_raw(&RawSettings::from_text(&enabled)).refresh_environment);
        assert_eq!(enabled, original);
    }

    #[test]
    fn pane_preferences_round_trip_and_allow_a_missing_mouse_override() {
        let defaults = RuntimeSettings::from_raw(&RawSettings::default());
        assert_eq!(defaults.focus_follows_mouse, None);
        assert!(defaults.dim_inactive_panes);
        let mut text = "theme=Nord\nfuture_option=keep\n".to_owned();
        for (focus, dim) in [(true, false), (false, true)] {
            text = apply_updates(
                &text,
                &[
                    ("focus_follows_mouse", (focus as u8).to_string()),
                    ("dim_inactive_panes", (dim as u8).to_string()),
                ],
            );
            let settings = RuntimeSettings::from_raw(&RawSettings::from_text(&text));
            assert_eq!(settings.focus_follows_mouse, Some(focus));
            assert_eq!(settings.dim_inactive_panes, dim);
            assert_eq!(settings.theme, ThemeName::Nord);
            assert!(text.contains("future_option=keep"));
        }
        text = apply_updates(&text, &[("focus_follows_mouse", String::new())]);
        assert_eq!(
            RuntimeSettings::from_raw(&RawSettings::from_text(&text)).focus_follows_mouse,
            None
        );
    }

    #[test]
    fn empty_quick_terminal_hotkey_is_distinct_from_an_unset_preference() {
        let unset = RuntimeSettings::from_raw(&RawSettings::from_text("theme=Nord\n"));
        let cleared =
            RuntimeSettings::from_raw(&RawSettings::from_text("quick_terminal_hotkey=\n"));
        assert_eq!(unset.quick_terminal_hotkey, DEFAULT_QUICK_TERMINAL_HOTKEY);
        assert_eq!(cleared.quick_terminal_hotkey, "");
    }
    use super::*;

    #[test]
    fn app_icon_defaults_and_unknown_values_use_titanium() {
        for text in ["", "app_icon=\n", "app_icon=unknown\n", "app_icon=../custom.ico\n"] {
            let settings = RuntimeSettings::from_raw(&RawSettings::from_text(text));
            assert_eq!(settings.app_icon, AppIconName::Titanium);
        }
    }

    #[test]
    fn all_app_icons_round_trip_without_changing_other_settings() {
        for variant in AppIconName::ALL {
            let text = apply_updates(
                "# keep\ntheme=Nord\ncustom=preserved\napp_icon=silver-violet\n",
                &[("app_icon", variant.settings_value().to_owned())],
            );
            let settings = RuntimeSettings::from_raw(&RawSettings::from_text(&text));
            assert_eq!(settings.app_icon, variant);
            assert_eq!(settings.theme, ThemeName::Nord);
            assert!(text.contains("custom=preserved"));
            assert!(text.contains("# keep"));
            assert_eq!(text.matches("app_icon=").count(), 1);
        }
        assert_eq!(AppIconName::from_settings(" TITANIUM "), Some(AppIconName::Titanium));
        assert_eq!(AppIconName::from_settings("night-mint"), Some(AppIconName::NightMint));
        let numbers: std::collections::HashSet<_> =
            AppIconName::ALL.into_iter().map(|variant| variant.palette().number).collect();
        assert_eq!(numbers.len(), 25);
    }

    #[test]
    fn nord_and_paper_carry_the_flush_card_geometry() {
        // Nord 是卡几何抽象的第一个实验者：半径与卡缝双双归零、靠一条竖线分界。
        // 这四个数一起构成「终端铺满整个右侧区域」的形态，任一项退回默认都会
        // 让它变回浮起的圆角卡——所以逐项钉死，而不是只断言 radius。
        for theme in ThemeName::BUILTIN {
            let geometry = theme.card_geometry();
            assert_eq!(geometry.radius, 0.0);
            assert_eq!(geometry.gutter, 0.0);
            assert_eq!(geometry.divider, 1.0);
            assert!(!geometry.shadow);
        }

        // Retired identifiers also resolve to the flat geometry contract.
        for theme in [ThemeName::Nebula, ThemeName::CoalDark] {
            let geometry = theme.card_geometry();
            assert_eq!(geometry.radius, 0.0);
            assert_eq!(geometry.gutter, 0.0);
            assert_eq!(geometry.divider, 1.0);
        }
    }

    #[test]
    fn card_geometry_keys_stay_none_until_the_user_sets_them() {
        // `None` 的含义是「用户没设过」，主题默认因此能生效。若在解析处就
        // unwrap 成具体数字，切主题便再也换不了形态——那是这套抽象的失效模式。
        let untouched = RuntimeSettings::from_raw(&RawSettings::from_text("theme=Nord\n"));
        assert_eq!(untouched.pane_card_radius, None);
        assert_eq!(untouched.pane_card_gutter, None);
        assert_eq!(untouched.pane_card_shadow, None);
        assert_eq!(untouched.pane_card_divider, None);

        // 显式值覆盖主题默认，且越界值按范围钳制而不是被丢弃。
        let tuned = RuntimeSettings::from_raw(&RawSettings::from_text(
            "pane_card_radius=999\n\
             pane_card_gutter=12\n\
             pane_card_shadow=on\n\
             pane_card_divider=99\n",
        ));
        assert_eq!(tuned.pane_card_radius, Some(MAX_PANE_CARD_RADIUS));
        assert_eq!(tuned.pane_card_gutter, Some(12.0));
        assert_eq!(tuned.pane_card_shadow, Some(true));
        assert_eq!(tuned.pane_card_divider, Some(MAX_PANE_CARD_DIVIDER));
    }

    #[test]
    fn parses_real_settings_shape() {
        let raw = RawSettings::from_text(
            "language=zh-CN\n\
             theme=SilverLight\n\
             follow_system_theme=0\n\
             ghost=1\n\
             accept=tab\n\
             completion_style=popup\n\
             shell=pwsh\n\
             font_family=Maple Mono Normal NF CN\n\
             ui_font_family=Arial\n\
             ui_font_size=18\n\
             font_family_cjk=PingFang SC\n\
             font_size=16.3\n\
             cursor_shape=beam\n\
             cursor_blink=1\n\
             copy_on_select=1\n\
             multiline_paste_confirm=0\n\
             tab_close_visible=0\n\
             static_tab_titles=1\n\
             bell=audible\n\
             cjk_bold_regular=1\n\
             tabs_position=top\n\
             tab_reveal=instant\n\
             density=compact\n\
             new_tab_position=end\n\
             windowing_behavior=use_existing\n\
             cell_width_mode=relaxed\n\
             fetch=1\n\
             powerline=0\n\
             keep_session=1\n\
             silent_start=1\n\
             restore_session=0\n\
             resume_ai=0\n\
             tray=0\n\
             blur=0\n\
             opacity=0.87\n\
             background=#101216\n\
             theme_foreground=#d6dae6\n\
             custom_theme=my-night\n\
             panel_resize=1\n\
             sidebar_w=222\n\
             ssh_proxy_mode=custom\n\
             ssh_proxy_url=socks5://127.0.0.1:7890\n\
             quick_terminal_hotkey=alt+`\n\
             startup_directory=\n",
        );
        let settings = RuntimeSettings::from_raw(&raw);
        assert_eq!(settings.language, LanguagePref::ZhCn);
        assert_eq!(settings.theme, ThemeName::SilverLight);
        assert_eq!(settings.font_family.as_deref(), Some("Maple Mono Normal NF CN"));
        assert_eq!(settings.font_family_cjk.as_deref(), Some("PingFang SC"));
        assert_eq!(settings.ui_font_family.as_deref(), Some("Arial"));
        assert_eq!(settings.ui_font_size_px, Some(18.0));
        // font_size 键存的是逻辑像素（旧壳写盘语义），不做 pt 换算。
        assert_eq!(settings.font_size_px, Some(16.3));
        assert_eq!(settings.cursor_shape, Some(CursorShapeName::Beam));
        assert_eq!(settings.cursor_blink, Some(true));
        assert!(settings.copy_on_select);
        assert!(!settings.multiline_paste_confirm);
        assert!(!settings.tab_close_visible);
        assert!(settings.static_tab_titles, "static_tab_titles=1 固定标签标题为目录名");
        assert_eq!(settings.bell, BellModeName::Audible);
        assert!(!settings.powerline);
        assert_eq!(settings.shell.as_deref(), Some("pwsh"));
        assert_eq!(settings.accept, AcceptKeyName::Tab);
        assert_eq!(settings.completion_style, CompletionStyleName::Popup);
        assert_eq!(settings.tabs_position, TabsPositionName::Top);
        assert_eq!(settings.tab_reveal, TabRevealName::Instant);
        assert_eq!(settings.density, DensityName::Compact);
        assert_eq!(settings.new_tab_position, NewTabPositionName::End);
        assert_eq!(settings.windowing_behavior, WindowingBehaviorName::UseExisting);
        assert_eq!(settings.cell_width_mode, CellWidthModeName::Relaxed);
        assert!(settings.fetch);
        assert!(settings.keep_session);
        assert!(settings.silent_start);
        assert!(!settings.restore_session);
        assert!(!settings.resume_ai);
        assert!(!settings.tray);
        assert_eq!(settings.blur, BlurModeName::None);
        assert!((settings.opacity - 0.87).abs() < 1e-6);
        assert_eq!(settings.background, Some([0x10, 0x12, 0x16]));
        assert_eq!(settings.theme_foreground, Some([0xd6, 0xda, 0xe6]));
        assert_eq!(settings.custom_theme.as_deref(), Some("my-night"));
        assert!(settings.panel_resize);
        assert_eq!(settings.sidebar_width, 222.0);
        assert_eq!(settings.ssh_proxy_mode, ProxyModeName::Custom);
        assert_eq!(settings.ssh_proxy_url, "socks5://127.0.0.1:7890");
        assert_eq!(settings.quick_terminal_hotkey, "alt+`");
        // 空值 = 未设置。
        assert_eq!(raw.value("startup_directory"), None);
    }

    #[test]
    fn defaults_when_file_content_is_absent_or_junk() {
        let settings = RuntimeSettings::from_raw(&RawSettings::from_text("theme=NoSuchTheme\n"));
        // 出厂默认逐项对照旧壳 nebula_settings_load。
        assert_eq!(settings.language, LanguagePref::System);
        assert_eq!(settings.theme, ThemeName::Nord);
        assert_eq!(settings.font_family, None);
        assert_eq!(settings.font_family_cjk, None);
        assert_eq!(settings.ui_font_family, None);
        assert_eq!(settings.ui_font_size_px, None);
        assert!(!settings.copy_on_select);
        assert!(settings.multiline_paste_confirm, "多行粘贴确认默认开（上游兼容）");
        assert!(settings.tab_close_visible, "标签关闭按钮默认可见（上游兼容）");
        assert!(!settings.static_tab_titles, "标签标题默认跟随 AI 任务标题（上游行为）");
        assert!(!settings.terminal_proxy, "终端网络代理上游默认关（兼容）");
        assert!(settings.powerline);
        assert!(settings.ghost);
        assert_eq!(settings.accept, AcceptKeyName::Both);
        assert_eq!(settings.completion_style, CompletionStyleName::Inline);
        assert!(settings.cjk_bold_regular);
        assert_eq!(settings.tabs_position, TabsPositionName::Sidebar);
        assert_eq!(settings.tab_reveal, TabRevealName::Slide);
        assert_eq!(settings.density, DensityName::Standard);
        assert_eq!(settings.new_tab_position, NewTabPositionName::AfterCurrent);
        assert_eq!(settings.windowing_behavior, WindowingBehaviorName::UseAnyExisting);
        assert_eq!(settings.cell_width_mode, CellWidthModeName::Compact);
        assert!(!settings.fetch);
        assert!(settings.auto_check_updates);
        assert!(!settings.keep_session);
        assert!(!settings.silent_start);
        assert!(settings.restore_session);
        assert!(settings.resume_ai);
        assert!(settings.tray);
        assert_eq!(settings.blur, BlurModeName::None);
        assert_eq!(settings.opacity, 1.0);
        assert_eq!(settings.background, None);
        assert_eq!(settings.theme_foreground, None);
        assert_eq!(settings.custom_theme, None);
        assert!(settings.panel_resize);
        assert!(
            !RuntimeSettings::from_raw(&RawSettings::from_text("panel_resize=0\n")).panel_resize
        );
        assert_eq!(settings.sidebar_width, DEFAULT_SIDEBAR_WIDTH);
        assert_eq!(settings.ssh_proxy_mode, ProxyModeName::Off);
        assert_eq!(settings.quick_terminal_hotkey, DEFAULT_QUICK_TERMINAL_HOTKEY);
        assert_eq!(settings.bell, BellModeName::Both);
    }

    #[test]
    fn automatic_update_checks_can_be_disabled_without_changing_the_default() {
        assert!(RuntimeSettings::from_raw(&RawSettings::default()).auto_check_updates);
        assert!(
            RuntimeSettings::from_raw(&RawSettings::from_text("auto_check_updates=1\n"))
                .auto_check_updates
        );
        assert!(
            !RuntimeSettings::from_raw(&RawSettings::from_text("auto_check_updates=0\n"))
                .auto_check_updates
        );
    }

    #[test]
    fn ai_toasts_default_on_and_accept_all_boolean_spellings() {
        for value in ["", "invalid", "1", "true", "YES", "On"] {
            let raw = RawSettings::from_text(&format!("ai_toasts={value}\n"));
            assert!(RuntimeSettings::from_raw(&raw).ai_toasts, "{value:?}");
        }
        assert!(RuntimeSettings::from_raw(&RawSettings::default()).ai_toasts);
        for value in ["0", "false", "NO", "Off"] {
            let raw = RawSettings::from_text(&format!("ai_toasts={value}\n"));
            let settings = RuntimeSettings::from_raw(&raw);
            assert!(!settings.ai_toasts, "{value:?}");
            assert_eq!(settings.bell, BellModeName::Both);
            assert!(settings.auto_check_updates);
            assert!(settings.resume_ai);
        }
    }

    #[test]
    fn ai_toast_toggle_round_trips_without_replacing_other_settings() {
        let original = "# preferences\nshell=zsh\ncustom_key=keep\nAI_TOASTS=1\n";
        let disabled = apply_updates(original, &[("ai_toasts", "0".to_owned())]);
        let restored = RuntimeSettings::from_raw(&RawSettings::from_text(&disabled));
        assert!(!restored.ai_toasts);
        assert_eq!(restored.shell.as_deref(), Some("zsh"));
        assert!(disabled.contains("# preferences\n"));
        assert!(disabled.contains("custom_key=keep\n"));
        let enabled = apply_updates(&disabled, &[("ai_toasts", "1".to_owned())]);
        assert!(RuntimeSettings::from_raw(&RawSettings::from_text(&enabled)).ai_toasts);
        assert_eq!(enabled.matches("ai_toasts=").count(), 1);
    }

    #[test]
    fn ctrl_wheel_font_zoom_defaults_on_and_accepts_all_boolean_spellings() {
        assert!(RuntimeSettings::from_raw(&RawSettings::default()).ctrl_wheel_font_zoom);
        for value in ["", "invalid", "1", "true", "YES", "On"] {
            let raw = RawSettings::from_text(&format!("ctrl_wheel_font_zoom={value}\n"));
            assert!(RuntimeSettings::from_raw(&raw).ctrl_wheel_font_zoom, "{value:?}");
        }
        for value in ["0", "false", "NO", "Off"] {
            let raw = RawSettings::from_text(&format!("ctrl_wheel_font_zoom={value}\n"));
            let settings = RuntimeSettings::from_raw(&raw);
            assert!(!settings.ctrl_wheel_font_zoom, "{value:?}");
            assert_eq!(settings.bell, BellModeName::Both);
            assert!(settings.font_size_px.is_none());
        }
    }

    #[test]
    fn bell_mode_parses_known_values_and_rejects_junk() {
        assert_eq!(BellModeName::from_settings("none"), Some(BellModeName::None));
        assert_eq!(BellModeName::from_settings("off"), Some(BellModeName::None));
        assert_eq!(BellModeName::from_settings("visual"), Some(BellModeName::Visual));
        assert_eq!(BellModeName::from_settings("audible"), Some(BellModeName::Audible));
        assert_eq!(BellModeName::from_settings("both"), Some(BellModeName::Both));
        assert_eq!(BellModeName::from_settings("loud"), None);
        assert_eq!(
            RuntimeSettings::from_raw(&RawSettings::from_text("bell=loud\n")).bell,
            BellModeName::Both
        );
    }

    #[test]
    fn blur_mode_parses_mica_alt_and_migrates_legacy_bool() {
        assert_eq!(BlurModeName::from_settings("none"), Some(BlurModeName::None));
        assert_eq!(BlurModeName::from_settings("off"), Some(BlurModeName::None));
        assert_eq!(BlurModeName::from_settings("mica"), Some(BlurModeName::Mica));
        assert_eq!(BlurModeName::from_settings("mica-alt"), Some(BlurModeName::MicaAlt));
        assert_eq!(BlurModeName::from_settings("aero"), Some(BlurModeName::Aero));
        // 未公开 API 的名字（`ACCENT_ENABLE_BLURBEHIND`）也认，手改配置的人
        // 可能照着实现名写。
        assert_eq!(BlurModeName::from_settings("blurbehind"), Some(BlurModeName::Aero));
        assert_eq!(BlurModeName::from_settings("acrylic"), Some(BlurModeName::Acrylic));
        assert_eq!(BlurModeName::from_settings("frosted"), None);
        // 大小写与空白由 from_settings 归一（设置文件是手改的）。
        assert_eq!(BlurModeName::from_settings("  Acrylic "), Some(BlurModeName::Acrylic));
        assert_eq!(BlurModeName::from_settings(" AERO "), Some(BlurModeName::Aero));

        // 旧壳布尔值迁移：关保持关，开落到 Mica（不是最贵的 Acrylic）。
        assert_eq!(BlurModeName::from_settings("0"), Some(BlurModeName::None));
        assert_eq!(BlurModeName::from_settings("false"), Some(BlurModeName::None));
        assert_eq!(BlurModeName::from_settings("1"), Some(BlurModeName::Mica));
        assert_eq!(BlurModeName::from_settings("true"), Some(BlurModeName::Mica));
        assert_eq!(
            RuntimeSettings::from_raw(&RawSettings::from_text("blur=1\nopacity=1.00\n")).opacity,
            1.0
        );
        assert_eq!(
            RuntimeSettings::from_raw(&RawSettings::from_text("blur=mica\nopacity=1.00\n")).opacity,
            1.0
        );

        // 认不出的值回落到默认关闭，不主动启用系统材质。
        assert_eq!(
            RuntimeSettings::from_raw(&RawSettings::from_text("blur=frosted\n")).blur,
            BlurModeName::None
        );
        assert_eq!(
            RuntimeSettings::from_raw(&RawSettings::from_text("blur=acrylic\n")).blur,
            BlurModeName::Acrylic
        );

        // settings_value 与 from_settings 必须互为逆运算，否则设置页存盘后
        // 重开会掉档。
        for mode in [
            BlurModeName::None,
            BlurModeName::Mica,
            BlurModeName::MicaAlt,
            BlurModeName::Aero,
            BlurModeName::Acrylic,
        ] {
            assert_eq!(BlurModeName::from_settings(mode.settings_value()), Some(mode));
        }

        assert!(!BlurModeName::None.enabled());
        assert!(BlurModeName::Mica.enabled());
        assert!(BlurModeName::MicaAlt.enabled());
        assert!(BlurModeName::Aero.enabled());
        assert!(BlurModeName::Acrylic.enabled());
    }

    #[test]
    fn opacity_defaults_to_opaque_and_preserves_explicit_values_for_every_material() {
        for blur in ["none", "mica", "mica-alt", "aero", "acrylic", "1", "true"] {
            let raw = format!("blur={blur}\n");
            let runtime = RuntimeSettings::from_raw(&RawSettings::from_text(&raw));
            assert_eq!(runtime.opacity, 1.0, "{blur}");
            assert_eq!(Some(runtime.blur), BlurModeName::from_settings(blur));
            for opacity in [0.0, 0.65, 1.0] {
                let raw = format!("blur={blur}\nopacity={opacity}\n");
                let runtime = RuntimeSettings::from_raw(&RawSettings::from_text(&raw));
                assert_eq!(runtime.opacity, opacity, "{raw}");
            }
        }
        let invalid =
            RuntimeSettings::from_raw(&RawSettings::from_text("blur=unknown\nopacity=invalid\n"));
        assert_eq!(invalid.blur, BlurModeName::None);
        assert_eq!(invalid.opacity, 1.0);
    }

    #[test]
    fn tabs_position_parses_supported_values_and_defaults_to_sidebar() {
        assert_eq!(TabsPositionName::from_settings("sidebar"), Some(TabsPositionName::Sidebar));
        assert_eq!(TabsPositionName::from_settings("TOP"), Some(TabsPositionName::Top));
        assert_eq!(TabsPositionName::from_settings("bottom"), None);
        assert_eq!(
            RuntimeSettings::from_raw(&RawSettings::from_text("tabs_position=bottom\n"))
                .tabs_position,
            TabsPositionName::Sidebar
        );
    }

    #[test]
    fn windowing_behavior_preserves_existing_default_and_accepts_terminal_aliases() {
        assert_eq!(
            WindowingBehaviorName::from_settings("use_new"),
            Some(WindowingBehaviorName::UseNew)
        );
        assert_eq!(
            WindowingBehaviorName::from_settings("useAnyExisting"),
            Some(WindowingBehaviorName::UseAnyExisting)
        );
        assert_eq!(
            WindowingBehaviorName::from_settings("useExisting"),
            Some(WindowingBehaviorName::UseExisting)
        );
        assert_eq!(WindowingBehaviorName::from_settings("reuse_everything"), None);
        assert_eq!(
            RuntimeSettings::from_raw(&RawSettings::default()).windowing_behavior,
            WindowingBehaviorName::UseAnyExisting
        );
    }

    #[test]
    fn hex_rgb_roundtrip() {
        assert_eq!(parse_hex_rgb("#8bd5ca"), Some([0x8b, 0xd5, 0xca]));
        assert_eq!(parse_hex_rgb("8bd5ca"), Some([0x8b, 0xd5, 0xca]));
        assert_eq!(parse_hex_rgb("#nothex"), None);
        assert_eq!(format_hex_rgb([0x8b, 0xd5, 0xca]), "#8bd5ca");
    }

    #[test]
    fn shorthand_rgb_expands_and_rejects_non_hex_or_alpha_input() {
        for value in ["#123", "123", "  #123  "] {
            assert_eq!(parse_hex_rgb(value), Some([0x11, 0x22, 0x33]), "{value:?}");
        }
        assert_eq!(parse_hex_rgb("#aBc"), Some([0xaa, 0xbb, 0xcc]));
        assert_eq!(format_hex_rgb(parse_hex_rgb("#aBc").unwrap()), "#aabbcc");
        for value in ["", "#", "#12", "#1234", "#12345", "#12345678", "#12g", "+1b2c3", "中文"] {
            assert_eq!(parse_hex_rgb(value), None, "{value:?}");
        }
    }

    #[test]
    fn theme_names_roundtrip() {
        assert_eq!(ThemeName::Nebula.prompt_name(), "Nebula");
        assert_eq!(ThemeName::from_prompt_name("Nebula"), Some(ThemeName::Nebula));
        for theme in [
            ThemeName::Nebula,
            ThemeName::SilverLight,
            ThemeName::SteelDark,
            ThemeName::LimestoneLight,
            ThemeName::CoalDark,
            ThemeName::LinenLight,
            ThemeName::MossDark,
            ThemeName::Nord,
            ThemeName::Paper,
        ] {
            assert_eq!(ThemeName::from_prompt_name(theme.prompt_name()), Some(theme));
        }
    }

    #[test]
    fn light_themes_replace_foreground_dark_themes_do_not() {
        assert!(ThemeName::SilverLight.term_theme().is_light);
        assert!(ThemeName::LimestoneLight.term_theme().is_light);
        assert!(ThemeName::LinenLight.term_theme().is_light);
        assert!(ThemeName::Paper.term_theme().is_light);
        assert!(!ThemeName::Nebula.term_theme().is_light);
        assert!(!ThemeName::SteelDark.term_theme().is_light);
        assert!(!ThemeName::Nord.term_theme().is_light);
        // 浅色主题终端底全白，背景由主题裁定而非用户配色。
        assert_eq!(ThemeName::SilverLight.term_theme().background, [255, 255, 255]);
        assert_eq!(ThemeName::Nebula.term_theme().background, [15, 17, 26]);
    }

    #[test]
    fn nord_and_warm_sand_keep_their_declared_terminal_palettes() {
        let nord = ThemeName::Nord.term_theme().exact.expect("Nord exact colors");
        assert_eq!(nord.foreground, [0xf1, 0xf6, 0xff]);
        assert_eq!(nord.ansi[0], [0x3b, 0x42, 0x52]);
        assert_eq!(nord.ansi[15], [0xec, 0xef, 0xf4]);
        assert_eq!(nord.cursor, Some([0xe5, 0xe9, 0xf0]));
        assert_eq!(nord.cursor_stroke, Some([0x88, 0xc0, 0xd0]));
        assert_eq!(nord.selection_background, Some([0xe5, 0xe9, 0xf0]));

        let paper = ThemeName::Paper.term_theme().exact.expect("Warm Sand exact colors");
        assert_eq!(paper.foreground, [0x2f, 0x2e, 0x2b]);
        assert_eq!(paper.ansi[0], [0x2f, 0x2e, 0x2b]);
        assert_eq!(paper.ansi[15], [0x4a, 0x43, 0x3b]);
        assert_eq!(paper.cursor, None);
        assert_eq!(paper.selection_background, None);
    }

    #[test]
    fn existing_themes_keep_their_original_terminal_color_contract() {
        for theme in [
            ThemeName::Nebula,
            ThemeName::SilverLight,
            ThemeName::SteelDark,
            ThemeName::LimestoneLight,
            ThemeName::CoalDark,
            ThemeName::LinenLight,
            ThemeName::MossDark,
        ] {
            assert_eq!(theme.term_theme().exact, None, "{}", theme.prompt_name());
        }
    }

    #[test]
    fn keys_are_case_insensitive_and_trimmed() {
        let raw = RawSettings::from_text("  Theme = SilverLight \nFONT_SIZE=12\n");
        assert_eq!(raw.value("theme"), Some("SilverLight"));
        assert_eq!(raw.f32("font_size"), Some(12.0));
    }

    #[test]
    fn apply_updates_replaces_in_place_and_appends_missing() {
        let text = "language=system\ntheme=Nebula\nshell=powershell\n";
        let updated =
            apply_updates(text, &[("theme", "SilverLight".into()), ("cursor_blink", "1".into())]);
        assert_eq!(
            updated,
            "language=system\ntheme=SilverLight\nshell=powershell\ncursor_blink=1\n"
        );
    }

    #[test]
    fn keybind_pairs_parse_and_rewrite() {
        let text = "theme=Nebula\nkeybind=ctrl+x:Copy\n# comment\nkeybind=alt+f1:SplitRight\n";
        assert_eq!(
            keybind_pairs_from_text(text),
            vec![
                ("ctrl+x".to_owned(), "Copy".to_owned()),
                ("alt+f1".to_owned(), "SplitRight".to_owned()),
            ]
        );
        let rewritten = apply_keybinds(text, &[("ctrl+y".to_owned(), "Paste".to_owned())]);
        assert_eq!(rewritten, "theme=Nebula\n# comment\nkeybind=ctrl+y:Paste\n");
    }

    #[test]
    fn apply_updates_preserves_unknown_lines_and_matches_case_insensitively() {
        let text = "# comment survives\nTHEME=CoalDark\nkeybind=ctrl+x=Copy\n";
        let updated = apply_updates(text, &[("theme", "MossDark".into())]);
        // 键名按调用方写法输出，注释与奇形行原样保留。
        assert_eq!(updated, "# comment survives\ntheme=MossDark\nkeybind=ctrl+x=Copy\n");
    }

    #[test]
    fn apply_updates_on_empty_file_appends_all() {
        let updated =
            apply_updates("", &[("theme", "Nebula".into()), ("font_size", "12.5".into())]);
        assert_eq!(updated, "theme=Nebula\nfont_size=12.5\n");
    }
