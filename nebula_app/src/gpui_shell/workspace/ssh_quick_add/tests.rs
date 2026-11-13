use super::*;

/// `add_window_view` 需要一个 Render 根视图；浮层状态在闭包里创建，
/// 这里只当窗口宿主。
struct TestHost;
impl gpui::Render for TestHost {
    fn render(&mut self, _window: &mut Window, _cx: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div()
    }
}

/// 临时主机库目录：唯一命名避免测试并行互踩，用完整目录删除。
fn temp_profile_dir(tag: &str) -> std::path::PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "pebrel-quick-add-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    directory
}

/// 内存凭据库：按目的地存密码。「写入」这一步可挂一个外来动作，交错
/// 测试用它把别的窗口的保存插进事务中间。测试永不触碰用户钥匙串。
struct FakeCredentials {
    secrets: std::collections::BTreeMap<String, Vec<u8>>,
    on_store: Option<Box<dyn FnMut(&str)>>,
}

impl FakeCredentials {
    fn new() -> Self {
        Self { secrets: Default::default(), on_store: None }
    }

    fn call(&mut self, op: CredentialOp<'_>) -> std::io::Result<Option<Vec<u8>>> {
        match op {
            CredentialOp::Prior(destination) => Ok(self.secrets.get(destination).cloned()),
            CredentialOp::Store(destination, secret) => {
                if let Some(on_store) = self.on_store.as_mut() {
                    on_store(destination);
                }
                self.secrets.insert(destination.to_owned(), secret.to_vec());
                Ok(None)
            },
            CredentialOp::Restore(destination, secret) => {
                self.secrets.insert(destination.to_owned(), secret.to_vec());
                Ok(None)
            },
            CredentialOp::Forget(destination) => {
                self.secrets.remove(destination);
                Ok(None)
            },
        }
    }
}

/// 窗口 B 的保存数据：同一目的地、默认认证、名称可区分。两个测试都要
/// 一份能和 A 的表单区分开的 profile。
fn window_b_profile() -> crate::ssh_profiles::SshProfileAuth {
    crate::ssh_profiles::SshProfileAuth {
        destination: "root@core.example.cn".to_owned(),
        auth: crate::ssh_profiles::SshAuthMode::Auto,
        private_keys: Vec::new(),
        label: Some("Window B".to_owned()),
        icon: None,
        connection: crate::ssh_profiles::SshConnectionOptions::default(),
    }
}

/// 模拟一个不走保存 API（不取租约）的写入方：把窗口 B 对同一目的地的
/// 保存直接写进主机库文件。A 事务的提交会在快照比对处失败。
fn out_of_band_window_b_save(path: &std::path::Path) {
    let mut window_b = crate::ssh_profiles::SshProfiles::load(path).unwrap();
    window_b.upsert(window_b_profile());
    let data = serde_json::to_vec_pretty(&window_b).unwrap();
    crate::atomic_file::write(path, &data).unwrap();
}

#[gpui::test]
fn quick_add_collision_reuses_the_saved_profile_untouched(cx: &mut gpui::TestAppContext) {
    let (mut state, cx) = init_and_build("other.example", cx);
    let directory = temp_profile_dir("collision");
    let path = directory.join("ssh_profiles.json");
    // 库里先有一台带完整配置的主机：私钥、跳板、名称、图标都是浮层
    // 这份子集表单覆盖不了的权威数据。
    let mut profiles = crate::ssh_profiles::SshProfiles::default();
    profiles.upsert(crate::ssh_profiles::SshProfileAuth {
        destination: "deploy@box.example".to_owned(),
        auth: crate::ssh_profiles::SshAuthMode::Password,
        private_keys: vec![std::path::PathBuf::from("~/.id_ed25519")],
        label: Some("Deploy box".to_owned()),
        icon: Some("debian".to_owned()),
        connection: {
            let mut connection = crate::ssh_profiles::SshConnectionOptions::default();
            connection.jump_mode = crate::ssh_profiles::SshHostJumpMode::Host;
            connection.jump_host = "bastion.example".to_owned();
            connection
        },
    });
    profiles.save(&path).unwrap();
    let before = std::fs::read(&path).unwrap();
    // 表单地址改成已有主机的目的地，还填了密码：保存必须原样复用，
    // 连凭据都不能替编辑器写。
    cx.update(|window, cx| {
        state.address.update(cx, |input, cx| input.set_value("deploy@box.example", window, cx));
        state.auth = crate::ssh_profiles::SshAuthMode::Password;
        state.password.update(cx, |input, cx| input.set_value("typed-for-other-host", window, cx));
    });
    let mut credential_writes = Vec::new();
    cx.update(|_, cx| {
        let destination = persist_quick_add_with(
            &state,
            &path,
            |op| {
                if let CredentialOp::Store(destination, secret) = op {
                    credential_writes.push((destination.to_owned(), secret.to_vec()));
                }
                Ok(None)
            },
            cx,
        )
        .unwrap();
        assert_eq!(destination, "deploy@box.example");
    });
    assert!(credential_writes.is_empty(), "collision must not touch credentials behind the editor");
    assert_eq!(
        std::fs::read(&path).unwrap(),
        before,
        "collision must not rewrite the saved profile"
    );
    let profile = crate::ssh_profiles::SshProfiles::load(&path)
        .unwrap()
        .for_destination("deploy@box.example");
    assert_eq!(profile.private_keys, vec![std::path::PathBuf::from("~/.id_ed25519")]);
    assert_eq!(profile.label.as_deref(), Some("Deploy box"));
    assert_eq!(profile.icon.as_deref(), Some("debian"));
    assert_eq!(profile.connection.jump_host, "bastion.example");
    let _ = std::fs::remove_dir_all(&directory);
}

