use super::config::resolve_from_ssh_config_text;
use super::{
    AuthMethod, SshDestination, authentication_plan, initial_remote_cd_command, is_password_prompt,
    parse_resolved_config, resolve_network_proxy, ssh_config_probe_target,
};
use crate::ssh_profiles::SshAuthMode;
use crate::ssh_proxy::{ProxyLink, ProxyMode, SshProxyConfig};
use rsa::pkcs1::{EncodeRsaPrivateKey, LineEnding as RsaLineEnding};
use std::path::PathBuf;
use std::sync::LazyLock;
use zeroize::Zeroizing;

#[test]
fn parses_saved_destinations() {
    let plain = SshDestination::parse("root@example.com").unwrap();
    assert_eq!((plain.user.as_str(), plain.host.as_str(), plain.port), ("root", "example.com", 22));

    let uri = SshDestination::parse("ssh://alice@example.com:2200").unwrap();
    assert_eq!((uri.user.as_str(), uri.host.as_str(), uri.port), ("alice", "example.com", 2200));

    let ipv6 = SshDestination::parse("ssh://root@[2001:db8::1]:2222").unwrap();
    assert_eq!((ipv6.host.as_str(), ipv6.port), ("2001:db8::1", 2222));
}

#[test]
fn parses_resolved_ssh_config() {
    let config =
        "user deploy\nhostname server.internal\nport 2200\nidentityfile ~/.ssh/id_ed25519\n";
    let destination = parse_resolved_config("prod", config).unwrap();
    assert_eq!(destination.user, "deploy");
    assert_eq!(destination.host, "server.internal");
    assert_eq!(destination.port, 2200);
    assert_eq!(destination.identity_files.len(), 1);
}

#[cfg(windows)]
#[test]
fn system_openssh_expands_alias_and_windows_identity_from_the_selected_file() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config");
    std::fs::write(
        &config,
        "Host rain\n HostName 192.0.2.30\n User root\n IdentityFile \"D:\\keys\\key one.pem\"\n",
    )
    .unwrap();
    let output = super::ssh_config_command("rain", Some(&config)).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let target = parse_resolved_config("rain", &String::from_utf8_lossy(&output.stdout)).unwrap();
    assert_eq!(target.host, "192.0.2.30");
    assert_eq!(target.user, "root");
    assert_eq!(target.identity_files, vec![PathBuf::from(r"D:\keys\key one.pem")]);
}

#[test]
fn unknown_host_needs_explicit_trust_and_changed_keys_remain_rejected() {
    use russh::client::Handler as _;
    use russh::keys::ssh_key::{Algorithm, PrivateKey};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("known_hosts");
    let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
    let changed = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
    let mut handler = super::ClientHandler {
        host: "fixture.example".into(),
        port: 2200,
        allow_prompt: false,
        handshake: super::lifecycle::Handshake::default(),
        known_hosts_path: Some(path.clone()),
    };
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    runtime.block_on(async {
        assert!(!handler.verify_host_key(key.public_key()).unwrap());
        assert!(
            !handler
                .confirm_unknown_key(key.public_key(), async {
                    panic!("unattended probes must not prompt")
                })
                .await
        );
        handler.allow_prompt = true;
        assert!(!handler.confirm_unknown_key(key.public_key(), async { false }).await);
        assert!(!path.exists(), "Cancel must not record trust");
        assert!(handler.confirm_unknown_key(key.public_key(), async { true }).await);
        assert!(handler.check_server_key(key.public_key()).await.unwrap());
        let trusted = std::fs::read(&path).unwrap();
        assert!(handler.verify_host_key(changed.public_key()).is_err());
        handler.allow_prompt = false;
        assert!(handler.check_server_key(changed.public_key()).await.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), trusted);
    });
}

