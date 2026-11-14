//! Saved launch identities may retain a previous build's temporary bootstrap path.

use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf, Prefix};
use std::sync::OnceLock;

use sha2::{Digest, Sha256};

use crate::tty::Shell;

const SCRIPT_DIRECTORY: &str = "pebrel-shell";

fn generation(version: &str, source: &[u8]) -> String {
    let mut name = String::with_capacity(version.len() + 65);
    name.push_str(version);
    name.push('-');
    for byte in Sha256::digest(source).iter() {
        write!(&mut name, "{byte:02x}").unwrap();
    }
    name
}

pub(super) fn versioned_path(base: &Path) -> PathBuf {
    // 同版本测试构建也可能修改脚本；指纹只计算一次，不随每次开标签重复散列。
    static GENERATION: OnceLock<String> = OnceLock::new();
    let generation = GENERATION
        .get_or_init(|| generation(env!("CARGO_PKG_VERSION"), super::NEBULA_PROMPT_PS1.as_bytes()));
    base.join(SCRIPT_DIRECTORY).join(generation).join("pebrel_prompt.ps1")
}

fn versioned_generation(name: &str) -> bool {
    let Some((version, digest)) = name.rsplit_once('-') else { return false };
    version.as_bytes().first().is_some_and(u8::is_ascii_digit)
        && version.bytes().all(|byte| byte.is_ascii_alphanumeric() || b".-+".contains(&byte))
        && digest.len() == 64
        && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(super) fn refresh(shell: &Shell) -> Option<Shell> {
    let script = prepared_script(shell)?;
    let mut directories = vec![std::env::temp_dir()];
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        directories.push(PathBuf::from(local).join("Temp"));
    }
    if let Some(config) = std::env::var_os("PEBREL_CONFIG_DIR") {
        directories.push(PathBuf::from(config).join("shell-integration"));
    }
    let args = original_args(shell, &script, &directories)?;
    // 只更新本次启动参数，不覆盖旧路径；新进程必须加载当前构建生成的集成。
    let current = super::nebula_prompt_script_path()?;
    Some(Shell::new(shell.program().to_owned(), super::powershell_integration_args(args, &current)))
}

fn prepared_script(shell: &Shell) -> Option<PathBuf> {
    let name = Path::new(shell.program()).file_name()?.to_str()?;
    if !["powershell", "powershell.exe", "pwsh", "pwsh.exe"]
        .iter()
        .any(|candidate| name.eq_ignore_ascii_case(candidate))
    {
        return None;
    }
    let args = shell.args();
    let split = args.len().checked_sub(5)?;
    if !args[split..split + 4]
        .iter()
        .zip(["-NoExit", "-ExecutionPolicy", "Bypass", "-Command"])
        .all(|(actual, expected)| actual.eq_ignore_ascii_case(expected))
        || !args[..split].iter().all(|arg| {
            ["-NoLogo", "-NoProfile", "-NoExit", "-STA", "-MTA"]
                .iter()
                .any(|flag| arg.eq_ignore_ascii_case(flag))
        })
    {
        return None;
    }
    let literal = args.last()?.strip_prefix(". '")?.strip_suffix('\'')?;
    if literal.contains(['\'', '\r', '\n']) {
        return None;
    }
    let path = PathBuf::from(literal);
    // 不探测 UNC 路径，也不把同名的任意用户脚本当作托管文件。
    let local = matches!(path.components().next(), Some(Component::Prefix(prefix))
        if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)));
    (local
        && path.is_absolute()
        && path.file_name()?.to_str()?.eq_ignore_ascii_case("pebrel_prompt.ps1"))
    .then_some(path)
}

