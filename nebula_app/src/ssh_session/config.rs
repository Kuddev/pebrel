//! Conservative fallback for simple ssh_config files when OpenSSH cannot expand them.
//! OpenSSH remains authoritative for Include, Match, tokens and other advanced options.

use super::{SshDestination, default_identity_files, expand_home, parse_host_port_optional};
use std::io;

pub(super) fn resolve_from_ssh_config(original: &str) -> io::Result<Option<SshDestination>> {
    match crate::ssh::read_ssh_config() {
        Ok(text) => resolve_from_ssh_config_text(original, &text),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn invalid(detail: impl std::fmt::Display) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("Cannot safely expand SSH config without OpenSSH: {detail}"),
    )
}

pub(super) fn resolve_from_ssh_config_text(
    original: &str,
    text: &str,
) -> io::Result<Option<SshDestination>> {
    let address = original.strip_prefix("ssh://").unwrap_or(original);
    let (explicit_user, host_port) =
        address.rsplit_once('@').map_or((None, address), |(user, target)| (Some(user), target));
    let (alias, explicit_port) = parse_host_port_optional(host_port)?;
    let (mut user, mut hostname, mut port, mut proxy_jump) = (None, None, None, None);
    let mut identities = Vec::new();
    let mut identities_specified = false;
    let mut active = true;
    let mut matched = false;
    for line in text.lines() {
        let tokens = crate::ssh::ssh_config_tokens_checked(line)?;
        let Some(keyword) = tokens.first() else { continue };
        let keyword = keyword.to_ascii_lowercase();
        let values = &tokens[1..];
        if matches!(keyword.as_str(), "include" | "match") {
            return Err(invalid(format!("{keyword} requires the system SSH client")));
        }
        if keyword == "host" {
            if values.is_empty() {
                return Err(invalid("Host has no patterns"));
            }
            active = block_matches_alias(values, &alias);
            matched |= active;
            continue;
        }
        if !active {
            continue;
        }
        let [value] = values else { return Err(invalid(format!("invalid {keyword} value"))) };
        if value.contains('%') || value.contains("${") {
            return Err(invalid(format!("{keyword} expansion requires the system SSH client")));
        }
        matched = true;
        match keyword.as_str() {
            "user" => {
                user.get_or_insert_with(|| value.clone());
            },
            "hostname" => {
                hostname.get_or_insert_with(|| value.clone());
            },
            "port" => {
                let parsed = value
                    .parse::<u16>()
                    .ok()
                    .filter(|port| *port != 0)
                    .ok_or_else(|| invalid("invalid Port"))?;
                port.get_or_insert(parsed);
            },
            "identityfile" => {
                identities_specified = true;
                if !value.eq_ignore_ascii_case("none") {
                    let path = expand_home(value);
                    if !identities.contains(&path) {
                        identities.push(path);
                    }
                }
            },
            // Keep `none` as a set value so later wildcard blocks cannot override it.
            "proxyjump" => {
                proxy_jump.get_or_insert_with(|| value.clone());
            },
            "proxycommand" if value.eq_ignore_ascii_case("none") => {},
            _ => return Err(invalid(format!("{keyword} requires the system SSH client"))),
        }
    }
    if !matched {
        return Ok(None);
    }
    let user = explicit_user
        .map(str::to_owned)
        .or(user)
        .or_else(|| std::env::var("USERNAME").ok())
        .or_else(|| std::env::var("USER").ok())
        .ok_or_else(|| invalid("no SSH username available"))?;
    let host = hostname.unwrap_or(alias);
    if user.is_empty()
        || host.is_empty()
        || host.chars().any(|ch| ch.is_whitespace() || ch.is_control())
    {
        return Err(invalid("invalid User or HostName"));
    }
    Ok(Some(SshDestination {
        original: original.to_owned(),
        user,
        host,
        port: explicit_port.or(port).unwrap_or(22),
        identity_files: if identities_specified { identities } else { default_identity_files() },
        proxy_jump: proxy_jump.filter(|jump| !jump.eq_ignore_ascii_case("none")),
    }))
}

fn block_matches_alias(patterns: &[String], alias: &str) -> bool {
    let mut positive = false;
    for pattern in patterns {
        if let Some(negative) = pattern.strip_prefix('!') {
            if glob_matches(negative, alias) {
                return false;
            }
        } else if glob_matches(pattern, alias) {
            positive = true;
        }
    }
    positive
}

fn glob_matches(pattern: &str, value: &str) -> bool {
    let pattern = pattern.to_ascii_lowercase().chars().collect::<Vec<_>>();
    let value = value.to_ascii_lowercase().chars().collect::<Vec<_>>();
    let (mut pattern_index, mut value_index) = (0, 0);
    let mut star = None;
    let mut star_value_index = 0;
    while value_index < value.len() {
        if pattern_index < pattern.len()
            && (pattern[pattern_index] == '?' || pattern[pattern_index] == value[value_index])
        {
            pattern_index += 1;
            value_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == '*' {
            star = Some(pattern_index);
            star_value_index = value_index;
            pattern_index += 1;
        } else if let Some(star_index) = star {
            pattern_index = star_index + 1;
            star_value_index += 1;
            value_index = star_value_index;
        } else {
            return false;
        }
    }
    while pattern_index < pattern.len() && pattern[pattern_index] == '*' {
        pattern_index += 1;
    }
    pattern_index == pattern.len()
}
