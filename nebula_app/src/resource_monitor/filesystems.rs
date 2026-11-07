//! Overview selection preserves the full mount inventory for read-only inspection.
use super::Disk;

/// Select the system mount first without assuming a Windows drive letter.
pub(crate) fn primary(disks: &[Disk]) -> Option<&Disk> {
    disks.iter().find(|disk| disk.system)
}

/// Select meaningful data mounts, excluding system infrastructure and proven duplicate mounts.
pub(crate) fn other(disks: &[Disk]) -> Vec<&Disk> {
    let mut selected = Vec::new();
    let system = primary(disks);
    for disk in disks {
        if disk.system || infrastructure(disk) || disk.total == 0 {
            continue;
        }
        // Deduplication requires both volume identity and filesystem-relative root.
        let duplicate = system.into_iter().chain(selected.iter().copied()).any(|old| {
            disk.source.as_ref().is_some_and(|source| !source.is_empty())
                && disk.source == old.source
                && disk.root.is_some()
                && disk.root == old.root
                && disk.filesystem == old.filesystem
        });
        if !duplicate {
            selected.push(disk);
        }
    }
    selected
}

/// Identify mounts dedicated to kernel interfaces, temporary storage or OS boot infrastructure.
fn infrastructure(disk: &Disk) -> bool {
    let mount = disk.mount.trim_end_matches('/');
    let beneath =
        |base: &str| mount == base || mount.strip_prefix(base).is_some_and(|s| s.starts_with('/'));
    matches!(
        disk.filesystem.as_deref(),
        Some(
            "tmpfs"
                | "devtmpfs"
                | "proc"
                | "sysfs"
                | "cgroup"
                | "cgroup2"
                | "devpts"
                | "mqueue"
                | "hugetlbfs"
                | "debugfs"
                | "tracefs"
                | "securityfs"
                | "configfs"
                | "fusectl"
                | "efivarfs"
                | "autofs"
        )
    ) || [
        "/boot",
        "/proc",
        "/sys",
        "/dev",
        "/System/Volumes",
        "/snap",
        "/var/lib/snapd/snap",
        "/var/lib/docker",
        "/var/lib/containers",
    ]
    .iter()
    .any(|base| beneath(base))
}

/// Determine the desktop host's system volume using its own OS environment.
pub(super) fn native_system_mount(mount: &std::path::Path) -> bool {
    if cfg!(target_os = "windows") {
        let drive = std::env::var("SystemDrive").ok();
        drive.is_some_and(|drive| {
            mount.to_string_lossy().trim_end_matches(['\\', '/']).eq_ignore_ascii_case(&drive)
        })
    } else {
        mount == std::path::Path::new("/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Construct mount metadata independently of desktop or SSH inspection.
    fn disk(mount: &str, filesystem: &str, source: &str, root: Option<&str>, system: bool) -> Disk {
        Disk {
            mount: mount.into(),
            total: 100,
            available: 50,
            filesystem: Some(filesystem.into()),
            source: Some(source.into()),
            root: root.map(str::to_owned),
            system,
        }
    }

    /// Root remains visible even when a container uses overlay; EFI and pseudo mounts stay out of overview.
    #[test]
    fn overview_preserves_system_and_data_mounts() {
        let disks = vec![
            disk("/", "overlay", "overlay", Some("/"), true),
            disk("/boot/efi", "vfat", "8:1", Some("/"), false),
            disk("/sys/firmware/efi/efivars", "efivarfs", "0:2", Some("/"), false),
            disk("/data", "ext4", "8:3", Some("/"), false),
        ];
        assert_eq!(primary(&disks).unwrap().mount, "/");
        assert_eq!(
            other(&disks).iter().map(|disk| disk.mount.as_str()).collect::<Vec<_>>(),
            ["/data"]
        );
        assert_eq!(disks.len(), 4);
    }

    /// Equal capacities and shared devices cannot erase distinct subvolumes or uncertain identities.
    #[test]
    fn duplicate_selection_requires_matching_volume_and_root() {
        let disks = vec![
            disk("/", "btrfs", "8:2", Some("/@"), true),
            disk("/mirror", "btrfs", "8:2", Some("/@"), false),
            disk("/home", "btrfs", "8:2", Some("/@home"), false),
            disk("/unknown", "btrfs", "8:2", None, false),
        ];
        assert_eq!(
            other(&disks).iter().map(|disk| disk.mount.as_str()).collect::<Vec<_>>(),
            ["/home", "/unknown"]
        );
    }

    /// Target-provided system identity supports Windows installations outside the C drive.
    #[test]
    fn system_selection_uses_target_metadata() {
        let disks =
            vec![disk("C:", "NTFS", "C:", None, false), disk("D:", "NTFS", "D:", None, true)];
        assert_eq!(primary(&disks).unwrap().mount, "D:");
        assert_eq!(other(&disks)[0].mount, "C:");
    }
}
