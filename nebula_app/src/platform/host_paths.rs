//! 来宾路径 → 宿主盘符路径的折叠（MSYS/WSL/Cygwin）。
//!
//! 平台分叉集中在本目录（`check_platform_cfg` 豁免区）：单字母根目录
//! 仅在 Windows 宿主上才是盘符挂载。

/// 挂载形态的 POSIX cwd → 宿主盘路径（纯字符串变换，不验证存在）。
///
/// 支持三种形态：`/mnt/<盘>/…`（WSL automount）、`/cygdrive/<盘>/…`
/// （Cygwin 默认挂载）与 `/<盘>/…`（MSYS/Git Bash 的 `$PWD`，仅 Windows——
/// 非 Windows 宿主上单字母根目录是真实目录，不能折）。无集成 shell
/// （fish 等）经 OSC 7 按 `$PWD` 原样上报这些形态；折回宿主盘后本地文件树、
/// Git 视图与左侧分组读的是同一份身份。非挂载形态返回 `None`。
pub fn mounted_host_path(cwd: &str) -> Option<std::path::PathBuf> {
    // MSYS/Git Bash 的 `$PWD`（`/d/…`）：单字母根目录仅在 Windows 宿主上才是
    // 盘符挂载，其他平台是真实目录，不折。
    #[cfg(windows)]
    let msys = cwd.strip_prefix('/');
    #[cfg(not(windows))]
    let msys: Option<&str> = None;
    let suffix = cwd.strip_prefix("/mnt/").or_else(|| cwd.strip_prefix("/cygdrive/")).or(msys)?;
    let bytes = suffix.as_bytes();
    if let Some(&drive) = bytes.first()
        && drive.is_ascii_alphabetic()
        && (bytes.len() == 1 || bytes[1] == b'/')
    {
        // 跳过盘符与其后的分隔符：`d/src` → `D:\src`，不是 `D:\\src`。
        let rest = if bytes.len() == 1 { "" } else { &suffix[2..] };
        return Some(std::path::PathBuf::from(format!(
            "{}:\\{}",
            drive.to_ascii_uppercase() as char,
            rest.replace('/', "\\")
        )));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::mounted_host_path;

    /// 挂载形态折叠：三种前缀 → 宿主盘路径；非挂载/非单字母盘形态原样
    /// （`None`）。这是本地 pane「OSC 7 cwd → 宿主目录」与左侧分组共用的
    /// 唯一变换，无集成 shell（fish 等）上报的 `$PWD` 由此两侧同源。
    #[test]
    fn mounted_cwds_fold_to_host_drive_paths() {
        assert_eq!(
            mounted_host_path("/mnt/d/gopath/src/pebrel"),
            Some(std::path::PathBuf::from(r"D:\gopath\src\pebrel"))
        );
        assert_eq!(
            mounted_host_path("/cygdrive/c/Users/guest"),
            Some(std::path::PathBuf::from(r"C:\Users\guest"))
        );
        assert_eq!(mounted_host_path("/mnt/d"), Some(std::path::PathBuf::from(r"D:\")));
        // MSYS $PWD 单字母盘根仅在 Windows 宿主上是盘符挂载；其他平台 /d
        // 是真实目录，不折（运行时判定不占 platform cfg 预算）。
        if std::env::consts::OS == "windows" {
            assert_eq!(
                mounted_host_path("/d/gopath/src"),
                Some(std::path::PathBuf::from(r"D:\gopath\src")),
                "MSYS $PWD 单字母盘根"
            );
        } else {
            assert_eq!(mounted_host_path("/d/gopath/src"), None, "/d 是真实目录");
        }
        // 非挂载形态：来宾自有路径与多字母根目录不是盘符。
        assert_eq!(mounted_host_path("/home/user/proj"), None);
        assert_eq!(mounted_host_path("/tmp"), None);
    }
}
