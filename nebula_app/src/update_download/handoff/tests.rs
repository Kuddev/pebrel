use super::*;

#[test]
fn update_restore_tickets_cover_success_rollback_and_acknowledgement() {
    // Run in a separate process: changing the settings environment in the shared
    // test runner would race unrelated tests and could touch real user settings.
    if std::env::var_os("PEBREL_RESTORE_TEST_CHILD").is_none() {
        let root = tempfile::tempdir().unwrap();
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "update_download::handoff::tests::update_restore_tickets_cover_success_rollback_and_acknowledgement", "--nocapture"])
            .env("PEBREL_RESTORE_TEST_CHILD", "1")
            .env("PEBREL_CONFIG_DIR", root.path())
            .env("NEBULA_CONFIG_DIR", root.path())
            .env_remove("PEBREL_UPDATE_RESTORE")
            .output().unwrap();
        assert!(
            result.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        return;
    }
    let config = canonical(&nebula_settings::settings_dir()).unwrap();
    let update_root = super::super::state_directory("").unwrap();
    let directory = super::super::state_directory("handoffs/restore-test").unwrap();
    std::fs::create_dir_all(&directory).unwrap();
    let executable = canonical(&std::env::current_exe().unwrap()).unwrap();
    let version = env!("CARGO_PKG_VERSION").to_owned();
    let mut plan = Plan {
        schema: 1,
        elevated: crate::platform::elevation::is_elevated().unwrap(),
        asset: UpdateAsset {
            version: version.clone(),
            name: "fixture".into(),
            download_url: String::new(),
            sha256: None,
            size: None,
        },
        transaction: "restore-test".into(),
        installation: executable.parent().unwrap().to_owned(),
        executable,
        config_directory: config.clone(),
        installer: config.join("fixture"),
        sha256: String::new(),
        bytes: 1,
        version: version.clone(),
        original_version: version,
        guard_path: config.join("guard"),
        participants: Vec::new(),
    };
    let save = |name: &str, value: serde_json::Value| {
        std::fs::write(directory.join(name), serde_json::to_vec(&value).unwrap()).unwrap();
    };
    std::fs::write(
        update_root.join("last-handoff.json"),
        serde_json::to_vec(&directory.join("plan.json")).unwrap(),
    )
    .unwrap();
    save("workspace.json", serde_json::to_value(vec![Session::new(0, Vec::new())]).unwrap());
    for recovered in [false, true] {
        plan.version = if recovered { "99.0.0".into() } else { env!("CARGO_PKG_VERSION").into() };
        save("plan.json", serde_json::to_value(&plan).unwrap());
        save(
            "result.json",
            serde_json::json!({"transaction": "wrong", "success": !recovered, "recovered_original": recovered}),
        );
        assert!(restore_ticket().is_none());
        assert!(completed_update_directory().is_none());
        save(
            "result.json",
            serde_json::json!({"transaction": plan.transaction, "success": !recovered, "recovered_original": recovered}),
        );
        assert_eq!(completed_update_directory().is_some(), !recovered);
        if !recovered {
            crate::update_check::release_notes::remember(
                &plan.version,
                "# Installed release\n**Changes**",
            );
            crate::update_check::release_notes::snapshot(&plan.version, &directory).unwrap();
            let notes = crate::update_check::release_notes::pending_installed().unwrap();
            assert_eq!(notes.body, "# Installed release\n**Changes**");
            assert!(crate::update_check::release_notes::claim(&plan.version).unwrap());
            assert!(!crate::update_check::release_notes::claim(&plan.version).unwrap());
            assert!(crate::update_check::release_notes::pending_installed().is_none());
            assert_eq!(crate::update_check::release_notes::current().unwrap().body, notes.body);
        } else {
            assert!(crate::update_check::release_notes::pending_installed().is_none());
        }
        plan.elevated = !plan.elevated;
        save("plan.json", serde_json::to_value(&plan).unwrap());
        assert!(restore_ticket().is_none(), "a ticket cannot cross the actual token scope");
        plan.elevated = !plan.elevated;
        save("plan.json", serde_json::to_value(&plan).unwrap());
        // 恢复入口返回规范路径；macOS 临时目录的 /var 与 /private/var 指向同一对象。
        assert_eq!(
            pending_restore_path().unwrap(),
            canonical(&directory.join("plan.json")).unwrap()
        );
        assert!(
            !directory.join("restore-attempts.json").exists()
                || read_json::<u32>(&directory.join("restore-attempts.json")) == Some(0)
        );
        assert_eq!(restore_ticket().unwrap().len(), 1);
        assert_eq!(
            restore_ticket_from(&directory.join("plan.json")).unwrap().len(),
            1,
            "the UI can retry missing windows from the same ticket"
        );
        acknowledge_restore(2);
        assert!(!directory.join("restored.json").exists());
        acknowledge_restore(1);
        assert!(directory.join("restored.json").exists());
        assert!(restore_ticket().is_none(), "a completed restore must not replay");
        std::fs::remove_file(directory.join("restored.json")).unwrap();
        save("restore-attempts.json", serde_json::json!(3));
        assert!(restore_ticket().is_none(), "crash-loop recovery is bounded");
        save("restore-attempts.json", serde_json::json!(0));
    }
    plan.original_version = "0.0.0".into();
    save("plan.json", serde_json::to_value(&plan).unwrap());
    assert!(restore_ticket().is_none(), "rollback must match the running original version");
    plan.original_version = env!("CARGO_PKG_VERSION").into();
    save("plan.json", serde_json::to_value(&plan).unwrap());
    assert_eq!(restore_ticket_from(&directory.join("plan.json")).unwrap().len(), 1);
    assert!(!acknowledge_restore(3), "unrelated windows do not acknowledge the ticket");
    assert!(acknowledge_restore(1));
    if crate::platform::Platform::current() == crate::platform::Platform::Windows {
        scheduled_snapshot_scope();
    }
    if plan.elevated {
        // 私有存储在配置临时目录之外；只删除此夹具创建的已知文件，保留任何意外内容。
        for name in
            ["plan.json", "workspace.json", "result.json", "restore-attempts.json", "restored.json"]
        {
            std::fs::remove_file(directory.join(name)).unwrap();
        }
        for name in ["last-handoff.json", "install-next.json"] {
            std::fs::remove_file(update_root.join(name)).unwrap();
        }
        std::fs::remove_dir(&directory).unwrap();
        std::fs::remove_dir(update_root.join("handoffs")).unwrap();
        std::fs::remove_dir(update_root).unwrap();
    }
}