#[gpui::test]
fn quick_add_password_failure_leaves_the_host_library_untouched(cx: &mut gpui::TestAppContext) {
    let (mut state, cx) = init_and_build("core.example.cn", cx);
    let directory = temp_profile_dir("password-failure");
    let path = directory.join("ssh_profiles.json");
    cx.update(|window, cx| {
        state.auth = crate::ssh_profiles::SshAuthMode::Password;
        state.password.update(cx, |input, cx| input.set_value("secret", window, cx));
    });
    cx.update(|_, cx| {
        let error = persist_quick_add_with(
            &state,
            &path,
            |op| match op {
                CredentialOp::Store(_, _) => Err(std::io::Error::other("keychain unavailable")),
                _ => Ok(None),
            },
            cx,
        )
        .unwrap_err();
        assert!(matches!(error, QuickAddError::Io(_)));
    });
    // 密码先于主机库提交：凭据失败时表单报错，文件必须还没被写出来。
    assert!(!path.exists(), "credential failure must not write the profile file");
    let _ = std::fs::remove_dir_all(&directory);
}

#[gpui::test]
fn quick_add_transaction_rejects_a_window_save_in_the_middle(cx: &mut gpui::TestAppContext) {
    let (mut state, cx) = init_and_build("core.example.cn", cx);
    let directory = temp_profile_dir("transaction");
    let path = directory.join("ssh_profiles.json");
    cx.update(|window, cx| {
        state.auth = crate::ssh_profiles::SshAuthMode::Password;
        state.password.update(cx, |input, cx| input.set_value("from-window-a", window, cx));
    });
    // 复现审查固定的交错顺序：A 读取主机库、B 保存同一目标及其密码、
    // A 写密码、A 提交。A 的事务全程持有租约，B 的保存API 落在 A 的
    // 凭据写入这一步，必须被租约拒绝，而不是带着 A 的旧快照写入。
    let mut credentials = FakeCredentials::new();
    let closure_path = path.clone();
    credentials.on_store = Some(Box::new(move |destination| {
        assert_eq!(destination, "root@core.example.cn");
        let mut window_b = crate::ssh_profiles::SshProfiles::load(&closure_path).unwrap();
        window_b.upsert(window_b_profile());
        let error = window_b.save(&closure_path).unwrap_err();
        assert_eq!(
            error.kind(),
            std::io::ErrorKind::WouldBlock,
            "the window save must be rejected by A's lease"
        );
    }));
    cx.update(|_, cx| {
        let destination =
            persist_quick_add_with(&state, &path, |op| credentials.call(op), cx).unwrap();
        assert_eq!(destination, "root@core.example.cn");
    });
    // 终态只有 A 的 profile 和 A 的密码；B 的保存一个字节都没落下。
    let profiles = crate::ssh_profiles::SshProfiles::load(&path).unwrap();
    let profile = profiles.for_destination("root@core.example.cn");
    assert_eq!(profile.auth, crate::ssh_profiles::SshAuthMode::Password);
    assert_eq!(profile.label, None);
    assert_eq!(
        credentials.secrets.get("root@core.example.cn").map(Vec::as_slice),
        Some(b"from-window-a".as_slice())
    );
    let _ = std::fs::remove_dir_all(&directory);
}

