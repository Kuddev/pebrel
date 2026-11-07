use std::path::{Path, PathBuf};

use crate::cli::{ConfigCommand, ConfigInitOptions, ConfigOptions, ConfigTypesOptions};
use crate::config;
use crate::config::source::{self, ConfigFormat};
use crate::config::template;

pub fn run(options: ConfigOptions) -> i32 {
    match options.command {
        ConfigCommand::Check(options) => run_check(options.config_file),
        ConfigCommand::Init(options) => run_init(options),
        ConfigCommand::Types(options) => run_types(options),
    }
}

fn run_check(explicit_path: Option<PathBuf>) -> i32 {
    let source = match source::discover(explicit_path) {
        Ok(Some(source)) => source,
        Ok(None) => {
            eprintln!("No Nebula configuration file was found.");
            return 1;
        },
        Err(error) => {
            eprintln!("Configuration discovery failed: {error}");
            return 1;
        },
    };

    match config::load_source(&source) {
        Ok(loaded) => {
            println!("Configuration is valid: {}", source.primary_path.display());
            for path in &loaded.config.config_paths {
                if path != &source.primary_path {
                    println!("  module: {}", path.display());
                }
            }
            0
        },
        Err(error) => {
            eprintln!("{}: {error}", source.primary_path.display());
            1
        },
    }
}

fn run_init(options: ConfigInitOptions) -> i32 {
    let path = match options.config_file.map(Ok).unwrap_or_else(source::default_lua_path) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("Unable to choose a configuration path: {error}");
            return 1;
        },
    };
    if !matches!(source::source_for_path(path.clone(), true), Ok(source) if source.format == ConfigFormat::Lua)
    {
        eprintln!("Lua configuration path must end in .lua: {}", path.display());
        return 1;
    }

    let requested_language = options.language.as_str();
    let language = match template::resolve_template_language(
        Some(requested_language),
        None,
        template::system_locale().as_deref(),
    ) {
        Ok(language) => language,
        Err(error) => {
            eprintln!("Unable to select template language: {error}");
            return 1;
        },
    };

    match template::write_template(&path, language, options.force) {
        Ok(written) => {
            println!("Wrote Lua configuration: {}", written.path.display());
            if let Some(backup) = written.backup {
                println!("Backed up previous configuration: {}", backup.display());
            }
            // Editor types are a convenience; the configuration itself is complete.
            if let Some(dir) = written.path.parent() {
                report_editor_types(dir);
            }
            0
        },
        Err(error) => {
            eprintln!("Unable to write Lua configuration: {error}");
            1
        },
    }
}

fn run_types(options: ConfigTypesOptions) -> i32 {
    let path = match options.config_file.map(Ok).unwrap_or_else(source::default_lua_path) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("Unable to choose a configuration path: {error}");
            return 1;
        },
    };
    let dir = path.parent().filter(|dir| !dir.as_os_str().is_empty()).unwrap_or(Path::new("."));
    if report_editor_types(dir) { 0 } else { 1 }
}

fn report_editor_types(dir: &Path) -> bool {
    match template::write_editor_types(dir) {
        Ok(written) => {
            println!("Wrote Luau editor types: {}", written.types_path.display());
            let luaurc = written.luaurc_path.display();
            match written.luaurc {
                template::LuaurcStatus::Created => println!("Created {luaurc}"),
                template::LuaurcStatus::AlreadyMapped => {},
                template::LuaurcStatus::NeedsAlias => println!(
                    "Add {} to {luaurc} so editors resolve require '@pebrel'",
                    template::luaurc_alias_hint()
                ),
                template::LuaurcStatus::SkippedHome => println!(
                    "Skipped .luaurc in the home directory; add {} to a .luaurc beside the \
                     configuration to enable editor types",
                    template::luaurc_alias_hint()
                ),
            }
            true
        },
        Err(error) => {
            eprintln!("Unable to write Luau editor types: {error}");
            false
        },
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::cli::{ConfigInitOptions, ConfigLanguage};

    #[test]
    fn check_accepts_valid_lua_and_rejects_invalid_lua() {
        let temp = tempfile::tempdir().unwrap();
        let valid = temp.path().join("valid.lua");
        let invalid = temp.path().join("invalid.lua");
        fs::write(&valid, "return { scrolling = { history = 321 } }").unwrap();
        fs::write(&invalid, "return { windwo = {} }").unwrap();
        assert_eq!(run_check(Some(valid)), 0);
        assert_eq!(run_check(Some(invalid)), 1);
    }

    #[test]
    fn init_writes_requested_localized_template() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("nebula.lua");
        let options = ConfigInitOptions {
            config_file: Some(path.clone()),
            language: ConfigLanguage::ZhCn,
            force: false,
        };
        assert_eq!(run_init(options), 0);
        assert_eq!(fs::read_to_string(path).unwrap(), template::CHINESE_TEMPLATE);
    }
}
