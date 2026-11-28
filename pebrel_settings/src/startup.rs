//! 启动与恢复的共享准入规则；各启动入口不自行解释原始配置。

use crate::RuntimeSettings;

impl RuntimeSettings {
    pub fn command_output_enabled(&self) -> bool {
        self.restore_session && self.save_command_output
    }

    /// 启动时再检查目录：保存后可能被删除，各启动入口应使用同一有效性规则。
    pub fn startup_directory_path(&self) -> Option<std::path::PathBuf> {
        let directory = self.startup_directory.as_deref()?.trim();
        if directory.is_empty() {
            return None;
        }
        let path = std::path::PathBuf::from(directory);
        path.is_dir().then_some(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RawSettings;

    #[test]
    fn recent_output_is_saved_only_when_both_recovery_switches_are_enabled() {
        for (raw, enabled) in [
            ("", true),
            ("save_command_output=0", false),
            ("restore_session=0", false),
            ("restore_session=0\nsave_command_output=1", false),
            ("restore_session=1\nsave_command_output=1", true),
        ] {
            let settings = RuntimeSettings::from_raw(&RawSettings::from_text(raw));
            assert_eq!(settings.command_output_enabled(), enabled, "{raw}");
        }
    }
}
