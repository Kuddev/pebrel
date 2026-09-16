use std::net::Ipv6Addr;

use serde::{Deserialize, Serialize};

use crate::i18n::t;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SshHostProxyMode {
    #[default]
    Inherit,
    Direct,
    Socks5,
    Http,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SshHostJumpMode {
    #[default]
    Inherit,
    None,
    Host,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SshConnectionOptions {
    pub proxy_mode: SshHostProxyMode,
    pub proxy_host: String,
    pub proxy_port: Option<u16>,
    pub proxy_username: String,
    pub jump_mode: SshHostJumpMode,
    pub jump_host: String,
}

impl SshConnectionOptions {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }

    pub fn effective_proxy_port(&self) -> u16 {
        self.proxy_port.unwrap_or(match self.proxy_mode {
            SshHostProxyMode::Http => 8080,
            _ => 1080,
        })
    }

    pub fn has_custom_proxy(&self) -> bool {
        matches!(self.proxy_mode, SshHostProxyMode::Socks5 | SshHostProxyMode::Http)
    }

    pub fn normalized_proxy_host(&self) -> String {
        let host = self.proxy_host.trim();
        host.strip_prefix('[')
            .and_then(|host| host.strip_suffix(']'))
            .unwrap_or(host)
            .to_ascii_lowercase()
    }

    pub fn validate(&self, destination: &str) -> Result<(), String> {
        self.validate_in(destination, crate::i18n::UiLanguage::current())
    }

    fn validate_in(
        &self,
        destination: &str,
        language: crate::i18n::UiLanguage,
    ) -> Result<(), String> {
        validate_ssh_destination(destination)?;
        if self.has_custom_proxy() {
            validate_host(&self.normalized_proxy_host())
                .map_err(|_| language.tr("ssh.connection.proxy_host_invalid").to_string())?;
            if self.effective_proxy_port() == 0 {
                return Err(language.tr("ssh.connection.proxy_port_range").to_string());
            }
            let username = self.proxy_username.trim();
            if username.chars().any(char::is_control) {
                return Err(language.tr("ssh.connection.proxy_username_control").to_string());
            }
            if self.proxy_mode == SshHostProxyMode::Socks5 && username.len() > 255 {
                return Err(language.tr("ssh.connection.socks5_username_too_long").to_string());
            }
            if self.proxy_mode == SshHostProxyMode::Http && username.contains(':') {
                return Err(language.tr("ssh.connection.http_username_colon").to_string());
            }
        }
        if self.jump_mode == SshHostJumpMode::Host {
            validate_ssh_destination(&self.jump_host)
                .map_err(|_| language.tr("ssh.connection.jump_host_invalid").to_string())?;
            if normalized_destination(&self.jump_host) == normalized_destination(destination) {
                return Err(language.tr("ssh.connection.jump_is_self").to_string());
            }
        }
        Ok(())
    }

    pub fn proxy_credential_target(&self, destination: &str) -> Option<String> {
        if !self.has_custom_proxy() || self.proxy_username.trim().is_empty() {
            return None;
        }
        use sha2::{Digest, Sha256};
        use std::fmt::Write as _;

        let mode = match self.proxy_mode {
            SshHostProxyMode::Socks5 => "socks5",
            SshHostProxyMode::Http => "http",
            _ => return None,
        };
        let mut digest = Sha256::new();
        for field in [
            destination.trim().to_owned(),
            mode.to_owned(),
            self.normalized_proxy_host(),
            self.effective_proxy_port().to_string(),
            self.proxy_username.trim().to_owned(),
        ] {
            digest.update((field.len() as u64).to_be_bytes());
            digest.update(field.as_bytes());
        }
        let mut fingerprint = String::with_capacity(64);
        for byte in digest.finalize() {
            let _ = write!(fingerprint, "{byte:02x}");
        }
        Some(format!("Pebrel/SSH/Proxy/{fingerprint}"))
    }
}

pub(crate) fn validate_ssh_destination(value: &str) -> Result<(), String> {
    let value = value.trim();
    if value.is_empty()
        || value.chars().any(|character| {
            character.is_whitespace()
                || character.is_control()
                || ",;&|<>\"'`\\?#".contains(character)
        })
    {
        return Err(t!("ssh.connection.destination_empty_or_chars").to_string());
    }
    let address = value.strip_prefix("ssh://").unwrap_or(value);
    if address.starts_with('-') {
        return Err(t!("ssh.connection.destination_option_prefix").to_string());
    }
    let host_port = if let Some((username, host)) = address.rsplit_once('@') {
        if username.is_empty() || username.contains(['@', ':', '/', '[', ']']) {
            return Err(t!("ssh.connection.username_invalid").to_string());
        }
        host
    } else {
        address
    };
    let (host, port) = if let Some(rest) = host_port.strip_prefix('[') {
        let (host, suffix) = rest
            .split_once(']')
            .ok_or_else(|| t!("ssh.connection.ipv6_missing_bracket").to_string())?;
        if host.parse::<Ipv6Addr>().is_err() {
            return Err(t!("ssh.connection.ipv6_invalid").to_string());
        }
        let port = if suffix.is_empty() {
            None
        } else {
            Some(
                suffix
                    .strip_prefix(':')
                    .ok_or_else(|| t!("ssh.connection.port_format").to_string())?,
            )
        };
        (host, port)
    } else if let Some((host, port)) = host_port.rsplit_once(':') {
        if host.contains(':') { (host_port, None) } else { (host, Some(port)) }
    } else {
        (host_port, None)
    };
    validate_host(host)?;
    if port.is_some_and(|port| port.parse::<u16>().map_or(true, |port| port == 0)) {
        return Err(t!("ssh.connection.port_range").to_string());
    }
    Ok(())
}

fn validate_host(host: &str) -> Result<(), String> {
    if host.is_empty() || host.starts_with('-') || host.len() > 253 {
        return Err(t!("ssh.connection.hostname_invalid").to_string());
    }
    if host.contains(':') {
        host.parse::<Ipv6Addr>().map_err(|_| t!("ssh.connection.ipv6_invalid").to_string())?;
    } else if host
        .chars()
        .any(|character| !character.is_alphanumeric() && !matches!(character, '.' | '-' | '_'))
    {
        return Err(t!("ssh.connection.hostname_chars").to_string());
    }
    Ok(())
}

fn normalized_destination(destination: &str) -> String {
    let destination = destination.trim().strip_prefix("ssh://").unwrap_or(destination.trim());
    let (username, host) = destination.rsplit_once('@').unwrap_or(("", destination));
    let host = host.strip_suffix(":22").unwrap_or(host).to_ascii_lowercase();
    format!("{username}@{host}")
}

#[cfg(test)]
#[path = "connection/tests.rs"]
mod tests;
