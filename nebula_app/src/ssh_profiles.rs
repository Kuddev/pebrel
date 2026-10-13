use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize};

#[path = "ssh_profiles/connection.rs"]
mod connection;
pub(crate) mod duplication;
#[path = "ssh_profiles/exchange.rs"]
pub(crate) mod exchange;
#[path = "ssh_profiles/organization.rs"]
mod organization;
pub(crate) use connection::validate_ssh_destination;
pub use connection::{SshConnectionOptions, SshHostJumpMode, SshHostProxyMode};
pub(crate) use organization::{HostOrganization, merge_host_sources};

const PROFILE_VERSION: u32 = 1;
const USERNAME_HISTORY_CAP: usize = 12;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SshAuthMode {
    #[default]
    Auto,
    Password,
    PublicKey,
    KeyboardInteractive,
}

impl<'de> Deserialize<'de> for SshAuthMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(match value.as_str() {
            "password" => Self::Password,
            "public_key" => Self::PublicKey,
            // v0.5 移除了 Agent；旧配置回退到 Auto，避免升级后配置失效。
            "agent" => Self::Auto,
            "keyboard_interactive" => Self::KeyboardInteractive,
            _ => Self::Auto,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SshProfileAuth {
    pub destination: String,
    #[serde(default)]
    pub auth: SshAuthMode,
    #[serde(default)]
    pub private_keys: Vec<PathBuf>,
    /// 列表里显示的名字。`None` / 空串都表示回落到地址本身，所以旧配置文件
    /// 不需要迁移就能读——`serde(default)` 让缺字段等价于"没起名字"。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// 侧栏图标，取 `ui::os_icons::CATALOG` 里的 id（"debian"、"docker"…）。
    /// `None` = 自动：跟着远端认出来的系统走，认不出用通用终端。存 id 而不是
    /// 码位——码位属于某一个字体文件，换字体就作废；id 是我们自己的稳定键。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "SshConnectionOptions::is_default")]
    pub connection: SshConnectionOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshProfiles {
    #[serde(default = "profile_version")]
    version: u32,
    #[serde(default)]
    profiles: Vec<SshProfileAuth>,
    /// 最近使用的 SSH 用户名，最新在前。它不含密码或主机，因此可以和
    /// Profile 一起持久化；旧文件缺少字段时由 `serde(default)` 读成空列表。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    usernames: Vec<String>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    organization: std::collections::BTreeMap<String, HostOrganization>,
    /// 副本的列表/凭据身份独立于连接地址；旧主机没有此项，仍以地址作为身份。
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    targets: std::collections::BTreeMap<String, String>,
    #[serde(skip)]
    source_bytes: Option<Vec<u8>>,
}

impl Default for SshProfiles {
    fn default() -> Self {
        Self {
            version: PROFILE_VERSION,
            profiles: Vec::new(),
            usernames: Vec::new(),
            organization: Default::default(),
            targets: Default::default(),
            source_bytes: None,
        }
    }
}

impl SshProfiles {
    pub fn load(path: &Path) -> io::Result<Self> {
        let data = match std::fs::read(path) {
            Ok(data) => data,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(err) => return Err(err),
        };
        let mut profiles: Self = serde_json::from_slice(&data)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        for profile in &mut profiles.profiles {
            deduplicate_key_paths(&mut profile.private_keys);
        }
        for value in profiles.organization.values() {
            value.validate().map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        }
        normalize_usernames(&mut profiles.usernames);
        profiles
            .validate_targets()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        profiles.version = PROFILE_VERSION;
        profiles.source_bytes = Some(data);
        Ok(profiles)
    }

    pub fn save(&mut self, path: &Path) -> io::Result<()> {
        self.validate_targets()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Reuse the OS-handle lease, scoped to this write. A stale snapshot must
        // never replace a profile another window has just edited.
        let _lease = crate::atomic_file::try_lifetime_lock(path)?.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::WouldBlock,
                "Host profiles are being saved by another window",
            )
        })?;
        let current = match std::fs::read(path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        if current != self.source_bytes {
            return Err(io::Error::other("Host profiles changed. Reload before saving."));
        }
        let data = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        crate::atomic_file::write(path, &data)?;
        self.source_bytes = Some(data);
        Ok(())
    }

    pub fn for_destination(&self, destination: &str) -> SshProfileAuth {
        self.profiles
            .iter()
            .find(|profile| profile.destination == destination)
            .cloned()
            .unwrap_or_else(|| SshProfileAuth {
                destination: destination.to_owned(),
                auth: SshAuthMode::Auto,
                private_keys: Vec::new(),
                label: None,
                icon: None,
                connection: SshConnectionOptions::default(),
            })
    }

    /// 地址 → 显示名。侧栏每帧都要画主机行，不能每帧读一次 JSON，所以调用方
    /// 把结果缓存起来，只在编辑器保存后刷新。
    pub fn labels(&self) -> std::collections::HashMap<String, String> {
        self.profiles
            .iter()
            .filter_map(|profile| {
                let label = profile.label.as_deref()?.trim();
                (!label.is_empty()).then(|| (profile.destination.clone(), label.to_owned()))
            })
            .collect()
    }

    /// 地址 → 图标 id。与 [`Self::labels`] 同一缓存策略：侧栏每帧要画，调
    /// 用方缓存住，编辑器保存后刷新。
    pub fn icons(&self) -> std::collections::HashMap<String, String> {
        self.profiles
            .iter()
            .filter_map(|profile| {
                let icon = profile.icon.as_deref()?.trim();
                (!icon.is_empty()).then(|| (profile.destination.clone(), icon.to_owned()))
            })
            .collect()
    }

    /// 可编辑用户名下拉的候选。显式历史保持 MRU 次序；旧版本没有历史字段，
    /// 因而再从现有 `user@host` Profile 补齐，升级后第一次打开就有可选项。
    pub fn usernames(&self) -> Vec<String> {
        let mut usernames = self.usernames.clone();
        for profile in &self.profiles {
            if let Some(username) =
                destination_username(self.connection_destination(&profile.destination))
            {
                push_unique_username(&mut usernames, username);
            }
        }
        usernames.truncate(USERNAME_HISTORY_CAP);
        usernames
    }

    /// 记录一次实际保存过的用户名。大小写不同可能代表不同远端账号，去重只做
    /// 精确比较；重新使用会被提升到最前面。
    pub fn remember_username(&mut self, username: &str) {
        let username = username.trim();
        if !valid_username_history_entry(username) {
            return;
        }
        self.usernames.retain(|existing| existing != username);
        self.usernames.insert(0, username.to_owned());
        self.usernames.truncate(USERNAME_HISTORY_CAP);
    }

    /// 下一个可用的默认标签，形如「主机 6」。
    ///
    /// 取现有默认标签里**编号的最大值 +1**，而不是条目总数：删掉中间几台之后
    /// 再新建，用总数必然撞上还在列表里的名字。用户自己起的名字（不匹配这个
    /// 前缀+数字的形式）不参与计算，也就不会被它挤占。
    ///
    /// 语言在生成的那一刻定下来就不再变——label 是**用户数据**，跟着界面语言
    /// 来回翻译，等于我们替用户改了他列表里的条目名。
    pub fn next_default_label(&self, prefix: &str) -> String {
        let max = self
            .profiles
            .iter()
            .filter_map(|profile| profile.label.as_deref())
            .filter_map(|label| label.trim().strip_prefix(prefix))
            .filter_map(|rest| rest.trim().parse::<u32>().ok())
            .max()
            .unwrap_or(0);
        format!("{prefix} {}", max + 1)
    }

    pub fn upsert(&mut self, mut profile: SshProfileAuth) {
        deduplicate_key_paths(&mut profile.private_keys);
        if let Some(existing) =
            self.profiles.iter_mut().find(|existing| existing.destination == profile.destination)
        {
            *existing = profile;
        } else {
            self.profiles.push(profile);
        }
    }

    pub fn remove(&mut self, destination: &str) {
        self.profiles.retain(|profile| profile.destination != destination);
        self.organization.remove(destination);
        self.targets.remove(destination);
    }

    pub fn jump_dependents(&self, destination: &str) -> Vec<String> {
        self.profiles
            .iter()
            .filter(|profile| {
                profile.destination != destination
                    && profile.connection.jump_mode == SshHostJumpMode::Host
                    && profile.connection.jump_host.trim() == destination
            })
            .map(|profile| profile.destination.clone())
            .collect()
    }

    pub fn rename(&mut self, old: &str, new: &str) {
        let organization = self.organization.remove(old);
        let target = self.targets.remove(old);
        if let Some(mut profile) =
            self.profiles.iter().find(|profile| profile.destination == old).cloned()
        {
            self.remove(old);
            profile.destination = new.to_owned();
            self.upsert(profile);
        }
        if let Some(organization) = organization {
            self.organization.insert(new.to_owned(), organization);
        }
        if let Some(target) = target {
            self.targets.insert(new.to_owned(), target);
        }
        for profile in &mut self.profiles {
            if profile.connection.jump_mode == SshHostJumpMode::Host
                && profile.connection.jump_host.trim() == old
            {
                profile.connection.jump_host = new.to_owned();
            }
        }
    }
}

fn profile_version() -> u32 {
    PROFILE_VERSION
}

fn destination_username(destination: &str) -> Option<&str> {
    let address = destination.trim().strip_prefix("ssh://").unwrap_or(destination.trim());
    let (username, host) = address.rsplit_once('@')?;
    (valid_username_history_entry(username) && !host.is_empty()).then_some(username)
}

fn valid_username_history_entry(username: &str) -> bool {
    !username.is_empty()
        && !username
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control() || "@;&|<>\"'`".contains(ch))
}

