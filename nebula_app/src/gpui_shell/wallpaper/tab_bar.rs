//! Tab surfaces own their alpha; the native compositor still owns one backdrop.
use super::*;

pub(crate) fn tab_bar_material_is_custom(cx: &App) -> bool {
    cx.try_global::<VisualEffects>().is_some_and(|effects| {
        effects.tab_bar_material.opacity.is_some() || effects.tab_bar_material.blur.is_some()
    })
}

pub(crate) fn tab_bar_surface_opacity(cx: &App) -> f32 {
    let Some(effects) = cx.try_global::<VisualEffects>() else { return 1.0 };
    effects.tab_bar_material.opacity.unwrap_or_else(|| chrome_surface_opacity(cx))
}

/// A different backdrop is safe only when all non-tab surfaces cover it. When
/// both regions are translucent, retain the window's material and report the
/// unsupported combination instead of silently changing the terminal backdrop.
pub(super) fn resolve_blur(
    window_blur: BlurModeName,
    tab_blur: Option<BlurModeName>,
    body_opaque: bool,
) -> (BlurModeName, Option<BlurModeName>) {
    match tab_blur {
        Some(requested) if requested != window_blur && !body_opaque => {
            (window_blur, Some(requested))
        },
        Some(requested) => (requested, None),
        None => (window_blur, None),
    }
}

pub(super) fn native_blur(cx: &App) -> (BlurModeName, Option<BlurModeName>) {
    let Some(effects) = cx.try_global::<VisualEffects>() else {
        return (BlurModeName::None, None);
    };
    resolve_blur(
        effects.blur,
        effects.tab_bar_material.blur,
        chrome_surface_opacity(cx) >= 1.0 && card_surface_opacity(cx) >= 1.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_content_allows_tab_only_blur_but_translucent_content_keeps_its_material() {
        for tab_blur in [BlurModeName::Acrylic, BlurModeName::Mica, BlurModeName::Aero] {
            assert_eq!(resolve_blur(BlurModeName::None, Some(tab_blur), true), (tab_blur, None));
            assert_eq!(
                resolve_blur(BlurModeName::None, Some(tab_blur), false),
                (BlurModeName::None, Some(tab_blur))
            );
            assert_eq!(resolve_blur(tab_blur, Some(tab_blur), false), (tab_blur, None));
            assert_eq!(resolve_blur(tab_blur, None, false), (tab_blur, None));
        }
        assert_eq!(
            resolve_blur(BlurModeName::Acrylic, Some(BlurModeName::None), true),
            (BlurModeName::None, None)
        );
    }

    #[cfg(feature = "gpui-test-support")]
    #[gpui::test]
    fn tab_alpha_and_live_window_opacity_remain_independent(cx: &mut gpui::TestAppContext) {
        use gpui_component::ActiveTheme as _;
        cx.update(gpui_component::init);
        cx.update(|cx| {
            test_install_visual_effects(cx, 1.0, BlurModeName::None);
            cx.global_mut::<VisualEffects>().tab_bar_material = nebula_settings::TabBarMaterial {
                opacity: Some(0.6),
                blur: Some(BlurModeName::Acrylic),
            };
            assert_eq!(tab_bar_surface_opacity(cx), 0.6);
            let foreground = cx.theme().foreground;
            let shell = cx.theme().background;
            let tab_color = crate::gpui_shell::theme::tab_bar_background(cx);
            assert_eq!((tab_color.h, tab_color.s, tab_color.l), (shell.h, shell.s, shell.l));
            assert_eq!(tab_color.a, 0.6);
            assert_eq!(crate::gpui_shell::theme::card_content_bg(cx).a, 1.0);
            assert_eq!(chrome_surface_opacity(cx), 1.0);
            assert_eq!(card_surface_opacity(cx), 1.0);
            assert_eq!(native_blur(cx), (BlurModeName::Acrylic, None));
            set_opacity_live(0.7, cx);
            assert_eq!(tab_bar_surface_opacity(cx), 0.6);
            assert_eq!(card_surface_opacity(cx), 0.7);
            assert_eq!(crate::gpui_shell::theme::tab_bar_background(cx).a, 0.6);
            assert_eq!(crate::gpui_shell::theme::card_content_bg(cx).a, 0.7);
            assert_eq!(cx.theme().foreground, foreground);
            assert_eq!(native_blur(cx), (BlurModeName::None, Some(BlurModeName::Acrylic)));
            set_opacity_live(1.0, cx);
            assert_eq!(native_blur(cx), (BlurModeName::Acrylic, None));
            cx.global_mut::<VisualEffects>().tab_bar_material = Default::default();
            set_opacity_live(0.8, cx);
            assert_eq!(tab_bar_surface_opacity(cx), 0.8);
            assert_eq!(card_surface_opacity(cx), 0.8);
            assert_eq!(native_blur(cx), (BlurModeName::None, None));
        });
        cx.run_until_parked();
    }
}