#[test]
fn fallback_ssh_config_resolves_aliases_when_openssh_probe_cannot_run() {
    let config = concat!(
        "\u{feff}\r\n",
        "Host *\r\n",
        "    IdentityFile ~/.ssh/default_key\r\n",
        "Host rain staging !blocked\r\n",
        "    HostName 192.168.100.3\r\n",
        "    User root\r\n",
        "    Port 2200\r\n",
        "    IdentityFile \"D:\\keys\\key one.pem\"\r\n",
        "    ProxyJump bastion\r\n",
    );
    let destination = resolve_from_ssh_config_text("rain", config).unwrap().unwrap();
    assert_eq!(destination.original, "rain");
    assert_eq!(destination.user, "root");
    assert_eq!(destination.host, "192.168.100.3");
    assert_eq!(destination.port, 2200);
    let default_key = crate::platform::dirs::home_dir()
        .expect("test environment must expose a home directory")
        .join(".ssh")
        .join("default_key");
    assert_eq!(
        destination.identity_files,
        vec![default_key, PathBuf::from(r"D:\keys\key one.pem"),]
    );
    assert_eq!(destination.proxy_jump.as_deref(), Some("bastion"));

    let explicit = resolve_from_ssh_config_text("deploy@rain:2222", config).unwrap().unwrap();
    assert_eq!(explicit.user, "deploy");
    assert_eq!(explicit.port, 2222);
}

#[test]
fn fallback_refuses_incomplete_routes_instead_of_connecting_directly() {
    for directive in [
        "Include other.conf",
        "Match host rain",
        "ProxyCommand nc proxy 22",
        "HostName %h.example",
        "Port broken",
        "HostName \"unterminated",
    ] {
        let config = format!("Host rain\n User root\n {directive}\n");
        assert!(resolve_from_ssh_config_text("rain", &config).is_err(), "{directive}");
    }
    let config = "Host rain\n User=root\n HostName = 192.0.2.1\n ProxyJump none\nHost *\n ProxyJump bastion\n";
    let target = resolve_from_ssh_config_text("rain", config).unwrap().unwrap();
    assert_eq!(target.host, "192.0.2.1");
    assert!(target.proxy_jump.is_none());
    let config = "Host prod* !prod-test\n User deploy\n HostName actual.example\n";
    assert!(resolve_from_ssh_config_text("prod-test", config).unwrap().is_none());
    assert_eq!(resolve_from_ssh_config_text("PROD-1", config).unwrap().unwrap().user, "deploy");
}

#[test]
fn openssh_probe_normalizes_legacy_destinations_with_explicit_ports() {
    assert_eq!(
        ssh_config_probe_target("root@154.64.232.109:20222"),
        "ssh://root@154.64.232.109:20222"
    );
    assert_eq!(
        ssh_config_probe_target("root@[2001:db8::1]:20222"),
        "ssh://root@[2001:db8::1]:20222"
    );
    assert_eq!(
        ssh_config_probe_target("ssh://root@example.com:20222"),
        "ssh://root@example.com:20222"
    );
    assert_eq!(ssh_config_probe_target("root@example.com"), "root@example.com");
    assert_eq!(ssh_config_probe_target("root@2001:db8::1"), "root@2001:db8::1");
}

#[test]
fn duplicated_ssh_tabs_quote_their_remote_working_directory() {
    assert_eq!(
        initial_remote_cd_command(Some("/srv/Team's App")),
        Some(b"cd '/srv/Team'\\''s App'\r".to_vec())
    );
    assert_eq!(initial_remote_cd_command(Some("relative/path")), None);
    assert_eq!(initial_remote_cd_command(Some("/srv/app\nwhoami")), None);
}

#[test]
fn proxy_test_timeout_copy_matches_shared_budget() {
    // 353bc17 把这条文案搬进 i18n 目录，秒数改由 `ProxyTestFailure` 携带
    // （旧的 `proxy_test_timeout_message` 随之删除，这个测试的引用被漏下
    // 了）。判据不变：用户看到的文案必须和共享的超时预算是同一个数。
    let timeout =
        crate::proxy_test::ProxyTestOutcome::Failed(crate::proxy_test::ProxyTestFailure::Timeout {
            seconds: super::TEST_TIMEOUT.as_secs(),
        });
    assert_eq!(
        crate::display::UiLanguage::ZhCn.proxy_test_message(&timeout, 0),
        "网络测试超时（12 秒无响应）"
    );
    assert_eq!(super::TEST_TIMEOUT.as_secs(), 12);
}

