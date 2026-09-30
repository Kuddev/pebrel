//! 从 Git 配置和本地引用推导可自动创建的分支；不执行 fetch 或 shell 脚本。

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use super::Reference;

#[derive(Debug, Default)]
struct Remote {
    fetch: Vec<(String, String)>,
    excluded: Vec<String>,
}

#[derive(Default)]
struct Matches {
    remotes: usize,
    preferred_usable: Option<bool>,
    any_usable: bool,
}

#[derive(Debug)]
pub(super) struct Config {
    pub guess: bool,
    default_remote: Option<String>,
    remotes: BTreeMap<String, Remote>,
}

impl Config {
    pub fn parse(text: &str) -> Self {
        let mut config = Self { guess: true, default_remote: None, remotes: BTreeMap::new() };
        for entry in text.split_terminator('\0') {
            let (key, value) = entry.split_once('\n').map_or((entry, None), |(k, v)| (k, Some(v)));
            match key {
                "checkout.guess" => {
                    config.guess = value.is_none_or(config_bool);
                },
                "checkout.defaultremote" => config.default_remote = value.map(str::to_owned),
                _ => {
                    let Some(name) =
                        key.strip_prefix("remote.").and_then(|s| s.strip_suffix(".fetch"))
                    else {
                        continue;
                    };
                    let Some(value) = value else { continue };
                    let remote = config.remotes.entry(name.to_owned()).or_default();
                    if let Some(excluded) = value.strip_prefix('^') {
                        remote.excluded.push(excluded.to_owned());
                    } else if let Some((source, destination)) =
                        value.trim_start_matches('+').split_once(':')
                    {
                        remote.fetch.push((source.to_owned(), destination.to_owned()));
                    }
                },
            }
        }
        config
    }

    pub fn guesses(&self, references: &[Reference], cancelled: &dyn Fn() -> bool) -> Vec<String> {
        let by_name: HashMap<_, _> = references.iter().map(|r| (r.full_name.as_str(), r)).collect();
        let mut shadowed = HashSet::new();
        for reference in references {
            let name = reference.full_name.as_str();
            shadowed.insert(name);
            for prefix in ["refs/", "refs/heads/", "refs/tags/", "refs/remotes/"] {
                if let Some(short) = name.strip_prefix(prefix) {
                    shadowed.insert(short);
                }
            }
            if let Some(short) =
                name.strip_prefix("refs/remotes/").and_then(|s| s.strip_suffix("/HEAD"))
            {
                shadowed.insert(short);
            }
        }
        let mut matches = BTreeMap::<String, Matches>::new();
        let mut excluded = HashSet::new();
        for (remote_name, remote) in &self.remotes {
            let mut seen = BTreeSet::new();
            for reference in references {
                for (source, destination) in &remote.fetch {
                    if cancelled() {
                        return Vec::new();
                    }
                    let Some(source_name) = substitute(destination, &reference.full_name, source)
                    else {
                        continue;
                    };
                    let Some(branch) = source_name.strip_prefix("refs/heads/") else { continue };
                    if !valid_branch(branch)
                        || shadowed.contains(branch)
                        || !seen.insert(branch.to_owned())
                    {
                        continue;
                    }
                    // 排除配置明确不再获取的分支，即使磁盘上仍留有旧跟踪引用。
                    if remote
                        .excluded
                        .iter()
                        .any(|pattern| captures(pattern, &source_name).is_some())
                    {
                        excluded.insert(branch.to_owned());
                        continue;
                    }
                    // Git 使用第一个匹配的 refspec，不能因其目标缺失而改用后面的映射。
                    let target = remote
                        .fetch
                        .iter()
                        .find_map(|(src, dst)| substitute(src, &source_name, dst));
                    let Some(target) = target.as_deref().and_then(|name| by_name.get(name)) else {
                        continue;
                    };
                    // 不可切换的同名引用仍参与 Git 的歧义判定，不能把另一个远端误当唯一。
                    let usable = target.commit && !target.symbolic;
                    let entry = matches.entry(branch.to_owned()).or_default();
                    entry.remotes += 1;
                    if self.default_remote.as_ref() == Some(remote_name) {
                        entry.preferred_usable = Some(usable);
                    }
                    entry.any_usable |= usable;
                }
            }
        }
        matches
            .into_iter()
            .filter_map(|(branch, matched)| {
                ((matched.remotes == 1 && matched.any_usable
                    || matched.preferred_usable == Some(true))
                    && !excluded.contains(&branch))
                .then_some(branch)
            })
            .collect()
    }
}

fn captures<'a>(pattern: &str, name: &'a str) -> Option<&'a str> {
    if let Some((prefix, suffix)) = pattern.split_once('*') {
        if suffix.contains('*') {
            return None;
        }
        name.strip_prefix(prefix)?.strip_suffix(suffix)
    } else {
        (pattern == name).then_some("")
    }
}

fn config_bool(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    if matches!(value.as_str(), "true" | "yes" | "on") {
        return true;
    }
    let value = value.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let (value, multiplier) = match value.as_bytes().last() {
        Some(b'k') => (&value[..value.len() - 1], 1_i64 << 10),
        Some(b'm') => (&value[..value.len() - 1], 1_i64 << 20),
        Some(b'g') => (&value[..value.len() - 1], 1_i64 << 30),
        _ => (value, 1),
    };
    let (digits, sign) = if let Some(value) = value.strip_prefix('-') {
        (value, -1)
    } else {
        (value.strip_prefix('+').unwrap_or(value), 1)
    };
    if digits.starts_with(['+', '-']) {
        return false;
    }
    let (digits, radix) = if let Some(value) = digits.strip_prefix("0x") {
        (value, 16)
    } else if digits.starts_with('0') {
        (digits, 8)
    } else {
        (digits, 10)
    };
    i64::from_str_radix(digits, radix)
        .ok()
        .and_then(|number| number.checked_mul(multiplier * sign))
        .and_then(|number| i32::try_from(number).ok())
        .is_some_and(|number| number != 0)
}

fn substitute(pattern: &str, name: &str, replacement: &str) -> Option<String> {
    let captured = captures(pattern, name)?;
    if pattern.contains('*') != replacement.contains('*') {
        return None;
    }
    Some(replacement.replacen('*', captured, 1))
}

fn valid_branch(name: &str) -> bool {
    // 这里只接受可新建的字面分支名；伪引用及完整对象 ID 会先被 Git 当作修订解析。
    !name.is_empty()
        && !name.starts_with('-')
        && !name.ends_with('.')
        && !name.contains("..")
        && !name.contains("@{")
        && !matches!(
            name,
            "@" | "HEAD"
                | "FETCH_HEAD"
                | "ORIG_HEAD"
                | "MERGE_HEAD"
                | "REBASE_HEAD"
                | "CHERRY_PICK_HEAD"
                | "REVERT_HEAD"
                | "AUTO_MERGE"
                | "BISECT_HEAD"
        )
        && !(matches!(name.len(), 40 | 64) && name.bytes().all(|b| b.is_ascii_hexdigit()))
        && !name.chars().any(|ch| {
            ch.is_control() || matches!(ch, ' ' | '~' | '^' | ':' | '?' | '*' | '[' | '\\')
        })
        && name
            .split('/')
            .all(|part| !part.is_empty() && !part.starts_with('.') && !part.ends_with(".lock"))
}
