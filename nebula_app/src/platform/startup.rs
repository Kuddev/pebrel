#[cfg(target_os = "macos")]
#[path = "portable.rs"]
mod portable;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Launch {
    Installed,
    Portable,
    Quit,
}

/// Resolve storage before migration, logging, cached paths or worker startup.
pub(crate) fn prepare_data(options: &crate::cli::Options) -> std::io::Result<Launch> {
    #[cfg(target_os = "macos")]
    {
        portable::prepare(
            options.subcommands.is_none() && !options.daemon,
            options.config_file.is_some(),
        )
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = options;
        Ok(Launch::Installed)
    }
}

/// Surface a pre-logger failure for a GUI launch; the caller also returns it on stderr.
pub(crate) fn report_error(error: &dyn std::fmt::Display, gui_launch: bool) {
    #[cfg(windows)]
    crate::panic::report_startup_error(error, gui_launch);
    #[cfg(target_os = "macos")]
    if gui_launch {
        portable::report_error(error);
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = (error, gui_launch);
}

pub fn prepare_gui() {
    #[cfg(target_os = "macos")]
    {
        crate::macos::locale::set_locale_environment();
        crate::macos::disable_autofill();
        if std::env::current_dir().ok().as_deref() == Some(std::path::Path::new("/")) {
            if let Some(home) = home::home_dir() {
                if let Err(error) = std::env::set_current_dir(home) {
                    eprintln!("Could not use the home directory: {error}");
                }
            }
        }
    }
    super::notifications::prepare();
}
