//! Manual desktop acceptance; run from a fresh QA directory containing
//! demo-project/conflict.rs in a real Git conflict, with isolated settings.

use super::*;

#[test]
#[ignore = "requires a desktop, PEBREL_I18N_QA_DIR, isolated PEBREL_CONFIG_DIR and UI verification"]
fn native_merge_localization() {
    let output = PathBuf::from(std::env::var_os("PEBREL_I18N_QA_DIR").unwrap());
    assert_eq!(std::env::current_dir().unwrap(), output);
    assert_eq!(
        std::env::var_os("PEBREL_CONFIG_DIR").map(PathBuf::from),
        Some(output.join("config")),
    );
    assert!(!output.join("ready").exists(), "use a fresh QA directory");
    let checked = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let after_run = checked.clone();
    gpui_platform::application().with_assets(crate::gpui_shell::assets::NebulaAssets).run(
        move |cx| {
            gpui_component::init(cx);
            cx.set_global(crate::gpui_shell::config::Settings::load(
                nebula_settings::ThemeName::Nord,
            ));
            crate::gpui_shell::theme::apply_chrome_theme(cx);
            crate::gpui_shell::math_view::register(cx);
            crate::gpui_shell::file_editor::init(cx);
            let handle = cx
                .open_window(
                    gpui::WindowOptions {
                        window_bounds: Some(gpui::WindowBounds::Windowed(gpui::Bounds::new(
                            gpui::point(px(40.0), px(40.0)),
                            gpui::size(px(1050.0), px(640.0)),
                        ))),
                        focus: false,
                        show: true,
                        ..Default::default()
                    },
                    |window, cx| {
                        let view = cx.new(|cx| {
                            CodeTabView::new_git_merge(
                                GitLocation::Local { root: PathBuf::from("demo-project") },
                                "conflict.rs".into(),
                                window,
                                cx,
                            )
                        });
                        cx.new(|cx| gpui_component::Root::new(view, window, cx))
                    },
                )
                .unwrap();
            cx.spawn(async move |cx| {
                cx.background_executor().timer(std::time::Duration::from_millis(500)).await;
                cx.update_window(handle.into(), |_, window, cx| {
                    window.refresh();
                    window.draw(cx).clear(cx);
                })
                .unwrap();
                std::fs::write(output.join("ready"), std::process::id().to_string()).unwrap();
                for _ in 0..300 {
                    if output.join("capture-complete").exists() {
                        break;
                    }
                    cx.background_executor().timer(std::time::Duration::from_millis(200)).await;
                }
                checked
                    .store(output.join("verified").exists(), std::sync::atomic::Ordering::SeqCst);
                let _ = cx.update_window(handle.into(), |_, window, _| window.remove_window());
                cx.update(|cx| cx.quit());
            })
            .detach();
        },
    );
    assert!(after_run.load(std::sync::atomic::Ordering::SeqCst), "native UI was not verified");
}
