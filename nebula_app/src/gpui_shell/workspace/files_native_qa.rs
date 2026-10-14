//! Opt-in native file browser acceptance, built only with test cfg. The regular
//! platform event loop stays on the main thread; all panes and endpoints are owned.
use super::*;
use crate::ssh_session::sftp_test_support::Fixture;
use gpui::{QuitMode, WindowBounds, WindowOptions};
use std::path::PathBuf;

pub(crate) fn run_if_requested() -> Result<bool, Box<dyn std::error::Error>> {
    let Some(root) = std::env::var_os("PEBREL_FILES_NATIVE_QA") else { return Ok(false) };
    let root = PathBuf::from(root);
    if !root.is_absolute() || !root.is_dir() {
        return Err(
            "PEBREL_FILES_NATIVE_QA must name an existing absolute artifact directory".into()
        );
    }
    let directory = tempfile::Builder::new().prefix("owned-files-").tempdir_in(root)?;
    let path = directory.path().to_owned();
    let language = if std::env::var("PEBREL_FILES_QA_LANGUAGE").as_deref() == Ok("zh-CN") {
        "zh-CN"
    } else {
        "en-US"
    };
    files_fixture::prepare(&path, language)?;
    // This is the first main-thread branch, before workers/settings exist.
    // Never repurpose HOME: test SSH enumeration has its own narrow override.
    unsafe {
        for name in files_fixture::SIDE_EFFECT_OVERRIDES {
            std::env::remove_var(name);
        }
        std::env::set_var("PEBREL_CONFIG_DIR", &path);
        std::env::set_var("PEBREL_TEST_SSH_HOME", &path);
    }
    files_fixture::assert_isolated(&path);
    let a = path.join("local-a");
    let b = path.join("local-b");
    for directory in [&a, &b] {
        std::fs::create_dir(directory)?;
    }
    std::fs::create_dir(b.join("nested"))?;
    std::fs::write(b.join("nested/中文 space.txt"), "完整内容\0\nlocal B")?;
    std::fs::write(a.join("local A only.txt"), "local A")?;
    for index in 0..50 {
        std::fs::write(a.join(format!("scroll-{index:02}.txt")), format!("row {index}"))?;
    }
    let runtime = crate::ssh_session::runtime()?;
    let selected = runtime.block_on(Fixture::new());
    let other = runtime.block_on(Fixture::new());
    let destinations = [selected.destination.clone(), other.destination.clone()];
    let stores = [selected.store.clone(), other.store.clone()];
    let evidence = path.join("remote-bytes.json");
    let evidence_task = runtime.spawn(async move {
        loop {
            let values: Vec<_> = stores.iter().map(|store| {
                store.lock().unwrap().files.iter().map(|(path, node)| {
                    (path.clone(), serde_json::json!({"bytes": node.data, "permissions": node.permissions}))
                }).collect::<std::collections::BTreeMap<_, _>>()
            }).collect();
            let _ = tokio::fs::write(&evidence, serde_json::to_vec_pretty(&values).unwrap()).await;
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    });
    crate::gpui_shell::try_write_stderr(format_args!(
        "Owned native file fixtures: {}",
        path.display()
    ));
    gpui_platform::application()
        .with_assets(crate::gpui_shell::assets::NebulaAssets)
        .with_quit_mode(QuitMode::LastWindowClosed)
        .run(move |cx| {
            crate::gpui_shell::init(cx, None);
            windowing::initialize(cx, crate::runtime_api::RuntimeHub::new());
            cx.activate(true);
            let bounds = Bounds::centered(None, size(px(1280.0), px(800.0)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(400.0), px(400.0))),
                    titlebar: Some(TitleBar::title_bar_options()),
                    ..Default::default()
                },
                move |window, cx| {
                    window.set_window_title("Pebrel owned file browser acceptance");
                    let workspace = cx.new(|cx| {
                        let mut workspace = NebulaWorkspace::new(
                            window,
                            None,
                            None,
                            1,
                            crate::runtime_api::RuntimeHub::new(),
                            windowing::WorkspaceStartup::Empty,
                            windowing::WindowRole::Regular,
                            cx,
                        );
                        workspace.tabs_position = nebula_settings::TabsPositionName::Top;
                        workspace.sidebar_collapsed = true;
                        files_fixture::append_pane(
                            &mut workspace,
                            &a,
                            None,
                            false,
                            None,
                            window,
                            cx,
                        );
                        files_fixture::append_pane(
                            &mut workspace,
                            &b,
                            None,
                            false,
                            None,
                            window,
                            cx,
                        );
                        for destination in &destinations {
                            files_fixture::append_pane(
                                &mut workspace,
                                &a,
                                Some(destination),
                                true,
                                None,
                                window,
                                cx,
                            );
                        }
                        workspace
                    });
                    cx.new(|cx| Root::new(workspace, window, cx))
                },
            )
            .expect("owned native QA window");
        });
    evidence_task.abort();
    drop((selected, other));
    let _ = directory.keep();
    Ok(true)
}