fn push_unique_username(usernames: &mut Vec<String>, username: &str) {
    if valid_username_history_entry(username) && !usernames.iter().any(|entry| entry == username) {
        usernames.push(username.to_owned());
    }
}

fn normalize_usernames(usernames: &mut Vec<String>) {
    let mut normalized = Vec::with_capacity(usernames.len().min(USERNAME_HISTORY_CAP));
    for username in std::mem::take(usernames) {
        push_unique_username(&mut normalized, username.trim());
    }
    normalized.truncate(USERNAME_HISTORY_CAP);
    *usernames = normalized;
}

fn deduplicate_key_paths(paths: &mut Vec<PathBuf>) {
    let mut seen = Vec::<String>::new();
    paths.retain(|path| {
        let normalized = path.to_string_lossy().to_lowercase();
        if seen.contains(&normalized) {
            false
        } else {
            seen.push(normalized);
            true
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{SshAuthMode, SshProfileAuth, SshProfiles};
    use std::path::PathBuf;

    #[test]
    fn host_copies_keep_independent_targets_metadata_and_csv_roundtrips() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ssh_profiles.json");
        let mut profiles = SshProfiles::default();
        let mut source = profiles.for_destination("root@example.com:2222");
        source.label = Some("生产".into());
        source.auth = SshAuthMode::PublicKey;
        source.private_keys.push("key.pem".into());
        source.icon = Some("debian".into());
        source.connection.jump_mode = super::SshHostJumpMode::Host;
        source.connection.jump_host = "bastion".into();
        profiles.upsert(source.clone());
        let organization =
            super::HostOrganization::from_inputs("公司", "linux,db", "中文备注").unwrap();
        profiles.set_organization(&source.destination, organization.clone()).unwrap();
        assert_eq!(profiles.duplicate_host(&source.destination, "copy-a").unwrap(), "生产 1");
        assert_eq!(profiles.duplicate_host(&source.destination, "copy-b").unwrap(), "生产 2");
        profiles.duplicate_host("copy-a", "copy-c").unwrap();
        let mut expected = source.clone();
        expected.destination = "copy-a".into();
        expected.label = Some("生产 1".into());
        assert_eq!(profiles.for_destination("copy-a"), expected);
        assert_eq!(profiles.organization("copy-a"), &organization);
        assert_eq!(profiles.connection_destination("copy-c"), source.destination);
        assert_eq!(
            profiles.edited_identity(Some("copy-a"), "operator@other:2200").unwrap(),
            "copy-a"
        );
        assert_eq!(profiles.for_destination(&source.destination), source);
        assert_eq!(profiles.connection_destination("copy-b"), source.destination);
        profiles.remove("copy-a");
        profiles.save(&path).unwrap();
        let restored = SshProfiles::load(&path).unwrap();
        assert_eq!(restored.connection_destination("copy-b"), source.destination);
        let csv = restored.export_csv();
        let mut imported = SshProfiles::default();
        imported.import_missing(&super::exchange::parse_csv(&csv).unwrap()).unwrap();
        assert_eq!(imported.connection_destination("copy-b"), source.destination);
        assert_eq!(imported.organization("copy-c"), &organization);
        assert_eq!(
            imported.filter_hosts(vec!["copy-b".into()], "example.com", false, None),
            vec!["copy-b"]
        );
    }

    #[test]
    fn copying_credentials_commits_only_after_success_and_rolls_back_failed_profile_writes() {
        use crate::ssh_credentials::credential_target;
        use std::cell::RefCell;
        use std::collections::HashMap;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ssh_profiles.json");
        let mut profiles = SshProfiles::default();
        let mut source = profiles.for_destination("root@host");
        source.connection.proxy_mode = super::SshHostProxyMode::Socks5;
        source.connection.proxy_host = "proxy".into();
        source.connection.proxy_username = "user".into();
        let proxy_key = source.connection.proxy_credential_target("root@host").unwrap();
        let copied_proxy_key = source.connection.proxy_credential_target("copy-a").unwrap();
        profiles.upsert(source);
        profiles.save(&path).unwrap();
        let stale = profiles.clone();
        let original = HashMap::from([
            (credential_target("root@host"), b"password".to_vec()),
            (proxy_key, b"proxy-secret".to_vec()),
        ]);
        let secrets = RefCell::new(original.clone());
        let copy = |profiles, identity| {
            super::duplication::save_duplicate(
                profiles,
                &path,
                "root@host",
                identity,
                |key| Ok(secrets.borrow().get(key).cloned()),
                |key, value| {
                    secrets.borrow_mut().insert(key.to_owned(), value.to_vec());
                    Ok(())
                },
                |key| {
                    secrets.borrow_mut().remove(key);
                    Ok(())
                },
            )
        };
        let (updated, _) = copy(profiles, "copy-a").unwrap();
        assert_eq!(secrets.borrow()[&credential_target("copy-a")], b"password");
        assert_eq!(secrets.borrow()[&copied_proxy_key], b"proxy-secret");
        let saved = std::fs::read(&path).unwrap();
        let credentials = secrets.borrow().clone();
        assert!(copy(stale, "copy-b").is_err(), "stale writes must not overwrite another copy");
        assert_eq!(*secrets.borrow(), credentials);
        assert_eq!(std::fs::read(&path).unwrap(), saved);
        assert!(!String::from_utf8(saved).unwrap().contains("proxy-secret"));
        assert_eq!(updated.connection_destination("copy-a"), "root@host");
        for (key, value) in original {
            assert_eq!(secrets.borrow()[&key], value);
        }
    }

    #[test]
    fn missing_profile_defaults_to_auto_without_keys() {
        let profiles = SshProfiles::default();

        assert_eq!(profiles.for_destination("dev@example.com").auth, SshAuthMode::Auto);
        assert!(profiles.for_destination("dev@example.com").private_keys.is_empty());
    }

    /// 图标是后加的字段，老配置文件里根本没有这一项——它必须能照读不误，
    /// 否则一次升级会让所有人的主机列表报错清空。
    #[test]
    fn profiles_written_before_icons_existed_still_load() {
        let old = r#"{"version":1,"profiles":[
            {"destination":"root@10.0.0.9","auth":"password","label":"生产库"}
        ]}"#;
        let profiles: SshProfiles = serde_json::from_str(old).expect("旧配置应当照读不误");
        let profile = profiles.for_destination("root@10.0.0.9");
        assert_eq!(profile.label.as_deref(), Some("生产库"));
        assert_eq!(profile.icon, None, "没写过图标 = 自动识别");
        assert!(profiles.icons().is_empty());
        assert_eq!(profiles.usernames(), vec!["root"], "旧 Profile 也应补出用户名候选");
    }

    /// 「自动识别」不落盘。存一个 `"auto"` 进去也能工作，但那样配置文件里
    /// 就分不出"用户挑了自动"和"用户根本没挑过"——而默认值将来可能变。
    #[test]
    fn only_explicitly_picked_icons_reach_the_file() {
        let mut profiles = SshProfiles::default();
        for (destination, icon) in
            [("root@10.0.0.1", Some("debian".to_owned())), ("root@10.0.0.2", None)]
        {
            profiles.upsert(SshProfileAuth {
                destination: destination.to_owned(),
                auth: SshAuthMode::Auto,
                private_keys: Vec::new(),
                label: None,
                icon,
                connection: Default::default(),
            });
        }
        let json = serde_json::to_string(&profiles).expect("serialize");
        assert!(json.contains(r#""icon":"debian""#));
        assert_eq!(json.matches("\"icon\"").count(), 1, "没挑过的那台不该写出空字段");

        let icons = profiles.icons();
        assert_eq!(icons.get("root@10.0.0.1").map(String::as_str), Some("debian"));
        assert!(!icons.contains_key("root@10.0.0.2"));
    }

    #[test]
    fn default_labels_continue_past_the_highest_number_not_the_count() {
        // 关键是"删掉中间几台之后"：按条目总数发号会直接撞上还在列表里的
        // 「主机 3」，用户看到两个同名条目。
        let mut profiles = SshProfiles::default();
        for n in 1..=3 {
            let label = profiles.next_default_label("主机");
            assert_eq!(label, format!("主机 {n}"));
            profiles.upsert(SshProfileAuth {
                destination: format!("root@10.0.0.{n}"),
                auth: SshAuthMode::Auto,
                private_keys: Vec::new(),
                label: Some(label),
                icon: None,
                connection: Default::default(),
            });
        }
        profiles.remove("root@10.0.0.1");
        profiles.remove("root@10.0.0.2");

        assert_eq!(profiles.next_default_label("主机"), "主机 4");
    }

    #[test]
    fn user_named_hosts_do_not_feed_the_default_counter() {
        // 自己起的名字不参与计数，否则「生产数据库 7」这种名字会把编号顶高。
        let mut profiles = SshProfiles::default();
        profiles.upsert(SshProfileAuth {
            destination: "root@example.com".to_owned(),
            auth: SshAuthMode::Auto,
            private_keys: Vec::new(),
            label: Some("生产数据库".to_owned()),
            icon: None,
            connection: Default::default(),
        });

        assert_eq!(profiles.next_default_label("主机"), "主机 1");
    }

    #[test]
    fn profile_round_trip_preserves_mode_and_key_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ssh_profiles.json");
        let mut profiles = SshProfiles::default();
        profiles.upsert(SshProfileAuth {
            destination: "dev@example.com".to_owned(),
            auth: SshAuthMode::PublicKey,
            private_keys: vec![PathBuf::from(r"C:\Keys\first"), PathBuf::from(r"C:\Keys\second")],
            label: None,
            icon: None,
            connection: Default::default(),
        });
        profiles.save(&path).unwrap();

        let loaded = SshProfiles::load(&path).unwrap();
        let profile = loaded.for_destination("dev@example.com");
        assert_eq!(profile.auth, SshAuthMode::PublicKey);
        assert_eq!(
            profile.private_keys,
            vec![PathBuf::from(r"C:\Keys\first"), PathBuf::from(r"C:\Keys\second")]
        );
    }

    #[test]
    fn duplicate_windows_key_paths_are_removed_without_reordering() {
        let mut profiles = SshProfiles::default();
        profiles.upsert(SshProfileAuth {
            destination: "dev@example.com".to_owned(),
            auth: SshAuthMode::Auto,
            private_keys: vec![
                PathBuf::from(r"C:\Keys\id_ed25519"),
                PathBuf::from(r"c:\keys\ID_ED25519"),
                PathBuf::from(r"C:\Keys\id_rsa"),
            ],
            label: None,
            icon: None,
            connection: Default::default(),
        });

        assert_eq!(
            profiles.for_destination("dev@example.com").private_keys,
            vec![PathBuf::from(r"C:\Keys\id_ed25519"), PathBuf::from(r"C:\Keys\id_rsa")]
        );
    }

    #[test]
    fn renaming_profile_moves_auth_metadata() {
        let mut profiles = SshProfiles::default();
        profiles.upsert(SshProfileAuth {
            destination: "old@example.com".to_owned(),
            auth: SshAuthMode::PublicKey,
            private_keys: vec![PathBuf::from(r"C:\Keys\id_ed25519")],
            label: None,
            icon: None,
            connection: Default::default(),
        });

        profiles.rename("old@example.com", "new@example.com");

        assert_eq!(profiles.for_destination("old@example.com").auth, SshAuthMode::Auto);
        assert_eq!(profiles.for_destination("new@example.com").auth, SshAuthMode::PublicKey);
    }

    #[test]
    fn legacy_agent_mode_migrates_to_auto() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ssh_profiles.json");
        std::fs::write(
            &path,
            r#"{"version":1,"profiles":[{"destination":"dev@example.com","auth":"agent","private_keys":[]}]}"#,
        )
        .unwrap();

        let loaded = SshProfiles::load(&path).unwrap();
        assert_eq!(loaded.for_destination("dev@example.com").auth, SshAuthMode::Auto);
    }

    #[test]
    fn unknown_auth_mode_falls_back_to_auto() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ssh_profiles.json");
        std::fs::write(
            &path,
            r#"{"version":1,"profiles":[{"destination":"dev@example.com","auth":"future-mode","private_keys":[]}]}"#,
        )
        .unwrap();

        let loaded = SshProfiles::load(&path).unwrap();
        assert_eq!(loaded.for_destination("dev@example.com").auth, SshAuthMode::Auto);
    }

    #[test]
    fn username_history_is_recent_first_bounded_and_backward_compatible() {
        let mut profiles = SshProfiles::default();
        profiles.remember_username("root");
        profiles.remember_username("deploy");
        profiles.remember_username("root");
        profiles.remember_username("bad user");

        assert_eq!(profiles.usernames(), vec!["root", "deploy"]);
        let json = serde_json::to_string(&profiles).expect("serialize");
        let loaded: SshProfiles = serde_json::from_str(&json).expect("round trip");
        assert_eq!(loaded.usernames(), vec!["root", "deploy"]);

        let legacy: SshProfiles = serde_json::from_str(
            r#"{"version":1,"profiles":[{"destination":"admin@example.com"}]}"#,
        )
        .expect("旧文件缺少 usernames 时仍应可读");
        assert_eq!(legacy.usernames(), vec!["admin"]);
    }
}
