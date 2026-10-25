use super::*;
use crate::theme_library::{MAX_DOCUMENT_BYTES, ThemeDocument, ThemeLibraryStore};
use clap::{Args, Subcommand};
use std::io::{Read, Write};
use std::path::PathBuf;

#[derive(Args, Debug)]
pub(crate) struct Options {
    #[clap(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    /// Validate a ZIP theme and its resource hashes without installing it.
    Check { path: PathBuf },
    /// Export a native theme JSON with its local background image as a ZIP.
    Pack {
        theme: PathBuf,
        #[clap(long)]
        output: PathBuf,
        #[clap(long)]
        author: String,
        /// Include a preview image for community browsing.
        #[clap(long)]
        preview: Option<PathBuf>,
        #[clap(long)]
        github: Option<String>,
        #[clap(long)]
        license: String,
        #[clap(long, default_value = "1.0.0")]
        version: String,
    },
    /// Install a verified ZIP into the local theme library without applying it.
    Import { path: PathBuf },
    /// Print the authoritative package byte limits.
    Limits,
}

pub(crate) fn run(options: Options) -> i32 {
    let result = execute(options.command);
    let ok = result.is_ok();
    let output = match result {
        Ok(value) => serde_json::json!({"ok": true, "result": value}),
        Err(error) => serde_json::json!({"ok": false, "error": error.to_string()}),
    };
    let mut stdout = std::io::stdout().lock();
    if serde_json::to_writer_pretty(&mut stdout, &output).is_err()
        || stdout.write_all(b"\n").and_then(|()| stdout.flush()).is_err()
    {
        return 1;
    }
    i32::from(!ok)
}

fn execute(command: Command) -> Result<serde_json::Value> {
    match command {
        Command::Limits => Ok(serde_json::json!({
            "archive_bytes": MAX_ARCHIVE_BYTES,
            "unpacked_bytes": MAX_UNPACKED_BYTES,
            "video_bytes": MAX_VIDEO_BYTES,
            "total_video_bytes": MAX_VIDEO_BYTES,
            "entries": MAX_ENTRIES,
        })),
        Command::Check { path } => {
            let checked = CheckedPackage::open(&path)?;
            Ok(
                serde_json::json!({"manifest": checked.manifest, "installed": false, "code_executed": false}),
            )
        },
        Command::Import { path } => {
            let document = CheckedPackage::open(&path)?.install(&ThemeLibraryStore::default())?;
            Ok(serde_json::json!({"id": document.id(), "name": document.name(), "applied": false}))
        },
        Command::Pack { theme, output, author, github, license, version, preview } => {
            let mut input = std::fs::File::open(&theme)?;
            let mut bytes = Vec::new();
            Read::by_ref(&mut input).take(MAX_DOCUMENT_BYTES as u64 + 1).read_to_end(&mut bytes)?;
            let document = ThemeDocument::from_json_bytes(&bytes)
                .map_err(|error| PackageError(error.to_string()))?;
            let mut definition =
                document.definition().map_err(|error| PackageError(error.to_string()))?;
            if let Some(path) =
                definition.effects.background_image.as_ref().filter(|path| !path.is_empty())
            {
                if !std::path::Path::new(path).is_absolute() {
                    definition.effects.background_image = Some(
                        theme
                            .parent()
                            .unwrap_or(std::path::Path::new("."))
                            .join(path)
                            .to_string_lossy()
                            .into_owned(),
                    );
                }
            }
            let document = document
                .with_definition(&definition)
                .map_err(|error| PackageError(error.to_string()))?;
            export_package(
                &document,
                Author { name: author, github },
                version,
                license,
                &output,
                preview.as_deref(),
            )?;
            Ok(serde_json::json!({"path": output, "name": document.name()}))
        },
    }
}
