//! Presentation-only grouping of the already filtered, ordered host snapshot.

use std::collections::{HashMap, HashSet};

use crate::ssh_profiles::SshProfiles;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum LibraryRow {
    Group { name: String, count: usize, collapsed: bool },
    Host { destination: String, index: usize },
}

/// Groups follow their first host; host order within a group stays authoritative.
/// Search reveals matches without changing the user's saved in-view fold state.
pub(super) fn library_rows(
    hosts: Vec<String>,
    profiles: &SshProfiles,
    collapsed_groups: &HashSet<String>,
    searching: bool,
) -> Vec<LibraryRow> {
    let mut groups: Vec<(String, Vec<LibraryRow>)> = Vec::new();
    let mut indices = HashMap::new();
    for (index, destination) in hosts.into_iter().enumerate() {
        let name = profiles.organization(&destination).group.clone();
        let group_index = *indices.entry(name.clone()).or_insert_with(|| {
            groups.push((name, Vec::new()));
            groups.len() - 1
        });
        groups[group_index].1.push(LibraryRow::Host { destination, index });
    }
    // Preserve the compact flat list when no host has been assigned a group.
    if groups.len() == 1 && groups[0].0.is_empty() {
        return groups.pop().unwrap().1;
    }
    let mut rows = Vec::new();
    for (name, hosts) in groups {
        let collapsed = !searching && collapsed_groups.contains(&name);
        rows.push(LibraryRow::Group { name, count: hosts.len(), collapsed });
        if !collapsed {
            rows.extend(hosts);
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_keep_headers_counts_and_existing_host_order() {
        let mut profiles = SshProfiles::default();
        for (host, group) in [("a", "work"), ("b", "home"), ("c", "work")] {
            profiles.upsert(profiles.for_destination(host));
            profiles
                .set_organization(
                    host,
                    crate::ssh_profiles::HostOrganization {
                        group: group.into(),
                        ..Default::default()
                    },
                )
                .unwrap();
        }
        let hosts = || ["a", "b", "c", "unassigned"].map(str::to_owned).to_vec();
        let collapsed = HashSet::from(["work".into(), "".into()]);
        let rows = library_rows(hosts(), &profiles, &collapsed, false);
        assert_eq!(
            rows,
            vec![
                LibraryRow::Group { name: "work".into(), count: 2, collapsed: true },
                LibraryRow::Group { name: "home".into(), count: 1, collapsed: false },
                LibraryRow::Host { destination: "b".into(), index: 1 },
                LibraryRow::Group { name: "".into(), count: 1, collapsed: true },
            ]
        );
        let revealed = library_rows(hosts(), &profiles, &collapsed, true);
        let destinations: Vec<_> = revealed
            .iter()
            .filter_map(|row| match row {
                LibraryRow::Host { destination, .. } => Some(destination.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(destinations, ["a", "c", "b", "unassigned"]);
        assert_eq!(library_rows(hosts(), &profiles, &collapsed, false), rows);
        assert!(library_rows(Vec::new(), &profiles, &collapsed, false).is_empty());
    }

    #[test]
    fn ungrouped_only_stays_flat_even_with_stale_fold_state() {
        assert_eq!(
            library_rows(
                vec!["a".into()],
                &SshProfiles::default(),
                &HashSet::from(["".into()]),
                false
            ),
            vec![LibraryRow::Host { destination: "a".into(), index: 0 }]
        );
    }
}
