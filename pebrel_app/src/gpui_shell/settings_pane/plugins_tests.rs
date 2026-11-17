//! 插件页：授权确认、安装、生成主题、停用与卸载，都经由渲染出的真实控件点击。

use super::plugins::{Pending, PluginsState};
use super::*;
use crate::plugins::Grant;
use crate::plugins::install::Request;
use crate::theme_library::ThemeLibraryStore;
use gpui::{Modifiers, TestAppContext, VisualTestContext, size};

const MANIFEST: &str = r#"
manifest_version = 1
api_version = 1
id = "example.ocean"
name = "Ocean"
version = "1.0.0"
entry = "init.luau"
[[commands]]
id = "generate"
title = "Generate Ocean"
handler = "generate"
output = "theme"
"#;

const SOURCE: &str = r##"
return {
    generate = function()
        return { base = "Nord", name = "Ocean fixture", terminal = { background = "#001122" } }
    end,
}
"##;

struct Fixture {
    package: tempfile::TempDir,
    config: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let package = tempfile::tempdir().unwrap();
        std::fs::write(package.path().join("plugin.toml"), MANIFEST).unwrap();
        std::fs::write(package.path().join("init.luau"), SOURCE).unwrap();
        Self { package, config: tempfile::tempdir().unwrap() }
    }

    fn store(&self) -> ThemeLibraryStore {
        ThemeLibraryStore::new(self.config.path().join("themes"))
    }

    fn pending(&self) -> Pending {
        Pending {
            path: self.package.path().to_owned(),
            request: Request { name: "Ocean".into(), requested: vec![Grant::ThemeLibrary] },
        }
    }
}

fn open<'a>(
    cx: &'a mut TestAppContext,
    fixture: &Fixture,
) -> (Entity<SettingsPane>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        cx.set_global(crate::gpui_shell::config::Settings::load(ThemeName::Nord));
    });
    let roots = (fixture.config.path().join("plugins"), fixture.config.path().join("themes"));
    let mut pane = None;
    let (_, window) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| SettingsPane::new(window, cx));
        view.update(cx, |pane, _| {
            pane.active_section = PLUGINS_SECTION;
            pane.plugins = PluginsState::with_roots(roots.0, roots.1);
        });
        pane = Some(view.clone());
        gpui_component::Root::new(view, window, cx)
    });
    window.simulate_resize(size(px(1040.0), px(900.0)));
    window.run_until_parked();
    (pane.unwrap(), window)
}

fn click(selector: &'static str, window: &mut VisualTestContext) {
    redraw(window);
    let bounds = window.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} rendered"));
    window.simulate_click(bounds.center(), Modifiers::default());
    window.run_until_parked();
}

fn redraw(window: &mut VisualTestContext) {
    window.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

fn feedback(pane: &Entity<SettingsPane>, window: &mut VisualTestContext) -> Result<String, String> {
    pane.read_with(window, |pane, _| pane.plugins.feedback.clone().expect("feedback").1)
}

#[gpui::test]
fn plugins_page_is_reachable_and_starts_empty_without_touching_the_real_library(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    let (pane, window) = open(cx, &fixture);
    redraw(window);
    assert!(window.debug_bounds("plugins-empty").is_some());
    assert!(window.debug_bounds("plugins-install").is_some());
    pane.read_with(window, |pane, _| {
        let listing = pane.plugins.listing.as_ref().unwrap().as_ref().unwrap();
        assert!(listing.plugins.is_empty());
    });
}

#[gpui::test]
fn cancelling_the_grant_installs_nothing(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (pane, window) = open(cx, &fixture);
    pane.update(window, |pane, cx| {
        pane.plugins.pending = Some(fixture.pending());
        cx.notify();
    });
    click("plugins-grant-cancel", window);
    redraw(window);
    assert!(window.debug_bounds("plugins-grant").is_none());
    assert!(!fixture.config.path().join("plugins/example.ocean").exists());
}

#[gpui::test]
fn granted_install_then_generate_adds_a_theme_without_applying_it(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (pane, window) = open(cx, &fixture);
    pane.update(window, |pane, cx| {
        pane.plugins.pending = Some(fixture.pending());
        cx.notify();
    });
    redraw(window);
    assert!(window.debug_bounds("plugins-grant").is_some());
    click("plugins-grant-allow", window);
    assert!(feedback(&pane, window).unwrap().contains("Ocean"));
    redraw(window);
    assert!(window.debug_bounds("plugin-row-0").is_some());
    assert!(window.debug_bounds("plugins-install").is_some());

    let theme_before = pane.read_with(window, |pane, _| pane.runtime.custom_theme.clone());
    click("plugin-generate-0-0", window);
    assert!(feedback(&pane, window).unwrap().contains("Ocean fixture"));
    redraw(window);
    assert!(window.debug_bounds("plugin-feedback-0").is_some());
    let custom = fixture.store().list().unwrap().custom;
    assert_eq!(custom.len(), 1);
    assert_eq!(custom[0].name(), "Ocean fixture");
    assert_eq!(pane.read_with(window, |pane, _| pane.runtime.custom_theme.clone()), theme_before);
}

#[gpui::test]
fn disabling_blocks_generation_and_uninstall_keeps_generated_themes(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (pane, window) = open(cx, &fixture);
    pane.update(window, |pane, cx| {
        pane.plugins.pending = Some(fixture.pending());
        cx.notify();
    });
    click("plugins-grant-allow", window);
    click("plugin-generate-0-0", window);
    assert_eq!(fixture.store().list().unwrap().custom.len(), 1);

    click("plugin-enabled-0", window);
    pane.read_with(window, |pane, _| {
        let listing = pane.plugins.listing.as_ref().unwrap().as_ref().unwrap();
        assert!(!listing.plugins[0].enabled);
    });
    // 停用后生成按钮不可点；直接调用也会被安装库拒绝。
    click("plugin-generate-0-0", window);
    assert_eq!(fixture.store().list().unwrap().custom.len(), 1);
    pane.update(window, |pane, cx| {
        pane.generate_plugin_theme("example.ocean".into(), "generate".into(), cx)
    });
    window.run_until_parked();
    assert!(feedback(&pane, window).unwrap_err().contains("disabled"));

    click("plugin-uninstall-0", window);
    redraw(window);
    assert!(window.debug_bounds("plugins-empty").is_some());
    assert!(!fixture.config.path().join("plugins/example.ocean").exists());
    assert_eq!(fixture.store().list().unwrap().custom.len(), 1);
}
