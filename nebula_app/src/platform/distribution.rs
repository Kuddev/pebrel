//! 安装渠道只控制应用自更新，不改变用户设置、插件或终端热路径。
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Distribution {
    Direct,
    Scoop,
    Msix,
    Unrecognized,
}

impl Distribution {
    pub(crate) fn externally_managed(self) -> bool {
        self != Self::Direct
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Direct => "Stable",
            Self::Scoop => "Scoop",
            Self::Msix => "MSIX",
            Self::Unrecognized => "—",
        }
    }

    pub(crate) fn message(self) -> Option<crate::i18n::Message> {
        use crate::i18n::Message;
        match self {
            Self::Direct => None,
            Self::Scoop => Some(Message::UpdateManagedScoop),
            Self::Msix => Some(Message::UpdateManagedMsix),
            Self::Unrecognized => Some(Message::UpdateManagedUnknown),
        }
    }
}

/// 在 GUI 启动锁检查时初始化；随后设置页仅访问这一个不可变值。
pub(crate) fn current() -> Distribution {
    static CHANNEL: OnceLock<Distribution> = OnceLock::new();
    *CHANNEL.get_or_init(detect)
}

pub(crate) fn require_direct_update() -> Result<(), String> {
    match current() {
        Distribution::Direct => Ok(()),
        Distribution::Scoop => Err("Update this installation with: scoop update pebrel".into()),
        Distribution::Msix => Err(
            "Update this MSIX package through its distribution source (Microsoft Store for Store installations).".into(),
        ),
        Distribution::Unrecognized => {
            Err("Installation ownership could not be established; use its original distribution source.".into())
        },
    }
}

fn detect() -> Distribution {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{
            APPMODEL_ERROR_NO_PACKAGE, ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS,
        };
        use windows_sys::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;
        let mut length = 0;
        // 只查询长度即可确认真实包身份，不按 WindowsApps 路径猜测，也不分配名称缓冲。
        // SAFETY: length 指向可写 UINT32；零长度查询允许空输出指针。
        let status = unsafe { GetCurrentPackageFullName(&mut length, std::ptr::null_mut()) };
        match status {
            ERROR_INSUFFICIENT_BUFFER | ERROR_SUCCESS => return Distribution::Msix,
            APPMODEL_ERROR_NO_PACKAGE => {},
            error => {
                log::warn!("Package identity query failed: {error}");
                return Distribution::Unrecognized;
            },
        }
        return std::env::current_exe()
            .and_then(std::fs::canonicalize)
            .map(|executable| from_marker(&executable))
            .unwrap_or(Distribution::Unrecognized);
    }
    #[cfg(not(windows))]
    Distribution::Direct
}

#[cfg(any(windows, test))]
fn from_marker(executable: &std::path::Path) -> Distribution {
    use std::io::Read as _;
    let Some(directory) = executable.parent() else { return Distribution::Unrecognized };
    let marker = directory.join("pebrel-distribution");
    let file = match std::fs::File::open(marker) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Distribution::Direct,
        Err(_) => return Distribution::Unrecognized,
    };
    let mut bytes = Vec::with_capacity(33);
    if file.take(33).read_to_end(&mut bytes).is_err() {
        return Distribution::Unrecognized;
    }
    parse_marker(&bytes)
}

#[cfg(any(windows, test))]
fn parse_marker(bytes: &[u8]) -> Distribution {
    if bytes.len() <= 32 && bytes.trim_ascii() == b"scoop" {
        Distribution::Scoop
    } else {
        // 未知/损坏标记不得悄悄切回普通安装器；标记只收窄更新能力。
        Distribution::Unrecognized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_explicit_scoop_marker_is_recognized() {
        for value in [b"scoop".as_slice(), b"scoop\r\n", b" scoop "] {
            assert_eq!(parse_marker(value), Distribution::Scoop);
        }
        for value in [b"".as_slice(), b"installer", b"msix", b"Scoop", &[b' '; 33]] {
            assert_eq!(parse_marker(value), Distribution::Unrecognized);
        }
    }

    #[test]
    fn ownership_is_local_to_installation_not_directory_name() {
        let root = tempfile::tempdir().unwrap();
        let executable = root.path().join("pebrel.exe");
        assert_eq!(from_marker(&executable), Distribution::Direct);
        std::fs::write(root.path().join("pebrel-distribution"), b"scoop\n").unwrap();
        assert_eq!(from_marker(&executable), Distribution::Scoop);
        std::fs::write(root.path().join("pebrel-distribution"), b"other").unwrap();
        assert_eq!(from_marker(&executable), Distribution::Unrecognized);
    }

    #[test]
    fn external_channels_never_own_setup_updates() {
        assert!(!Distribution::Direct.externally_managed());
        assert!(Distribution::Direct.message().is_none());
        for channel in [Distribution::Scoop, Distribution::Msix, Distribution::Unrecognized] {
            assert!(channel.externally_managed());
            assert!(channel.message().is_some());
        }
    }

    #[cfg(windows)]
    #[test]
    fn managed_process_stops_before_update_io() {
        const PROBE: &str = "PEBREL_DISTRIBUTION_TEST_CHILD";
        if let Some(marker) = std::env::var_os(PROBE) {
            let expected =
                if marker == "scoop" { Distribution::Scoop } else { Distribution::Unrecognized };
            assert_eq!(current(), expected);
            assert!(require_direct_update().is_err());
            assert!(crate::update_check::check_now().is_err());
            // 旧壳仍校验安装归属；更新下载模块只在 GPUI 产品中存在。
            #[cfg(feature = "gpui-shell")]
            {
                let asset = crate::update_check::UpdateAsset {
                    version: "0.0.0".into(),
                    name: String::new(),
                    download_url: String::new(),
                    size: None,
                    sha256: None,
                };
                assert!(crate::update_download::begin(&asset).is_err());
                assert!(crate::update_download::handoff::prepare(&asset).is_err());
                assert!(crate::update_download::handoff::schedule(&asset).is_err());
                assert!(!crate::update_download::handoff::apply_scheduled());
                assert!(!crate::update_download::handoff::installation_in_progress().unwrap());
                crate::update_download::hydrate();
                assert!(crate::update_download::cached_asset().is_none());
            }
            let executable = std::env::current_exe().unwrap();
            assert!(!executable.parent().unwrap().join(".pebrel-update.nebula-lock").exists());
            return;
        }
        let executable = std::env::current_exe().unwrap();
        for marker in ["scoop", "unknown"] {
            // 同卷硬链接避免复制大型测试二进制；子进程拥有独立 OnceLock 和更新状态。
            let root = tempfile::tempdir_in(executable.parent().unwrap()).unwrap();
            let child = root.path().join("distribution-test.exe");
            std::fs::hard_link(&executable, &child).unwrap();
            std::fs::write(root.path().join("pebrel-distribution"), marker).unwrap();
            let output = std::process::Command::new(child)
                .args([
                    "--exact",
                    "platform::distribution::tests::managed_process_stops_before_update_io",
                ])
                .env(PROBE, marker)
                .output()
                .unwrap();
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stdout));
        }
    }
}
