//! 每用户登录项：launchd 与 XDG autostart 都在下次登录时启动同一主程序。
use std::io;
use std::path::{Path, PathBuf};

pub(super) fn entry_path() -> io::Result<PathBuf> {
    let home = crate::platform::dirs::home_dir()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Home directory unavailable"))?;
    if cfg!(target_os = "macos") {
        Ok(home.join("Library/LaunchAgents/io.github.kuddev.pebrel.plist"))
    } else {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| home.join(".config"));
        Ok(config.join("autostart/pebrel.desktop"))
    }
}

pub(super) fn set_enabled(enabled: bool) -> io::Result<()> {
    let path = entry_path()?;
    if !enabled {
        return match std::fs::remove_file(path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            result => result,
        };
    }
    let executable = std::env::current_exe()?;
    let content = if cfg!(target_os = "macos") {
        launch_agent(&executable)?
    } else {
        let executable = linux_launcher(
            &executable,
            std::env::var_os("APPIMAGE").as_deref().map(Path::new),
            std::env::var_os("APPDIR").as_deref().map(Path::new),
        );
        desktop_entry(&executable)?
    };
    crate::atomic_file::write(&path, content.as_bytes())
}

fn linux_launcher(executable: &Path, image: Option<&Path>, appdir: Option<&Path>) -> PathBuf {
    // AppImage 的挂载路径只对本次进程有效；登录项必须指向原始包。
    if let (Some(image), Some(appdir)) = (image, appdir)
        && image.is_absolute()
        && image.is_file()
        && std::fs::canonicalize(executable)
            .ok()
            .zip(std::fs::canonicalize(appdir).ok())
            .is_some_and(|(executable, root)| executable.starts_with(root))
    {
        return image.to_owned();
    }
    // 解包便携版仍经 AppRun 设置动态库路径，不能直启 usr/bin 内的裸二进制。
    if let Some(bin) = executable.parent()
        && bin.file_name().is_some_and(|name| name == "bin")
        && let Some(usr) = bin.parent()
        && usr.file_name().is_some_and(|name| name == "usr")
        && let Some(root) = usr.parent()
        && root.join("AppRun").is_file()
    {
        return root.join("AppRun");
    }
    executable.to_owned()
}

fn executable_text(executable: &Path) -> io::Result<&str> {
    executable
        .to_str()
        .filter(|text| executable.is_absolute() && !text.chars().any(char::is_control))
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "Invalid startup executable path")
        })
}

fn launch_agent(executable: &Path) -> io::Result<String> {
    let executable = executable_text(executable)?
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>io.github.kuddev.pebrel</string>
<key>ProgramArguments</key><array><string>{executable}</string><string>--gpui</string></array>
<key>RunAtLoad</key><true/>
<key>LimitLoadToSessionType</key><string>Aqua</string>
</dict></plist>
"#
    ))
}

fn desktop_entry(executable: &Path) -> io::Result<String> {
    // Exec 不是 shell。先按引号参数规则引用，再转义桌面文件字符串层；% 也需字面化。
    let mut argument = String::new();
    for ch in executable_text(executable)?.chars() {
        if matches!(ch, '\\' | '"' | '$' | '`') {
            argument.push('\\');
        }
        argument.push(ch);
        if ch == '%' {
            argument.push('%');
        }
    }
    let argument = argument.replace('\\', "\\\\");
    Ok(format!(
        "[Desktop Entry]\nType=Application\nName=Pebrel\nExec=\"{argument}\" --gpui\nTerminal=false\nX-GNOME-Autostart-enabled=true\n"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_login_uses_a_persistent_launcher() {
        let root = tempfile::tempdir().unwrap();
        let appdir = root.path().join("mount");
        std::fs::create_dir_all(appdir.join("usr/bin")).unwrap();
        let executable = appdir.join("usr/bin/pebrel");
        let image = root.path().join("Pebrel.AppImage");
        std::fs::write(&executable, b"fixture").unwrap();
        std::fs::write(&image, b"fixture").unwrap();
        assert_eq!(linux_launcher(&executable, Some(&image), Some(&appdir)), image);
        assert_eq!(
            linux_launcher(&executable, Some(&image), Some(Path::new("missing"))),
            executable
        );
        std::fs::write(appdir.join("AppRun"), b"fixture").unwrap();
        assert_eq!(linux_launcher(&executable, None, None), appdir.join("AppRun"));
    }

    #[test]
    fn login_entries_preserve_literal_paths_and_reject_injected_lines() {
        let root = tempfile::tempdir().unwrap();
        let executable = root.path().join("a & b <apps> $tools `x` %f").join("pebrel");
        let plist = launch_agent(&executable).unwrap();
        assert!(plist.contains("a &amp; b &lt;apps&gt; $tools `x` %f"));
        assert!(plist.contains("<string>--gpui</string>"));
        let desktop = desktop_entry(&executable).unwrap();
        assert!(desktop.contains("%%f"));
        assert!(desktop.contains("\\\\$tools"));
        assert!(desktop.contains("\\\\`x\\\\`"));
        for path in [Path::new("relative"), Path::new("/tmp/invalid\nExec=bad")] {
            assert!(launch_agent(path).is_err());
            assert!(desktop_entry(path).is_err());
        }
    }
}