fn original_args(shell: &Shell, script: &Path, directories: &[PathBuf]) -> Option<Vec<String>> {
    let mut parent = script.parent()?;
    if parent
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case(SCRIPT_DIRECTORY))
    {
        if !versioned_generation(parent.file_name()?.to_str()?) {
            return None;
        }
        // 旧版本目录可以已经被清理，只核实它所属的托管根，不要求旧脚本仍存在。
        parent = parent.parent()?.parent()?;
    }
    let parent = parent.canonicalize().ok()?;
    let owned = directories.iter().any(|directory| {
        directory.is_absolute()
            && directory.canonicalize().is_ok_and(|directory| directory == parent)
    });
    owned.then(|| shell.args()[..shell.args().len() - 5].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Directories(PathBuf);

    impl Directories {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "pebrel-bootstrap-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            ));
            std::fs::create_dir(&root).unwrap();
            for name in ["old", "current"] {
                std::fs::create_dir(root.join(name)).unwrap();
            }
            Self(root)
        }
    }

    impl Drop for Directories {
        fn drop(&mut self) {
            for name in ["old", "current"] {
                let _ = std::fs::remove_dir(self.0.join(name));
            }
            let _ = std::fs::remove_dir(&self.0);
        }
    }

    fn prepared(program: &str, directory: &Path) -> Shell {
        Shell::new(
            program.to_owned(),
            super::super::powershell_integration_args(
                vec!["-NoLogo".into(), "-NoProfile".into()],
                &directory.join("pebrel_prompt.ps1"),
            ),
        )
    }

    #[test]
    fn frozen_managed_launch_can_use_the_current_bootstrap_without_changing_user_flags() {
        let directories = Directories::new();
        let old = directories.0.join("old");
        let current = directories.0.join("current");
        let shell = prepared("powershell.exe", &old);
        let path = prepared_script(&shell).unwrap();
        let args = original_args(&shell, &path, &[old.clone()]).unwrap();
        assert_eq!(args, ["-NoLogo", "-NoProfile"]);
        let refreshed =
            super::super::powershell_integration_args(args, &current.join("pebrel_prompt.ps1"));
        assert_eq!(&refreshed[..2], &shell.args()[..2]);
        assert!(!refreshed.last().unwrap().contains(&old.to_string_lossy().to_string()));
        assert!(refreshed.last().unwrap().contains(&current.to_string_lossy().to_string()));
        assert!(!old.join("pebrel_prompt.ps1").exists(), "old cache is not written");
    }

    #[test]
    fn custom_commands_shells_and_same_named_files_outside_managed_roots_are_untouched() {
        let directories = Directories::new();
        let managed = directories.0.join("old");
        let custom = directories.0.join("current");
        for program in ["cmd.exe", "wsl.exe", "bash.exe"] {
            assert!(prepared_script(&prepared(program, &managed)).is_none());
        }
        let shell = prepared("pwsh.exe", &custom);
        let script = prepared_script(&shell).unwrap();
        assert!(original_args(&shell, &script, &[managed.clone()]).is_none());
        for command in [
            "Write-Output test",
            ". '\\\\server\\share\\pebrel_prompt.ps1'",
            ". 'C:\\Temp\\custom.ps1'",
            ". 'C:\\Temp\\pebrel_prompt.ps1'; Write-Output test",
        ] {
            let mut args = prepared("powershell.exe", &managed).args().to_vec();
            *args.last_mut().unwrap() = command.into();
            assert!(prepared_script(&Shell::new("powershell.exe".into(), args)).is_none());
        }
        let mut args = shell.args().to_vec();
        args.insert(0, "-File".into());
        assert!(prepared_script(&Shell::new("powershell.exe".into(), args)).is_none());
    }

    #[test]
    fn script_generations_change_with_version_or_contents_but_are_stable_otherwise() {
        let first = generation("2.1.2", b"current script");
        assert_eq!(first, generation("2.1.2", b"current script"));
        assert_ne!(first, generation("2.1.3", b"current script"));
        assert_ne!(first, generation("2.1.2", b"next candidate"));
        assert!(versioned_generation(&first));
        assert!(!versioned_generation("custom-script"));
        assert!(!versioned_generation("2.1.2-abcdef"));
    }

    #[test]
    fn a_missing_old_generation_is_refreshed_without_accepting_arbitrary_subdirectories() {
        let directories = Directories::new();
        let root = directories.0.join("old");
        let old = root.join(SCRIPT_DIRECTORY).join(generation("2.1.0", b"old script"));
        let shell = prepared("powershell.exe", &old);
        let script = prepared_script(&shell).unwrap();
        assert!(!script.exists());
        assert_eq!(
            original_args(&shell, &script, &[root.clone()]).unwrap(),
            ["-NoLogo", "-NoProfile"]
        );
        let unrelated = prepared("powershell.exe", &root.join(SCRIPT_DIRECTORY).join("custom"));
        let script = prepared_script(&unrelated).unwrap();
        assert!(original_args(&unrelated, &script, &[root]).is_none());
    }

    #[test]
    fn restored_command_line_loads_current_generated_script_and_leaves_flat_file_unchanged() {
        let old = std::env::temp_dir().join("pebrel_prompt.ps1");
        let before = std::fs::read(&old).ok();
        let shell = prepared("powershell.exe", old.parent().unwrap());
        let mut options = crate::tty::Options::default();
        options.shell = Some(shell.clone());
        let command = super::super::cmdline(&options);
        let current = super::super::nebula_prompt_script_path().unwrap();
        assert!(command.contains(SCRIPT_DIRECTORY));
        assert!(command.contains(env!("CARGO_PKG_VERSION")));
        assert!(!command.contains(&format!(". '{}'", old.display())));
        assert_eq!(std::fs::read(&old).ok(), before);
        let bytes = std::fs::read(&current).unwrap();
        assert_eq!(&bytes[..3], &[0xef, 0xbb, 0xbf]);
        assert_eq!(&bytes[3..], super::super::NEBULA_PROMPT_PS1.as_bytes());
        assert!(String::from_utf8_lossy(&bytes).contains("pebrel_editor_ready"));
        assert_eq!(options.shell.as_ref().unwrap().args(), shell.args());
    }
}
