//! 可选的服务器：`~/.ssh/config` 别名 + Pebrel 已保存的主机。
//!
//! 顺序与去重复用侧栏的单一权威 [`crate::ssh_profiles::merge_host_sources`]，读取
//! 侧栏同一份 `saved_hosts`/`pinned_hosts`/`hidden_hosts` 设置：命令行列出的候选、
//! 界面主机选择器与 SSH 侧栏三处顺序一致，隐藏的主机也同样不出现。

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HostCandidate {
    /// 交给 `ssh` 的名字：config 别名或已保存的目的地。
    pub(crate) name: String,
    /// 用户在 SSH 设置里起的名称；没有时为空。
    pub(crate) label: String,
}

pub(crate) fn candidates() -> Vec<HostCandidate> {
    let raw = nebula_settings::RawSettings::load();
    let list = |key: &str| -> Vec<String> {
        raw.value(key)
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .map(str::to_owned)
            .collect()
    };
    let profiles = crate::ssh_profiles::SshProfiles::load(
        &crate::display::nebula_data_dir().join("ssh_profiles.json"),
    )
    .unwrap_or_default();
    let configured = crate::ssh::ssh_config_hosts();
    merged(
        &list("saved_hosts"),
        &list("pinned_hosts"),
        &list("hidden_hosts"),
        &configured,
        &profiles,
    )
}

fn merged(
    saved: &[String],
    pinned: &[String],
    hidden: &[String],
    configured: &[String],
    profiles: &crate::ssh_profiles::SshProfiles,
) -> Vec<HostCandidate> {
    let labels = profiles.labels();
    crate::ssh_profiles::merge_host_sources(
        saved,
        pinned,
        hidden,
        configured,
        profiles.destinations(),
    )
    .into_iter()
    .map(|name| {
        let label = labels.get(&name).cloned().unwrap_or_default();
        HostCandidate { name, label }
    })
    .collect()
}

/// 按名字选主机：先精确匹配别名/目的地，再精确匹配用户起的名称，最后忽略大小写
/// 匹配名称。都不中时原样返回，让 `ssh` 自己解析（例如临时输入的 `user@host`）。
pub(crate) fn resolve(name: &str, candidates: &[HostCandidate]) -> String {
    candidates
        .iter()
        .find(|host| host.name == name)
        .or_else(|| candidates.iter().find(|host| host.label == name))
        .or_else(|| {
            candidates
                .iter()
                .find(|host| !host.label.is_empty() && host.label.eq_ignore_ascii_case(name))
        })
        .map_or_else(|| name.to_owned(), |host| host.name.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(name: &str, label: &str) -> HostCandidate {
        HostCandidate { name: name.to_owned(), label: label.to_owned() }
    }

    #[test]
    fn resolve_prefers_destinations_then_labels_and_keeps_unknown_names() {
        let hosts =
            [host("ipxair-cc", ""), host("root@10.0.0.2", "Build Box"), host("Build Box", "")];
        assert_eq!(resolve("ipxair-cc", &hosts), "ipxair-cc");
        // An alias spelled exactly like another host's label wins over that label.
        assert_eq!(resolve("Build Box", &hosts), "Build Box");
        assert_eq!(resolve("build box", &hosts), "root@10.0.0.2");
        assert_eq!(resolve("user@other", &hosts), "user@other");
    }

    #[test]
    fn merged_hosts_follow_the_sidebar_order_and_hide_rules() {
        let profiles = crate::ssh_profiles::SshProfiles::default();
        let saved = vec!["recent".to_owned(), "alias-b".to_owned()];
        let pinned = vec!["alias-b".to_owned()];
        let hidden = vec!["alias-c".to_owned()];
        let configured = vec!["alias-a".to_owned(), "alias-b".to_owned(), "alias-c".to_owned()];
        let names: Vec<_> = merged(&saved, &pinned, &hidden, &configured, &profiles)
            .into_iter()
            .map(|host| host.name)
            .collect();
        assert_eq!(names, ["alias-b", "recent", "alias-a"]);
    }
}