#[gpui::test]
fn quick_add_commit_failure_rolls_the_password_back(cx: &mut gpui::TestAppContext) {
    let (mut state, cx) = init_and_build("core.example.cn", cx);
    cx.update(|window, cx| {
        state.auth = crate::ssh_profiles::SshAuthMode::Password;
        state.password.update(cx, |input, cx| input.set_value("typed-secret", window, cx));
    });

    // 场景一：事务前该目的地没有密码。提交前一刻，一个绕过保存 API 的
    // 写入方把 B 的同目标保存落了盘；提交必须失败，且本次写入的密码被
    // 删掉——库和凭据都停在 B 的状态上，谁也不覆盖谁。
    {
        let directory = temp_profile_dir("rollback-forget");
        let path = directory.join("ssh_profiles.json");
        let mut credentials = FakeCredentials::new();
        let closure_path = path.clone();
        credentials.on_store = Some(Box::new(move |_| out_of_band_window_b_save(&closure_path)));
        cx.update(|_, cx| {
            let error =
                persist_quick_add_with(&state, &path, |op| credentials.call(op), cx).unwrap_err();
            assert!(
                matches!(&error, QuickAddError::Io(message) if message.contains("Host profiles changed")),
                "the commit itself must fail on the changed file: {error:?}"
            );
        });
        assert!(
            credentials.secrets.is_empty(),
            "the transaction's own password write must be rolled back"
        );
        let profiles = crate::ssh_profiles::SshProfiles::load(&path).unwrap();
        let profile = profiles.for_destination("root@core.example.cn");
        assert_eq!(
            profile.label.as_deref(),
            Some("Window B"),
            "the out-of-band save must survive the failed commit"
        );
        let _ = std::fs::remove_dir_all(&directory);
    }

    // 场景二：事务前已有旧密码（比如早先删过的主机留下的孤儿凭据）。
    // 同样的提交失败这次要恢复旧值，而不是删掉。
    {
        let directory = temp_profile_dir("rollback-restore");
        let path = directory.join("ssh_profiles.json");
        let mut credentials = FakeCredentials::new();
        credentials.secrets.insert("root@core.example.cn".to_owned(), b"old-secret".to_vec());
        let closure_path = path.clone();
        credentials.on_store = Some(Box::new(move |_| out_of_band_window_b_save(&closure_path)));
        cx.update(|_, cx| {
            let error =
                persist_quick_add_with(&state, &path, |op| credentials.call(op), cx).unwrap_err();
            assert!(matches!(error, QuickAddError::Io(_)));
        });
        assert_eq!(
            credentials.secrets.get("root@core.example.cn").map(Vec::as_slice),
            Some(b"old-secret".as_slice()),
            "commit failure must restore the prior password, not the typed one"
        );
        let _ = std::fs::remove_dir_all(&directory);
    }
}

/// 提交失败但本次没写过密码：自动认证与密码留空的保存不得碰凭据——
/// 目的地原有的密码不是这次写的，回滚删除就是替用户清库。提交失败用
/// 维护者的注入方式复现：预置锁文件后目录只读，租约照常取得，落盘的
/// 临时文件建不出来。目录权限在 Windows 上不拦截建文件，这条只在
/// Unix 上跑。
#[cfg(unix)]
#[gpui::test]
fn quick_add_commit_failure_without_a_password_write_keeps_the_existing_credential(
    cx: &mut gpui::TestAppContext,
) {
    use std::os::unix::fs::PermissionsExt as _;
    let (mut state, cx) = init_and_build("core.example.cn", cx);
    let directory = temp_profile_dir("rollback-no-write");
    let path = directory.join("ssh_profiles.json");
    // 库里先有另一台主机，目的地原有的密码只存在凭据库里。
    let mut seeded = crate::ssh_profiles::SshProfiles::default();
    seeded.upsert(crate::ssh_profiles::SshProfileAuth {
        destination: "other@box.example".to_owned(),
        auth: crate::ssh_profiles::SshAuthMode::Password,
        private_keys: Vec::new(),
        label: Some("Other box".to_owned()),
        icon: None,
        connection: crate::ssh_profiles::SshConnectionOptions::default(),
    });
    seeded.save(&path).unwrap();
    // 预置锁文件：目录只读后，已有锁文件仍能打开加锁，落盘才会败在
    // 建临时文件这一步——正好是提交失败，而不是取租约失败。
    std::fs::write(path.with_extension("json.nebula-lock"), b"0").unwrap();
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o555)).unwrap();

    // 「本次没写密码」的两种保存：自动认证、密码认证但密码留空。
    for (name, auth) in [
        ("auto", crate::ssh_profiles::SshAuthMode::Auto),
        ("empty-password", crate::ssh_profiles::SshAuthMode::Password),
    ] {
        cx.update(|window, cx| {
            state.auth = auth;
            state.password.update(cx, |input, cx| input.set_value("", window, cx));
        });
        let mut credentials = FakeCredentials::new();
        credentials.secrets.insert("root@core.example.cn".to_owned(), b"pre-existing".to_vec());
        let mut ops = Vec::new();
        cx.update(|_, cx| {
            let error = persist_quick_add_with(
                &state,
                &path,
                |op| {
                    ops.push(match op {
                        CredentialOp::Prior(_) => "prior",
                        CredentialOp::Store(..) => "store",
                        CredentialOp::Restore(..) => "restore",
                        CredentialOp::Forget(_) => "forget",
                    });
                    credentials.call(op)
                },
                cx,
            )
            .unwrap_err();
            assert!(matches!(error, QuickAddError::Io(_)), "{name}: expected the commit error");
        });
        assert!(
            ops.is_empty(),
            "{name}: a save that writes no password must not touch credentials: {ops:?}"
        );
        assert_eq!(
            credentials.secrets.get("root@core.example.cn").map(Vec::as_slice),
            Some(b"pre-existing".as_slice()),
            "{name}: the existing credential must survive a failed commit it did not write"
        );
    }
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755)).unwrap();
    let _ = std::fs::remove_dir_all(&directory);
}

