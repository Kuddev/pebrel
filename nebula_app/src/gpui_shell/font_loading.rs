//! 字体注册按应用生命周期保留；图标不触发完整 Maple 字库的加载。

use std::borrow::Cow;

use gpui::{App, Global};

use crate::font_install::{REQUIRED_FONT_BYTES, REQUIRED_FONT_FAMILY, font_family_chain};

#[derive(Default)]
struct MapleRegistration(bool);
impl Global for MapleRegistration {}

fn requests_maple(families: &str) -> bool {
    font_family_chain(families).iter().any(|name| name.eq_ignore_ascii_case(REQUIRED_FONT_FAMILY))
}

pub(super) fn ensure_selected(cx: &mut App) {
    if cx.default_global::<MapleRegistration>().0 {
        return;
    }
    let needed = cx.try_global::<super::config::Settings>().is_some_and(|settings| {
        [
            settings.font_family.as_str(),
            settings.font_bold_family.as_str(),
            settings.font_italic_family.as_str(),
            settings.font_bold_italic_family.as_str(),
            settings.ui_font_family.as_deref().unwrap_or_default(),
        ]
        .into_iter()
        .any(requests_maple)
            || settings.font_cjk.as_ref().is_some_and(|fonts| {
                fonts.iter().any(|font| {
                    requests_maple(&font.family)
                        || font.fallbacks.as_ref().is_some_and(|fallbacks| {
                            fallbacks.fallback_list().iter().any(|family| requests_maple(family))
                        })
                })
            })
    });
    if needed {
        ensure_family(REQUIRED_FONT_FAMILY, cx);
    }
}

pub(super) fn ensure_family(families: &str, cx: &mut App) {
    if cx.default_global::<MapleRegistration>().0 || !requests_maple(families) {
        return;
    }
    // 设置、草稿预览和终端创建共用注册状态，不在逐字形绘制时重建字体集合。
    match cx.text_system().add_fonts(vec![Cow::Borrowed(REQUIRED_FONT_BYTES)]) {
        Ok(()) => cx.default_global::<MapleRegistration>().0 = true,
        Err(error) => super::try_write_stderr(format_args!(
            "[pebrel:gpui] failed to register selected Maple font: {error}"
        )),
    }
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests {
    use super::*;

    #[gpui::test]
    fn draft_font_registration_does_not_require_applying_the_draft(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            ensure_family("Consolas", cx);
            assert!(!cx.global::<MapleRegistration>().0);
            ensure_family(REQUIRED_FONT_FAMILY, cx);
            assert!(cx.global::<MapleRegistration>().0);
            assert!(cx.try_global::<super::super::config::Settings>().is_none());
        });
    }

    #[gpui::test]
    fn complete_maple_is_loaded_only_when_a_configured_font_needs_it(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(|cx| {
            let mut settings =
                super::super::config::Settings::load(nebula_settings::ThemeName::Nord);
            settings.font_family = "Consolas".into();
            settings.font_bold_family = "Consolas".into();
            settings.font_italic_family = "Consolas".into();
            settings.font_bold_italic_family = "Consolas".into();
            settings.font_cjk = None;
            settings.ui_font_family = None;
            cx.set_global(settings);
            ensure_selected(cx);
            assert!(!cx.global::<MapleRegistration>().0);
            cx.global_mut::<super::super::config::Settings>().font_family =
                format!("Consolas, {REQUIRED_FONT_FAMILY}");
            ensure_selected(cx);
            assert!(cx.global::<MapleRegistration>().0);
        });
    }
}
