pub(crate) fn exercise_fullscreen(cx: &mut gpui::App) {
    use gpui::AppContext as _;
    use std::time::Duration;
    let Some(target) = std::env::var_os("PEBREL_FULLSCREEN_QA_DIR") else { return };
    let target = std::path::PathBuf::from(target);
    cx.spawn(async move |cx| {
        let pause = Duration::from_millis(250);
        let mut handle = None;
        for _ in 0..80 {
            handle = cx.update(|cx| cx.windows().first().copied());
            if handle.is_some() {
                break;
            }
            cx.background_executor().timer(pause).await;
        }
        let handle = handle.expect("expected a regular Pebrel window");
        for (step, fullscreen) in [false, true, false, true, false].into_iter().enumerate() {
            if step != 0 {
                handle.update(cx, |_, window, _| window.toggle_fullscreen()).unwrap();
            }
            for _ in 0..60 {
                let landed = handle.update(cx, |_, window, _| window.is_fullscreen()).unwrap();
                if landed == fullscreen {
                    break;
                }
                cx.background_executor().timer(pause).await;
            }
            cx.background_executor().timer(Duration::from_secs(2)).await;
            let (actual, toolbar, transparent, number) = handle
                .update(cx, |_, window, _| {
                    let native = native_window(window).expect("expected an AppKit NSWindow");
                    (
                        window.is_fullscreen(),
                        native.toolbar().unwrap().isVisible(),
                        native.titlebarAppearsTransparent(),
                        native.windowNumber(),
                    )
                })
                .unwrap();
            let sample = serde_json::json!({"step":step,"expected_fullscreen":fullscreen,
                "fullscreen":actual,"toolbar_visible":toolbar,"titlebar_transparent":transparent});
            std::fs::write(target.join(format!("step-{step}.json")), sample.to_string()).unwrap();
            let _ = std::process::Command::new("/usr/sbin/screencapture")
                .args(["-x", "-l", &number.to_string()])
                .arg(target.join(format!("step-{step}.png")))
                .status();
        }
        cx.update(|cx| cx.quit());
    })
    .detach();
}
