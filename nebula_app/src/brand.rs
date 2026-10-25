pub const NAME: &str = "Pebrel";
pub const WINDOWS_APP_ID: &str = "com.pebrel.terminal";
pub const LINUX_APP_ID: &str = "io.github.kuddev.pebrel";
pub const LINUX_PREVIEW_APP_ID: &str = "io.github.kuddev.pebrel.preview";
pub const DESCRIPTION: &str = "Pebrel — a GPU-accelerated terminal emulator";

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    #[test]
    fn display_name_and_cli_use_pebrel() {
        let mut command = crate::cli::Options::command();
        assert_eq!(command.get_name(), super::NAME);
        assert_eq!(command.get_bin_name(), Some("pebrel"));
        assert!(command.render_version().starts_with(super::NAME));
        assert!(command.render_long_help().to_string().contains(super::DESCRIPTION));
        assert_eq!(crate::config::window::Identity::default().title, super::NAME);
        assert_eq!(crate::config::window::DEFAULT_CLASS, "Pebrel");
    }

    #[test]
    fn linux_desktop_files_match_native_window_identity() {
        for (id, entry) in [
            (
                super::LINUX_APP_ID,
                include_str!("../../packaging/linux/io.github.kuddev.pebrel.desktop"),
            ),
            (
                super::LINUX_PREVIEW_APP_ID,
                include_str!("../../packaging/linux/io.github.kuddev.pebrel.preview.desktop"),
            ),
        ] {
            assert!(entry.lines().any(|line| line == format!("StartupWMClass={id}")));
            assert!(entry.lines().any(|line| line == format!("Icon={id}")));
        }
    }
}