#[test]
fn ssh_runtime_uses_global_network_proxy_and_keeps_openssh_proxy_jump() {
    let global = SshProxyConfig {
        mode: ProxyMode::Custom,
        url: "socks5://127.0.0.1:7890".to_owned(),
        no_proxy: Vec::new(),
    };
    let direct_target = SshDestination::parse("root@example.com").unwrap();
    assert!(matches!(
        resolve_network_proxy(&global, &direct_target).unwrap(),
        Some(ProxyLink::Server(_))
    ));

    let mut jump_target = direct_target;
    jump_target.proxy_jump = Some("bastion.internal".to_owned());
    assert_eq!(
        resolve_network_proxy(&global, &jump_target).unwrap(),
        Some(ProxyLink::Jump("bastion.internal".to_owned()))
    );
}

#[test]
fn auto_auth_plan_keeps_key_order_and_deduplicates_keys() {
    let explicit = vec![PathBuf::from(r"C:\Keys\chosen"), PathBuf::from(r"c:\keys\CHOSEN")];
    let resolved = vec![PathBuf::from(r"c:\keys\Chosen"), PathBuf::from(r"C:\Keys\config")];

    assert_eq!(
        authentication_plan(SshAuthMode::Auto, &explicit, &resolved),
        vec![
            AuthMethod::PrivateKey(PathBuf::from(r"C:\Keys\chosen")),
            AuthMethod::Agent,
            AuthMethod::PrivateKey(PathBuf::from(r"C:\Keys\config")),
            AuthMethod::StoredPassword,
            AuthMethod::KeyboardInteractive,
            AuthMethod::PromptPassword,
        ]
    );
}

#[test]
fn password_mode_supports_pam_without_falling_back_to_keys() {
    assert_eq!(
        authentication_plan(
            SshAuthMode::Password,
            &[PathBuf::from(r"C:\Keys\ignored")],
            &[PathBuf::from(r"C:\Keys\ignored-config")],
        ),
        vec![
            AuthMethod::StoredPassword,
            AuthMethod::KeyboardInteractive,
            AuthMethod::PromptPassword,
        ]
    );
}

#[test]
fn keyboard_interactive_reuses_password_only_for_password_prompts() {
    for prompt in ["Password:", "Enter passphrase:", "密码：", "请输入口令："] {
        assert!(is_password_prompt(prompt), "应识别密码问题: {prompt}");
    }
    for prompt in ["Verification code:", "OTP:", "Token:", "Duo choice:"] {
        assert!(!is_password_prompt(prompt), "二次验证不得自动填登录密码: {prompt}");
    }
}

#[test]
fn public_key_mode_uses_only_key_sources() {
    assert_eq!(
        authentication_plan(
            SshAuthMode::PublicKey,
            &[PathBuf::from(r"C:\Keys\chosen")],
            &[PathBuf::from(r"C:\Keys\config")],
        ),
        vec![
            AuthMethod::PrivateKey(PathBuf::from(r"C:\Keys\chosen")),
            AuthMethod::PrivateKey(PathBuf::from(r"C:\Keys\config")),
        ]
    );
}

#[test]
fn interactive_mode_is_strict() {
    assert_eq!(
        authentication_plan(SshAuthMode::KeyboardInteractive, &[], &[]),
        vec![AuthMethod::KeyboardInteractive]
    );
}

// 私钥夹具只在测试进程内生成，避免仓库保存可被误用的静态密钥材料；
// LazyLock 让两个解析场景复用同一次较慢的 RSA 素数生成。
static TEST_RSA_PKCS1_PEM: LazyLock<Zeroizing<String>> = LazyLock::new(|| {
    let key = rsa::RsaPrivateKey::new(&mut rand::rng(), 1024).expect("测试用 RSA 密钥必须能生成");
    key.to_pkcs1_pem(RsaLineEnding::LF).expect("测试用 RSA 密钥必须能编码为 PKCS#1 PEM")
});

