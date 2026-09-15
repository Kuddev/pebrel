use super::{LanguagePreference, Message, UiLanguage};

#[test]
fn language_metadata_and_settings_values_remain_in_sync() {
    assert_eq!(UiLanguage::ALL.len(), nebula_settings::LanguagePref::LANGUAGES.len());
    for preference in LanguagePreference::ALL {
        assert_eq!(LanguagePreference::parse(preference.as_str()), Some(*preference));
        if let Some(language) = preference.explicit() {
            assert_eq!(language.code(), preference.as_str());
            assert_eq!(preference.resolved(), language);
        }
    }
}

#[test]
fn negotiates_locale_and_falls_back_to_english() {
    assert_eq!(UiLanguage::for_locale(Some("fr_CA.UTF-8")), UiLanguage::FrFr);
    assert_eq!(UiLanguage::for_locale(Some("zh-Hant-HK")), UiLanguage::ZhTw);
    assert_eq!(UiLanguage::for_locale(Some("unsupported")), UiLanguage::EnUs);
    assert_eq!(UiLanguage::for_locale(None), UiLanguage::EnUs);
}

#[test]
fn typed_and_compatibility_lookups_agree() {
    assert_eq!(UiLanguage::FrFr.text(Message::SettingsSidebarNetwork), "Réseau");
    assert_eq!(UiLanguage::JaJp.tr("settings.sidebar.network"), "ネットワーク");
    for language in UiLanguage::ALL {
        assert_eq!(
            language.tr("settings.sidebar.network"),
            language.text(Message::SettingsSidebarNetwork)
        );
        assert_eq!(language.tr("missing.message.id"), "missing.message.id");
    }
}

#[test]
fn inline_migration_preserves_bilingual_text_and_english_fallback() {
    assert_eq!(UiLanguage::ZhCn.pick("网络", "Network"), "网络");
    assert_eq!(UiLanguage::EnUs.pick("网络", "Network"), "Network");
    assert_eq!(UiLanguage::FrFr.pick("网络", "Network"), "Réseau");
    assert_eq!(UiLanguage::FrFr.pick("未迁移文案", "Unmigrated text"), "Unmigrated text");
}

#[test]
fn arguments_are_localized_without_losing_placeholders() {
    assert_eq!(
        UiLanguage::EnUs.tr_args("provider.test.success", &[("status", "200")]),
        "Connection succeeded (HTTP 200)"
    );
    assert_eq!(
        UiLanguage::FrFr.tr_args("provider.test.success", &[("status", "200")]),
        "Connexion réussie (HTTP 200)"
    );
    assert_eq!(
        UiLanguage::FrFr.format(Message::ProviderTestSuccess, &[("status", "200")]),
        "Connexion réussie (HTTP 200)"
    );
}

#[test]
#[ignore = "manual static translation microbenchmark"]
fn measure_static_catalog_costs() {
    use std::hint::black_box;
    use std::time::Instant;

    let count = 2_000_000u128;
    let started = Instant::now();
    for _ in 0..count {
        black_box(black_box(UiLanguage::FrFr).text(black_box(Message::SettingsSidebarNetwork)));
    }
    eprintln!(
        "typed lookup: {} ns/op; {} messages; {} locales; {} translated bytes",
        started.elapsed().as_nanos() / count,
        super::MESSAGE_COUNT,
        UiLanguage::ALL.len(),
        super::TRANSLATED_BYTES
    );
    let started = Instant::now();
    for _ in 0..count {
        black_box(black_box(UiLanguage::FrFr).tr(black_box("settings.sidebar.network")));
    }
    eprintln!("key lookup: {} ns/op", started.elapsed().as_nanos() / count);
}

#[test]
fn settings_help_descriptions_are_localized_in_both_languages() {
    assert_eq!(
        UiLanguage::EnUs.tr("settings.help.background"),
        "Changes only the terminal background; Settings keeps the theme colors."
    );
    assert_eq!(
        UiLanguage::ZhCn.tr("settings.help.background"),
        "只更改终端底色，设置页仍跟随主题。"
    );
    assert_eq!(
        UiLanguage::ZhCn.tr("settings.help.keep_session_detail"),
        "关闭此项后，关窗会终止 Shell，未保存的工作可能丢失。开启后可重新附着到后台会话。"
    );
    assert_eq!(
        UiLanguage::EnUs.tr("settings.help.keep_session_detail"),
        "When disabled, closing the window terminates its shells and may lose unsaved work. When enabled, background sessions can be reattached."
    );
}

#[test]
fn settings_help_falls_back_to_english_for_untracked_languages() {
    let third_language = UiLanguage::FrFr;
    assert_eq!(
        third_language.tr("settings.help.background"),
        "Changes only the terminal background; Settings keeps the theme colors."
    );
    assert_eq!(
        third_language.tr("settings.help.keep_session_detail"),
        "When disabled, closing the window terminates its shells and may lose unsaved work. When enabled, background sessions can be reattached."
    );
}

#[test]
fn status_messages_are_localized_in_both_languages() {
    assert_eq!(
        UiLanguage::EnUs.tr("settings.status.provider_saved"),
        "Provider settings saved"
    );
    assert_eq!(
        UiLanguage::ZhCn.tr("settings.status.provider_saved"),
        "供应商配置已保存"
    );
    assert_eq!(
        UiLanguage::EnUs.tr("settings.status.codex_confirmation"),
        "Click again to confirm: the API key will be written in plain text to Codex auth.json (the original file will be backed up)"
    );
    assert_eq!(
        UiLanguage::ZhCn.tr("settings.status.codex_confirmation"),
        "再次点击确认：API Key 将明文写入 Codex auth.json（原文件会备份）"
    );
}

#[test]
fn about_page_messages_are_localized_in_both_languages() {
    assert_eq!(
        UiLanguage::EnUs.tr("settings.status.about_version_updates"),
        "Version and updates"
    );
    assert_eq!(
        UiLanguage::ZhCn.tr("settings.status.about_version_updates"),
        "版本与更新"
    );
    assert_eq!(
        UiLanguage::EnUs.tr("settings.status.about_report_issue"),
        "Report an issue"
    );
    assert_eq!(
        UiLanguage::ZhCn.tr("settings.status.about_gpu_tagline"),
        "GPU 加速终端 · Windows"
    );
}

#[test]
fn migrated_messages_fall_back_to_english_for_french() {
    assert_eq!(
        UiLanguage::FrFr.tr("settings.status.provider_saved"),
        "Provider settings saved"
    );
    assert_eq!(
        UiLanguage::FrFr.tr("settings.status.backup_restored"),
        "Backup restored (some settings, including fonts and tray options, apply after restart)"
    );
    assert_eq!(
        UiLanguage::FrFr.tr("settings.help.blur_detail"),
        "Mica effects use fewer resources. Aero and Acrylic blur live content behind the window and cost more."
    );
}
