use super::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Preferences {
    pub enabled: bool,
    pub lan_enabled: bool,
    pub relay_enabled: bool,
    pub address: Option<IpAddr>,
    pub port: u16,
    pub default_input: bool,
    pub notifications: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            enabled: false,
            lan_enabled: true,
            relay_enabled: false,
            address: None,
            port: 0,
            default_input: false,
            notifications: true,
        }
    }
}

fn key(name: &str) -> String {
    // 独立配置实例和管理员实例不继承另一实例的手机授权。
    let path = crate::display::nebula_data_dir();
    let identity =
        format!("{}:{}", path.display(), crate::platform::elevation::requires_isolation());
    let digest = Sha256::digest(identity.as_bytes());
    let scope: String = digest[..12].iter().map(|b| format!("{b:02x}")).collect();
    format!("Pebrel/Mobile/V2/{scope}/{name}")
}

pub(super) fn load(name: &str) -> Result<Option<Vec<u8>>, Failure> {
    crate::platform::credentials::load(&key(name)).map_err(|_| Failure::Credentials)
}

pub(super) fn store(name: &str, bytes: &[u8]) -> Result<(), Failure> {
    crate::platform::credentials::store(&key(name), bytes).map_err(|_| Failure::Credentials)
}

/// 凭据由一次系统写入提交，避免「偏好已保存、令牌仍是旧值」的半成功状态。
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct Configuration {
    pub preferences: Preferences,
    pub relay: Option<String>,
    pub lan: Option<String>,
}

impl Configuration {
    pub(super) fn load() -> Result<Self, Failure> {
        load("configuration")?
            .map(|bytes| serde_json::from_slice(&bytes).map_err(|_| Failure::Credentials))
            .transpose()
            .map(Option::unwrap_or_default)
    }

    pub(super) fn store(&self) -> Result<(), Failure> {
        store("configuration", &serde_json::to_vec(self).map_err(|_| Failure::Invalid)?)
    }
}