fn encrypted_openssh_pem(password: &str) -> Zeroizing<String> {
    let mut rng = rand::rng();
    let key = russh::keys::ssh_key::PrivateKey::random(
        &mut rng,
        russh::keys::ssh_key::Algorithm::Ed25519,
    )
    .expect("测试用 Ed25519 密钥必须能生成");
    key.encrypt(&mut rng, password)
        .expect("测试用 OpenSSH 密钥必须能加密")
        .to_openssh(russh::keys::ssh_key::LineEnding::LF)
        .expect("测试用 OpenSSH 密钥必须能编码为 PEM")
}

#[test]
fn rsa_pkcs1_pem_parses_without_passphrase() {
    let key = russh::keys::decode_secret_key(TEST_RSA_PKCS1_PEM.as_str(), None)
        .expect("PKCS#1 RSA .pem 必须无口令直接解析（russh 需要 rsa feature）");
    assert!(key.algorithm().is_rsa());
}

#[test]
fn rsa_pkcs1_pem_with_passphrase_also_parses() {
    // 用户把无口令密钥误存了口令时不该解析失败：无加密的 PEM 忽略口令。
    let key = russh::keys::decode_secret_key(TEST_RSA_PKCS1_PEM.as_str(), Some("whatever"))
        .expect("无加密 PEM 携带多余口令也应解析");
    assert!(key.algorithm().is_rsa());
}

#[test]
fn encrypted_openssh_key_is_classified_as_needing_passphrase() {
    let pem = encrypted_openssh_pem("test-passphrase");
    let err =
        russh::keys::decode_secret_key(pem.as_str(), None).expect_err("加密密钥无口令解析必须失败");
    assert!(super::key_needs_passphrase(&err, pem.as_bytes()));
    // 口令正确则解开——证明失败确实只是缺口令。
    russh::keys::decode_secret_key(pem.as_str(), Some("test-passphrase"))
        .expect("口令正确必须解开");
}

#[test]
fn pkcs8_encrypted_banner_is_classified_as_needing_passphrase() {
    let pem = [b"-----BEGIN ENCRYPTED ".as_slice(), b"PRIVATE KEY-----\nAAAA\n"].concat();
    let pem = String::from_utf8(pem).expect("测试 PEM 必须是 UTF-8");
    let err = russh::keys::decode_secret_key(&pem, None).expect_err("占位密文必须解析失败");
    assert!(super::key_needs_passphrase(&err, pem.as_bytes()));
}

#[test]
fn unparseable_key_is_not_classified_as_needing_passphrase() {
    // 格式不支持/损坏绝不能进口令流程——那正是「无口令 .pem 弹出系统
    // 口令框」事故的形态。
    let garbage = "this is not a private key";
    let err = russh::keys::decode_secret_key(garbage, None).expect_err("垃圾输入必须解析失败");
    assert!(!super::key_needs_passphrase(&err, garbage.as_bytes()));
}

#[test]
fn all_keys_failing_locally_is_not_reported_as_server_rejection() {
    let errors = vec!["C:\\keys\\a.pem: 无法解析（unsupported）".to_owned()];
    let message = super::auth_failure(SshAuthMode::PublicKey, 1, &errors);
    assert!(message.starts_with("私钥无法使用"), "实际文案: {message}");
    assert!(!message.contains("服务器拒绝"), "实际文案: {message}");

    // 有密钥真的送到了服务器（本地失败数 < 密钥数）时保留原判词。
    let partial = super::auth_failure(SshAuthMode::PublicKey, 2, &errors);
    assert!(partial.contains("服务器拒绝"), "实际文案: {partial}");
    assert!(partial.contains("本地密钥问题"), "实际文案: {partial}");
}