fn init_and_build<'a>(
    destination: &str,
    cx: &'a mut gpui::TestAppContext,
) -> (SshQuickAddState, &'a mut gpui::VisualTestContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        cx.set_global(crate::gpui_shell::config::Settings::load(nebula_settings::ThemeName::Nord));
    });
    let mut state = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        state = Some(SshQuickAddState::new(destination, window, cx));
        TestHost
    });
    (state.expect("state built"), cx)
}

#[test]
fn quick_add_prefills_user_and_port_from_the_typed_address() {
    // 不依赖窗口：预填拆分只走 `split_destination_*`。
    let (address_with_user, port) =
        crate::display::split_destination_port("deploy@box.example:2222");
    let (username, address) = crate::display::split_destination_user(&address_with_user);
    assert_eq!(
        (username.as_str(), address.as_str(), port.as_str()),
        ("deploy", "box.example", "2222")
    );
}

#[gpui::test]
fn quick_add_draft_composes_and_validates(cx: &mut gpui::TestAppContext) {
    let (state, cx) = init_and_build("core.example.cn", cx);
    cx.update(|window, cx| {
        assert_eq!(state.draft_destination(cx).unwrap(), "root@core.example.cn");
        // 端口非法直接拒绝，不进入落盘。
        state.port.update(cx, |input, cx| input.set_value("70000", window, cx));
        assert_eq!(state.draft_destination(cx).unwrap_err(), QuickAddError::InvalidPort);
        state.port.update(cx, |input, cx| input.set_value("2222", window, cx));
        assert_eq!(state.draft_destination(cx).unwrap(), "root@core.example.cn:2222");
        // 带非法字符交给权威校验拒绝；空地址单独报"缺地址"。
        state.address.update(cx, |input, cx| input.set_value("bad host", window, cx));
        assert!(matches!(state.draft_destination(cx), Err(QuickAddError::Rejected(_))));
        state.address.update(cx, |input, cx| input.set_value("", window, cx));
        assert_eq!(state.draft_destination(cx).unwrap_err(), QuickAddError::MissingAddress);
    });
}

#[gpui::test]
fn quick_add_persists_a_nameless_profile_before_any_connection(cx: &mut gpui::TestAppContext) {
    let (mut state, cx) = init_and_build("core.example.cn", cx);
    let directory = std::env::temp_dir().join(format!(
        "pebrel-quick-add-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("ssh_profiles.json");
    cx.update(|_, cx| {
        let destination = persist_quick_add(&state, &path, cx).unwrap();
        assert_eq!(destination, "root@core.example.cn");
    });
    let profiles = crate::ssh_profiles::SshProfiles::load(&path).unwrap();
    let profile = profiles.for_destination("root@core.example.cn");
    // 没填名称：保持空，列表回落显示地址，而不是自动起「主机 N」。
    assert_eq!(profile.label, None);
    assert_eq!(profile.auth, crate::ssh_profiles::SshAuthMode::Auto);
    // 公钥模式：浮层收集不了私钥文件，落盘必须被守卫拦下，
    // 且不能把空私钥的坏 profile 写进文件。
    cx.update(|_, cx| {
        state.auth = crate::ssh_profiles::SshAuthMode::PublicKey;
        assert_eq!(
            persist_quick_add(&state, &path, cx).unwrap_err(),
            QuickAddError::KeysNeedAdvancedEditor
        );
    });
    assert_eq!(
        crate::ssh_profiles::SshProfiles::load(&path)
            .unwrap()
            .for_destination("root@core.example.cn")
            .auth,
        crate::ssh_profiles::SshAuthMode::Auto,
        "rejected save must not overwrite the stored profile"
    );
    let _ = std::fs::remove_dir_all(&directory);
}
