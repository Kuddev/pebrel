
#[cfg(all(test, windows))]
#[gpui::test]
fn verify_installed_niu_directory_and_default_selection(cx: &mut gpui::TestAppContext) {
    let directory = std::path::PathBuf::from(std::env::var_os("NIU_PROOF_INSTALL").unwrap());
    assert!(directory.join("niu.exe").is_file());
    assert!(shell_picker::import_terminal_directory_blocking(&directory).is_ok());
    let (items, _) = shell_picker::shell_select_items("cmd", 1.0, crate::display::UiLanguage::EnUs);
    let selected = items.iter().find(|s| s.name.as_ref() == "Niu" && s.id.starts_with("profile:niu|")).expect("imported shell missing from default selector").id.clone();
    cx.update(|cx| {
        gpui_component::init(cx);
        cx.set_global(crate::gpui_shell::config::Settings::load(ThemeName::Paper));
    });
    let mut handle = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let pane = cx.new(|cx| SettingsPane::new(window, cx));
        handle = Some(pane.clone());
        gpui_component::Root::new(pane, window, cx)
    });
    let pane = handle.unwrap();
    visual.update(|_, cx| {
        let select = pane.read(cx).shell_select.clone();
        select.update(cx, |_, cx| cx.emit(SelectEvent::<Vec<ShellSelectItem>>::Confirm(Some(selected.clone()))));
    });
    visual.run_until_parked();
    assert_eq!(pebrel_settings::RuntimeSettings::load().shell.as_deref(),Some(selected.as_str()));
}