fn scheduled_snapshot_scope() {
    let elevated = crate::platform::elevation::is_elevated().unwrap();
    let ordinary = Session::new(
        0,
        vec![crate::session::TabSession::single("ordinary-only".into(), None, None)],
    );
    crate::session::try_save(&ordinary).unwrap();
    let update_root = super::super::state_directory("").unwrap();
    let version = env!("CARGO_PKG_VERSION");
    let name = crate::update_check::assets::native_names(version)[0].clone();
    let asset = UpdateAsset {
        version: version.into(),
        name: name.clone(),
        download_url: format!(
            "https://github.com/Kuddev/pebrel/releases/download/v{version}/{name}"
        ),
        size: Some(2),
        sha256: Some("a".repeat(64)),
    };
    let _job = super::super::begin_download_session(&asset).unwrap();
    super::super::session().as_mut().unwrap().status =
        super::super::DownloadStatus::Ready { path: update_root.join(name), bytes: 2 };
    let before = Session::new(
        0,
        vec![crate::session::TabSession::single("administrator-before".into(), None, None)],
    );
    let after = Session::new(
        0,
        vec![crate::session::TabSession::single("administrator-after".into(), None, None)],
    );
    schedule(&asset, &[before.clone()]).unwrap();
    assert_eq!(checkpoint_scheduled(&after).unwrap(), elevated);
    let actual = scheduled_windows().unwrap();
    assert_eq!(actual[0].tabs, if elevated { after.tabs } else { ordinary.tabs.clone() });
    assert_eq!(
        crate::session::load_update_windows().unwrap()[0].tabs,
        ordinary.tabs,
        "administrator scheduling and subsequent saves never overwrite the ordinary workspace"
    );
    if elevated {
        std::fs::remove_file(update_root.join("scheduled-workspace.json")).unwrap();
        assert!(
            scheduled_windows().is_err(),
            "a missing administrator snapshot cannot fall back to the ordinary session"
        );
    }
    SCHEDULED_IN_THIS_PROCESS.store(false, Ordering::Release);
    *super::super::session() = None;
}
