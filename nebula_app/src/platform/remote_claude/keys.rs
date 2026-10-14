//! 连接期的一次性 Ed25519 密钥。
//!
//! 每次连接生成两对：`host` 是本机临时 sshd 的主机密钥（远端 `known_hosts`
//! 只认这一把，下次连接换新），`identity` 是远端回连本机时用的登录密钥。
//! 两者都只活到本次会话结束，不做任何持久化。
//!
//! 种子来自 `getrandom`（生产依赖）而不是 `rand`——`rand` 在本 crate 里只是
//! dev-dependency，实现代码用它编译不过。

use std::path::Path;

use russh::keys::ssh_key::private::Ed25519Keypair;
use russh::keys::ssh_key::{LineEnding, PrivateKey};
use zeroize::Zeroizing;

/// 一对新生成的密钥：私钥是 OpenSSH 文本（LF），公钥是单行 `ssh-ed25519 AAAA…`。
pub(super) struct KeyPair {
    pub(super) private: Zeroizing<String>,
    pub(super) public: String,
}

pub(super) fn generate() -> Result<KeyPair, String> {
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).map_err(|error| format!("no OS randomness: {error}"))?;
    let key = PrivateKey::from(Ed25519Keypair::from_seed(&seed));
    let private = key.to_openssh(LineEnding::LF).map_err(|error| error.to_string())?;
    let public = key.public_key().to_openssh().map_err(|error| error.to_string())?;
    Ok(KeyPair { private, public })
}

/// 写入私钥：OpenSSH 要求私钥文件以换行结尾，文件权限由调用方在目录上收口。
pub(super) fn write_private(path: &Path, key: &KeyPair) -> std::io::Result<()> {
    let mut text = key.private.to_string();
    if !text.ends_with('\n') {
        text.push('\n');
    }
    std::fs::write(path, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_keys_are_open_ssh_ed25519() {
        let key = generate().unwrap();
        assert!(key.private.starts_with("-----BEGIN OPENSSH PRIVATE KEY-----"));
        assert!(key.private.trim_end().ends_with("-----END OPENSSH PRIVATE KEY-----"));
        assert!(key.public.starts_with("ssh-ed25519 "));
        // 两把不同的密钥不能相同：种子每次都要重新取。
        assert_ne!(key.public, generate().unwrap().public);
    }

    #[test]
    fn written_private_key_ends_with_a_newline() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("identity");
        write_private(&path, &generate().unwrap()).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.ends_with('\n'));
        assert!(!text.contains('\r'));
    }
}
