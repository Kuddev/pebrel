//! Bounded, static theme import/export adapters.
//!
//! These adapters consume text as data.  No Lua, shell, include, import or
//! external script is ever evaluated.  The native Pebrel format is delegated to
//! [`ThemeDocument`]; the other formats map only their documented color fields.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde_json::{Map, Value, json};

use super::document::{
    DocumentError, MAX_DOCUMENT_BYTES, ThemeDocument, format_hex_rgb, parse_hex_rgb, seed_document,
};

pub const MAX_INPUT_BYTES: usize = 256 * 1024;
/// Keep one bounded import from expanding into an unbounded number of native
/// documents and their 256-color palettes.
pub const MAX_IMPORT_CANDIDATES: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeFormat {
    Pebrel,
    WindowsTerminal,
    Kitty,
    Ghostty,
    WezTerm,
    Alacritty,
    Iterm2,
}

impl ThemeFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Pebrel => "pebrel-theme.json",
            Self::WindowsTerminal => "json",
            Self::Kitty | Self::Ghostty => "conf",
            Self::WezTerm | Self::Alacritty => "toml",
            Self::Iterm2 => "itermcolors",
        }
    }
}

impl fmt::Display for ThemeFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Pebrel => "Pebrel",
            Self::WindowsTerminal => "Windows Terminal",
            Self::Kitty => "Kitty",
            Self::Ghostty => "Ghostty",
            Self::WezTerm => "WezTerm",
            Self::Alacritty => "Alacritty",
            Self::Iterm2 => "iTerm2",
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportCandidate {
    pub document: ThemeDocument,
    pub format: ThemeFormat,
    pub filename: String,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportDiagnostic {
    pub filename: String,
    pub format: Option<ThemeFormat>,
    pub message: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Inspection {
    pub candidates: Vec<ImportCandidate>,
    pub diagnostics: Vec<ImportDiagnostic>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportArtifact {
    pub text: String,
    pub extension: &'static str,
    pub summary: String,
    pub losses: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormatError {
    TooLarge { bytes: usize },
    Unsupported(String),
    Parse(String),
    Invalid(String),
    Document(DocumentError),
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { bytes } => {
                write!(f, "input is {bytes} bytes; limit is {MAX_INPUT_BYTES}")
            },
            Self::Unsupported(message) | Self::Parse(message) | Self::Invalid(message) => {
                f.write_str(message)
            },
            Self::Document(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for FormatError {}

impl From<DocumentError> for FormatError {
    fn from(error: DocumentError) -> Self {
        Self::Document(error)
    }
}

#[derive(Default)]
struct SourceTheme {
    name: String,
    background: Option<String>,
    foreground: Option<String>,
    cursor: Option<String>,
    cursor_text: Option<String>,
    selection_background: Option<String>,
    selection_foreground: Option<String>,
    palette: BTreeMap<u8, String>,
    indexed: BTreeMap<u16, String>,
    extensions: Map<String, Value>,
    warnings: Vec<String>,
}

pub fn inspect(text: &str, filename: impl Into<String>) -> Inspection {
    let filename = filename.into();
    if text.len() > MAX_INPUT_BYTES {
        return Inspection {
            diagnostics: vec![ImportDiagnostic {
                filename,
                format: None,
                message: format!("input exceeds the 256 KiB limit ({} bytes)", text.len()),
            }],
            ..Inspection::default()
        };
    }
    let lower_filename = filename.to_ascii_lowercase();
    if lower_filename.ends_with(".yaml") || lower_filename.ends_with(".yml") {
        return Inspection {
            diagnostics: vec![ImportDiagnostic {
                filename,
                format: None,
                message: "YAML theme files are not supported; use Pebrel JSON or a static terminal color format".to_owned(),
            }],
            ..Inspection::default()
        };
    }
    let format = detect_format(text, &filename);
    match format {
        Some(ThemeFormat::Iterm2) => Inspection {
            diagnostics: vec![ImportDiagnostic {
                filename,
                format: Some(ThemeFormat::Iterm2),
                message: "iTerm2 XML import is unavailable without an XML parser dependency".to_owned(),
            }],
            ..Inspection::default()
        },
        Some(format) => inspect_format(text, &filename, format),
        None => Inspection {
            diagnostics: vec![ImportDiagnostic {
                filename,
                format: None,
                message: "unsupported theme format; use Pebrel JSON, Windows Terminal JSON, Kitty, Ghostty, WezTerm or Alacritty".to_owned(),
            }],
            ..Inspection::default()
        },
    }
}

pub fn export(
    document: &ThemeDocument,
    format: ThemeFormat,
) -> Result<ExportArtifact, FormatError> {
    let name = document.name().to_owned();
    let value = document.to_value();
    let terminal = object(value.get("terminal"), "terminal")?;
    let palette = document
        .palette()
        .ok_or_else(|| FormatError::Invalid("document has no ANSI 0..15 palette".to_owned()))?;
    let indexed = document.indexed().cloned().unwrap_or_default();
    let indexed = indexed
        .into_iter()
        .map(|(key, value)| {
            let index = key
                .parse::<u16>()
                .map_err(|_| FormatError::Invalid(format!("indexed key {key} is invalid")))?;
            if !(16..=255).contains(&index) {
                return Err(FormatError::Invalid(format!("indexed key {key} must be 16..255")));
            }
            let color = value
                .as_str()
                .ok_or_else(|| FormatError::Invalid(format!("indexed {key} is not a color")))?;
            if parse_hex_rgb(color).is_none() {
                return Err(FormatError::Invalid(format!("indexed {key} is not a valid color")));
            }
            Ok((index, color.to_owned()))
        })
        .collect::<Result<Vec<_>, FormatError>>()?;
    match format {
        ThemeFormat::Pebrel => {
            let bytes = document.to_json_bytes()?;
            let text = String::from_utf8(bytes)
                .map_err(|error| FormatError::Invalid(error.to_string()))?
                + "\n";
            Ok(ExportArtifact {
                text,
                extension: format.extension(),
                summary: format!("{} will be exported as a complete Pebrel theme", name),
                losses: Vec::new(),
            })
        },
        ThemeFormat::WindowsTerminal => export_windows(document, terminal, &palette, &indexed),
        ThemeFormat::Kitty => export_kitty(document, terminal, &palette, &indexed),
        ThemeFormat::Ghostty => export_ghostty(document, terminal, &palette, &indexed),
        ThemeFormat::WezTerm => export_wezterm(document, terminal, &palette, &indexed),
        ThemeFormat::Alacritty => export_alacritty(document, terminal, &palette, &indexed),
        ThemeFormat::Iterm2 => Err(FormatError::Unsupported(
            "iTerm2 XML export is unavailable without an XML serializer dependency".to_owned(),
        )),
    }
}

fn inspect_format(text: &str, filename: &str, format: ThemeFormat) -> Inspection {
    let result = match format {
        ThemeFormat::Pebrel | ThemeFormat::WindowsTerminal => inspect_json(text, filename, format),
        ThemeFormat::Kitty | ThemeFormat::Ghostty => inspect_key_value(text, filename, format),
        ThemeFormat::WezTerm | ThemeFormat::Alacritty => inspect_toml(text, filename, format),
        ThemeFormat::Iterm2 => unreachable!(),
    };
    match result {
        Ok(candidates) => Inspection { candidates, diagnostics: Vec::new() },
        Err(error) => Inspection {
            diagnostics: vec![ImportDiagnostic {
                filename: filename.to_owned(),
                format: Some(format),
                message: error.to_string(),
            }],
            ..Inspection::default()
        },
    }
}

fn inspect_json(
    text: &str,
    filename: &str,
    format: ThemeFormat,
) -> Result<Vec<ImportCandidate>, FormatError> {
    let value: Value = serde_json::from_str(text)
        .map_err(|error| FormatError::Parse(format!("invalid JSON: {error}")))?;
    let mut output = Vec::new();
    match value {
        Value::Array(values) => {
            ensure_candidate_limit(values.len())?;
            for (index, value) in values.into_iter().enumerate() {
                let result = if value.get("terminal").is_some() {
                    import_native(value, filename)
                } else {
                    import_windows(value, filename, index)
                };
                output.push(result?);
            }
        },
        Value::Object(mut object) if object.get("themes").is_some() => {
            let themes = object
                .remove("themes")
                .ok_or_else(|| FormatError::Invalid("themes must be an array".to_owned()))?;
            let themes = themes
                .as_array()
                .ok_or_else(|| FormatError::Invalid("themes must be an array".to_owned()))?;
            ensure_candidate_limit(themes.len())?;
            for (index, value) in themes.iter().cloned().enumerate() {
                output.push(if value.get("terminal").is_some() {
                    import_native(value, filename)?
                } else {
                    import_windows(value, filename, index)?
                });
            }
        },
        Value::Object(mut object) if object.get("schemes").is_some() => {
            let schemes = object
                .remove("schemes")
                .ok_or_else(|| FormatError::Invalid("schemes must be an array".to_owned()))?;
            let schemes = schemes
                .as_array()
                .ok_or_else(|| FormatError::Invalid("schemes must be an array".to_owned()))?;
            ensure_candidate_limit(schemes.len())?;
            for (index, value) in schemes.iter().cloned().enumerate() {
                output.push(import_windows(value, filename, index)?);
            }
        },
        value if value.get("terminal").is_some() => output.push(import_native(value, filename)?),
        value => output.push(import_windows(value, filename, 0)?),
    }
    if output.is_empty() {
        return Err(FormatError::Invalid("JSON contains no themes".to_owned()));
    }
    if format == ThemeFormat::Pebrel
        && output.iter().any(|candidate| candidate.format != ThemeFormat::Pebrel)
    {
        return Err(FormatError::Invalid("JSON is not a Pebrel theme document".to_owned()));
    }
    Ok(output)
}

fn import_native(value: Value, filename: &str) -> Result<ImportCandidate, FormatError> {
    let document = ThemeDocument::from_value(value)?;
    Ok(ImportCandidate {
        document,
        format: ThemeFormat::Pebrel,
        filename: filename.to_owned(),
        warnings: Vec::new(),
    })
}

fn import_windows(
    value: Value,
    filename: &str,
    index: usize,
) -> Result<ImportCandidate, FormatError> {
    let object = value.as_object().ok_or_else(|| {
        FormatError::Invalid("Windows Terminal scheme must be an object".to_owned())
    })?;
    let mut source = SourceTheme {
        name: object
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("Imported theme {}", index + 1)),
        background: string_field(object, "background"),
        foreground: string_field(object, "foreground"),
        cursor: string_field(object, "cursorColor"),
        selection_background: string_field(object, "selectionBackground"),
        selection_foreground: string_field(object, "selectionForeground"),
        ..SourceTheme::default()
    };
    let names = ["black", "red", "green", "yellow", "blue", "purple", "cyan", "white"];
    for (index, name) in names.into_iter().enumerate() {
        if let Some(color) = string_field(object, name) {
            source.palette.insert(index as u8, color);
        }
        let bright = format!("bright{}", title_case(name));
        if let Some(color) = string_field(object, &bright) {
            source.palette.insert((index + 8) as u8, color);
        }
    }
    let known: BTreeSet<&str> = [
        "name",
        "foreground",
        "background",
        "cursorColor",
        "selectionBackground",
        "selectionForeground",
        "black",
        "red",
        "green",
        "yellow",
        "blue",
        "purple",
        "cyan",
        "white",
        "brightBlack",
        "brightRed",
        "brightGreen",
        "brightYellow",
        "brightBlue",
        "brightPurple",
        "brightCyan",
        "brightWhite",
    ]
    .into_iter()
    .collect();
    preserve_unknown(object, &known, &mut source.extensions);
    finish_source(source, ThemeFormat::WindowsTerminal, filename)
}

fn ensure_candidate_limit(count: usize) -> Result<(), FormatError> {
    if count > MAX_IMPORT_CANDIDATES {
        return Err(FormatError::Invalid(format!(
            "theme file contains {count} candidates; limit is {MAX_IMPORT_CANDIDATES}"
        )));
    }
    Ok(())
}

fn inspect_key_value(
    text: &str,
    filename: &str,
    expected: ThemeFormat,
) -> Result<Vec<ImportCandidate>, FormatError> {
    let entries = parse_key_values(text)?;
    let ghostty = expected == ThemeFormat::Ghostty
        || entries.iter().any(|(key, _)| key == "palette" || key.contains('-'));
    let format = if ghostty { ThemeFormat::Ghostty } else { ThemeFormat::Kitty };
    let mut source = SourceTheme { name: filename_base(filename), ..SourceTheme::default() };
    let mut unknown = Vec::new();
    for (key, value) in entries {
        match format {
            ThemeFormat::Kitty => match key.as_str() {
                "foreground" => source.foreground = Some(value),
                "background" => source.background = Some(value),
                "cursor" => source.cursor = Some(value),
                "cursor_text_color" => source.cursor_text = Some(value),
                "selection_foreground" => source.selection_foreground = Some(value),
                "selection_background" => source.selection_background = Some(value),
                key if key
                    .strip_prefix("color")
                    .and_then(|value| value.parse::<u16>().ok())
                    .is_some() =>
                {
                    let index = key
                        .strip_prefix("color")
                        .and_then(|value| value.parse::<u16>().ok())
                        .unwrap_or(256);
                    if index > 255 {
                        return Err(FormatError::Invalid(format!(
                            "Kitty color index {index} must be 0..255"
                        )));
                    }
                    if index < 16 {
                        source.palette.insert(index as u8, value);
                    } else {
                        source.indexed.insert(index, value);
                    }
                },
                "include" | "shell" | "launch" | "map" | "remote_control" => {
                    source
                        .warnings
                        .push(format!("Kitty {key} directive was retained and not executed"));
                    unknown.push(format!("{key} {value}"));
                },
                _ => unknown.push(format!("{key} {value}")),
            },
            ThemeFormat::Ghostty => match key.as_str() {
                "foreground" => source.foreground = Some(value),
                "background" => source.background = Some(value),
                "cursor-color" | "cursor" => source.cursor = Some(value),
                "cursor-text" | "cursor-text-color" => source.cursor_text = Some(value),
                "selection-background" => source.selection_background = Some(value),
                "selection-foreground" => source.selection_foreground = Some(value),
                "palette" => parse_ghostty_palette(&value, &mut source)?,
                "command" | "custom-shader" | "keybind" => {
                    source
                        .warnings
                        .push(format!("Ghostty {key} directive was retained and not executed"));
                    unknown.push(format!("{key} = {value}"));
                },
                _ => unknown.push(format!("{key} = {value}")),
            },
            _ => unreachable!(),
        }
    }
    if !unknown.is_empty() {
        source.extensions.insert(
            "unknown_lines".to_owned(),
            Value::Array(unknown.into_iter().map(Value::String).collect()),
        );
    }
    finish_source(source, format, filename).map(|candidate| vec![candidate])
}

fn parse_key_values(text: &str) -> Result<Vec<(String, String)>, FormatError> {
    let mut output = Vec::new();
    for (line_number, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = if let Some((key, value)) = line.split_once('=') {
            (key.trim(), value.trim())
        } else {
            line.split_once(char::is_whitespace)
                .map(|(key, value)| (key.trim(), value.trim()))
                .ok_or_else(|| {
                    FormatError::Parse(format!(
                        "line {} is not a static key/value entry",
                        line_number + 1
                    ))
                })?
        };
        if key.is_empty()
            || !key.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
        {
            return Err(FormatError::Parse(format!(
                "line {} contains an unsupported key",
                line_number + 1
            )));
        }
        let value =
            value.split_whitespace().next().unwrap_or_default().trim_matches('"').to_owned();
        if value.is_empty() {
            return Err(FormatError::Parse(format!("line {} has an empty value", line_number + 1)));
        }
        output.push((key.to_owned(), value));
    }
    Ok(output)
}

fn parse_ghostty_palette(value: &str, source: &mut SourceTheme) -> Result<(), FormatError> {
    let (index, color) = value.split_once('=').ok_or_else(|| {
        FormatError::Parse("Ghostty palette must be written as index=#rrggbb".to_owned())
    })?;
    let index: u16 = index
        .parse()
        .map_err(|_| FormatError::Invalid("Ghostty palette index is not an integer".to_owned()))?;
    if index > 255 {
        return Err(FormatError::Invalid(format!("Ghostty palette index {index} must be 0..255")));
    }
    if index < 16 {
        source.palette.insert(index as u8, color.to_owned());
    } else {
        source.indexed.insert(index, color.to_owned());
    }
    Ok(())
}

fn inspect_toml(
    text: &str,
    filename: &str,
    expected: ThemeFormat,
) -> Result<Vec<ImportCandidate>, FormatError> {
    let value: toml::Value = toml::from_str(text)
        .map_err(|error| FormatError::Parse(format!("invalid static TOML: {error}")))?;
    let object = value
        .as_table()
        .ok_or_else(|| FormatError::Invalid("TOML root must be a table".to_owned()))?;
    let colors = object
        .get("colors")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| FormatError::Invalid("TOML is missing [colors]".to_owned()))?;
    let alacritty = colors.contains_key("primary")
        || colors.contains_key("normal")
        || colors.contains_key("bright")
        || colors.contains_key("indexed_colors");
    let format = if alacritty { ThemeFormat::Alacritty } else { ThemeFormat::WezTerm };
    if expected != format && expected != ThemeFormat::WezTerm && expected != ThemeFormat::Alacritty
    {
        return Err(FormatError::Invalid(
            "TOML format does not match the selected adapter".to_owned(),
        ));
    }
    let mut source = SourceTheme { name: filename_base(filename), ..SourceTheme::default() };
    let known = if alacritty {
        parse_alacritty(colors, &mut source)?;
        known_alacritty()
    } else {
        parse_wezterm(colors, &mut source)?;
        known_wezterm()
    };
    if contains_dynamic_directive(text) {
        source.warnings.push(
            "dynamic/include/script-like TOML fields were retained and not executed".to_owned(),
        );
    }
    let mut extension_value = Map::new();
    preserve_unknown_toml(object, &known, &mut extension_value);
    if !extension_value.is_empty() {
        extension_value.insert("raw_source".to_owned(), Value::String(text.to_owned()));
        source.extensions.insert("raw_source".to_owned(), Value::Object(extension_value));
    }
    finish_source(source, format, filename).map(|candidate| vec![candidate])
}

fn parse_wezterm(colors: &toml::value::Table, source: &mut SourceTheme) -> Result<(), FormatError> {
    source.background = toml_string(colors, "background");
    source.foreground = toml_string(colors, "foreground");
    source.cursor_text = toml_string(colors, "cursor_fg");
    source.cursor = toml_string(colors, "cursor_bg");
    source.selection_foreground = toml_string(colors, "selection_fg");
    source.selection_background = toml_string(colors, "selection_bg");
    if let Some(values) = colors.get("ansi").and_then(toml::Value::as_array) {
        for (index, value) in values.iter().enumerate().take(8) {
            if let Some(color) = value.as_str() {
                source.palette.insert(index as u8, color.to_owned());
            }
        }
    }
    if let Some(values) = colors.get("brights").and_then(toml::Value::as_array) {
        for (index, value) in values.iter().enumerate().take(8) {
            if let Some(color) = value.as_str() {
                source.palette.insert((index + 8) as u8, color.to_owned());
            }
        }
    }
    if let Some(values) = colors.get("indexed").and_then(toml::Value::as_table) {
        for (index, value) in values {
            let index = index.parse::<u16>().map_err(|_| {
                FormatError::Invalid(format!("WezTerm indexed key {index} is invalid"))
            })?;
            source.indexed.insert(
                index,
                value
                    .as_str()
                    .ok_or_else(|| {
                        FormatError::Invalid(format!("WezTerm indexed {index} is not a color"))
                    })?
                    .to_owned(),
            );
        }
    }
    Ok(())
}

fn parse_alacritty(
    colors: &toml::value::Table,
    source: &mut SourceTheme,
) -> Result<(), FormatError> {
    let primary = colors
        .get("primary")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| FormatError::Invalid("Alacritty is missing [colors.primary]".to_owned()))?;
    source.background = toml_string(primary, "background");
    source.foreground = toml_string(primary, "foreground");
    if let Some(cursor) = colors.get("cursor").and_then(toml::Value::as_table) {
        source.cursor_text = toml_string(cursor, "text");
        source.cursor = toml_string(cursor, "cursor");
    }
    if let Some(selection) = colors.get("selection").and_then(toml::Value::as_table) {
        source.selection_foreground = toml_string(selection, "text");
        source.selection_background = toml_string(selection, "background");
    }
    for (section, offset) in [("normal", 0_u8), ("bright", 8_u8)] {
        if let Some(values) = colors.get(section).and_then(toml::Value::as_table) {
            for (index, name) in
                ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"]
                    .into_iter()
                    .enumerate()
            {
                if let Some(color) = values.get(name).and_then(toml::Value::as_str) {
                    source.palette.insert(offset + index as u8, color.to_owned());
                }
            }
        }
    }
    if let Some(values) = colors.get("indexed_colors").and_then(toml::Value::as_array) {
        for value in values {
            let object = value.as_table().ok_or_else(|| {
                FormatError::Invalid("Alacritty indexed_colors entry must be a table".to_owned())
            })?;
            let index = object.get("index").and_then(toml::Value::as_integer).ok_or_else(|| {
                FormatError::Invalid("Alacritty indexed_colors entry has no index".to_owned())
            })?;
            if !(16..=255).contains(&index) {
                return Err(FormatError::Invalid(format!(
                    "Alacritty indexed color {index} must be 16..255"
                )));
            }
            let color = object.get("color").and_then(toml::Value::as_str).ok_or_else(|| {
                FormatError::Invalid("Alacritty indexed_colors entry has no color".to_owned())
            })?;
            source.indexed.insert(index as u16, color.to_owned());
        }
    }
    Ok(())
}

fn known_wezterm() -> BTreeSet<String> {
    [
        "background",
        "foreground",
        "cursor_fg",
        "cursor_bg",
        "selection_fg",
        "selection_bg",
        "ansi",
        "brights",
        "indexed",
    ]
    .into_iter()
    .map(|key| format!("colors.{key}"))
    .collect()
}

fn known_alacritty() -> BTreeSet<String> {
    let mut known = BTreeSet::new();
    for key in ["background", "foreground"] {
        known.insert(format!("colors.primary.{key}"));
    }
    for key in ["text", "cursor"] {
        known.insert(format!("colors.cursor.{key}"));
    }
    for key in ["text", "background"] {
        known.insert(format!("colors.selection.{key}"));
    }
    for section in ["normal", "bright"] {
        for key in ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"] {
            known.insert(format!("colors.{section}.{key}"));
        }
    }
    known.insert("colors.indexed_colors".to_owned());
    known
}

fn preserve_unknown_toml(
    table: &toml::value::Table,
    known: &BTreeSet<String>,
    target: &mut Map<String, Value>,
) {
    preserve_unknown_toml_at(table, "", known, target);
}

fn preserve_unknown_toml_at(
    table: &toml::value::Table,
    prefix: &str,
    known: &BTreeSet<String>,
    target: &mut Map<String, Value>,
) {
    for (key, value) in table {
        let full = if prefix.is_empty() { key.to_owned() } else { format!("{prefix}.{key}") };
        if known.contains(&full) {
            continue;
        }
        if known.iter().any(|path| path.starts_with(&format!("{full}."))) {
            if let Some(nested) = value.as_table() {
                let mut child = Map::new();
                preserve_unknown_toml_at(nested, &full, known, &mut child);
                if !child.is_empty() {
                    target.insert(key.clone(), Value::Object(child));
                }
            }
            continue;
        }
        target.insert(key.clone(), toml_to_json(value));
    }
}

fn toml_to_json(value: &toml::Value) -> Value {
    match value {
        toml::Value::String(value) => Value::String(value.clone()),
        toml::Value::Integer(value) => json!(value),
        toml::Value::Float(value) => json!(value),
        toml::Value::Boolean(value) => json!(value),
        toml::Value::Datetime(value) => Value::String(value.to_string()),
        toml::Value::Array(values) => Value::Array(values.iter().map(toml_to_json).collect()),
        toml::Value::Table(values) => Value::Object(
            values.iter().map(|(key, value)| (key.clone(), toml_to_json(value))).collect(),
        ),
    }
}

fn finish_source(
    mut source: SourceTheme,
    format: ThemeFormat,
    filename: &str,
) -> Result<ImportCandidate, FormatError> {
    let background = source
        .background
        .take()
        .ok_or_else(|| FormatError::Invalid("theme must provide background".to_owned()))?;
    let foreground = source
        .foreground
        .take()
        .ok_or_else(|| FormatError::Invalid("theme must provide foreground".to_owned()))?;
    let background = parse_hex_rgb(&fixed_color(&background, "background")?)
        .ok_or_else(|| FormatError::Invalid("background was not normalized to RGB".to_owned()))?;
    let foreground = parse_hex_rgb(&fixed_color(&foreground, "foreground")?)
        .ok_or_else(|| FormatError::Invalid("foreground was not normalized to RGB".to_owned()))?;
    let palette_count = source.palette.len();
    let mut document = seed_document(&source.name, background, foreground)?;
    let mut value = document.to_value();
    let terminal = value
        .get_mut("terminal")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| FormatError::Invalid("seed has no terminal object".to_owned()))?;
    for key in
        ["cursor", "cursor_text", "cursor_stroke", "selection_background", "selection_foreground"]
    {
        terminal.insert(key.to_owned(), Value::Null);
    }
    set_optional_color(
        terminal,
        "cursor",
        source.cursor.take(),
        &mut source.warnings,
        &mut source.extensions,
    )?;
    set_optional_color(
        terminal,
        "cursor_text",
        source.cursor_text.take(),
        &mut source.warnings,
        &mut source.extensions,
    )?;
    set_optional_color(
        terminal,
        "selection_background",
        source.selection_background.take(),
        &mut source.warnings,
        &mut source.extensions,
    )?;
    set_optional_color(
        terminal,
        "selection_foreground",
        source.selection_foreground.take(),
        &mut source.warnings,
        &mut source.extensions,
    )?;
    let palette = terminal
        .get_mut("palette")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| FormatError::Invalid("seed has no ANSI palette".to_owned()))?;
    for (index, color) in source.palette {
        palette[index as usize] = Value::String(fixed_color(&color, &format!("ANSI {index}"))?);
    }
    if palette_count < 16 {
        source.warnings.push("source omitted part of ANSI 0..15; defaults were filled".to_owned());
    }
    let indexed = terminal
        .get_mut("indexed")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| FormatError::Invalid("seed has no indexed table".to_owned()))?;
    for (index, color) in source.indexed {
        if !(16..=255).contains(&index) {
            return Err(FormatError::Invalid(format!("indexed color {index} must be 16..255")));
        }
        indexed.insert(
            index.to_string(),
            Value::String(fixed_color(&color, &format!("indexed {index}"))?),
        );
    }
    if !source.extensions.is_empty() {
        value["extensions"][format.to_string().to_ascii_lowercase()] =
            Value::Object(source.extensions);
    }
    value["ui"]["derive"] = Value::Bool(true);
    value["metadata"]["source_format"] = Value::String(format.to_string());
    document = ThemeDocument::from_value(value)?;
    source.warnings.push(
        "source contains terminal colors only; interface colors are derived by Pebrel".to_owned(),
    );
    Ok(ImportCandidate {
        document,
        format,
        filename: filename.to_owned(),
        warnings: dedupe(source.warnings),
    })
}

fn set_optional_color(
    terminal: &mut Map<String, Value>,
    key: &str,
    value: Option<String>,
    warnings: &mut Vec<String>,
    extensions: &mut Map<String, Value>,
) -> Result<(), FormatError> {
    let Some(value) = value else { return Ok(()) };
    if is_dynamic_color(&value) {
        warnings.push(format!("{key} uses dynamic color semantics and was not converted"));
        extensions.insert(format!("dynamic_{key}"), Value::String(value));
        return Ok(());
    }
    terminal.insert(key.to_owned(), Value::String(fixed_color(&value, key)?));
    Ok(())
}

fn fixed_color(value: &str, field: &str) -> Result<String, FormatError> {
    let color = parse_hex_rgb(value)
        .ok_or_else(|| FormatError::Invalid(format!("{field} must be a #RGB or #RRGGBB color")))?;
    Ok(format_hex_rgb(color))
}

fn is_dynamic_color(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "none" | "default" | "reverse" | "cell-foreground" | "cell-background"
    )
}

fn contains_dynamic_directive(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line.trim().to_ascii_lowercase();
        ["include", "import", "shell", "command", "lua", "function"]
            .iter()
            .any(|word| line.starts_with(word) || line.contains(&format!(" {word}")))
    })
}

fn preserve_unknown(
    object: &Map<String, Value>,
    known: &BTreeSet<&str>,
    target: &mut Map<String, Value>,
) {
    for (key, value) in object {
        if !known.contains(key.as_str()) {
            target.insert(key.clone(), value.clone());
        }
    }
}

fn object<'a>(value: Option<&'a Value>, key: &str) -> Result<&'a Map<String, Value>, FormatError> {
    value
        .and_then(Value::as_object)
        .ok_or_else(|| FormatError::Invalid(format!("missing {key} object")))
}

fn string_field(object: &Map<String, Value>, key: &str) -> Option<String> {
    object.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn toml_string(object: &toml::value::Table, key: &str) -> Option<String> {
    object.get(key).and_then(toml::Value::as_str).map(str::to_owned)
}

fn filename_base(filename: &str) -> String {
    filename
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(filename)
        .rsplit_once('.')
        .map_or(filename, |(base, _)| base)
        .to_owned()
}

fn title_case(value: &str) -> String {
    let mut characters = value.chars();
    characters.next().map_or_else(String::new, |first| {
        first.to_uppercase().collect::<String>() + characters.as_str()
    })
}

fn detect_format(text: &str, filename: &str) -> Option<ThemeFormat> {
    let lower = filename.to_ascii_lowercase();
    let trimmed = text.trim_start_matches('\u{feff}').trim();
    if lower.ends_with(".itermcolors")
        || trimmed.starts_with("<?xml")
        || trimmed.starts_with("<plist")
    {
        return Some(ThemeFormat::Iterm2);
    }
    if lower.ends_with(".pebrel-theme.json") {
        return Some(ThemeFormat::Pebrel);
    }
    if lower.ends_with(".toml") || trimmed.starts_with("[colors") {
        return Some(if trimmed.contains("[colors.primary]") {
            ThemeFormat::Alacritty
        } else {
            ThemeFormat::WezTerm
        });
    }
    if lower.ends_with(".json") || trimmed.starts_with('{') || trimmed.starts_with('[') {
        return Some(ThemeFormat::WindowsTerminal);
    }
    if lower.ends_with(".ghostty")
        || text.lines().any(|line| {
            line.trim_start().starts_with("palette =")
                || line.trim_start().starts_with("cursor-color")
        })
    {
        return Some(ThemeFormat::Ghostty);
    }
    if lower.ends_with(".conf")
        || text.lines().any(|line| {
            line.trim_start().starts_with("color0 ")
                || line.trim_start().starts_with("cursor_text_color ")
        })
    {
        return Some(ThemeFormat::Kitty);
    }
    None
}

fn dedupe(values: Vec<String>) -> Vec<String> {
    let mut output = Vec::new();
    for value in values {
        if !output.contains(&value) {
            output.push(value);
        }
    }
    output
}

fn export_windows(
    document: &ThemeDocument,
    terminal: &Map<String, Value>,
    palette: &[[u8; 3]; 16],
    indexed: &[(u16, String)],
) -> Result<ExportArtifact, FormatError> {
    let mut scheme = Map::new();
    scheme.insert("name".to_owned(), Value::String(document.name().to_owned()));
    scheme.insert("background".to_owned(), terminal_color(terminal, "background")?);
    scheme.insert("foreground".to_owned(), terminal_color(terminal, "foreground")?);
    if let Some(value) = terminal.get("cursor").and_then(Value::as_str) {
        scheme.insert("cursorColor".to_owned(), Value::String(value.to_owned()));
    }
    if let Some(value) = terminal.get("selection_background").and_then(Value::as_str) {
        scheme.insert("selectionBackground".to_owned(), Value::String(value.to_owned()));
    }
    for (index, name) in ["black", "red", "green", "yellow", "blue", "purple", "cyan", "white"]
        .into_iter()
        .enumerate()
    {
        scheme.insert(name.to_owned(), Value::String(format_hex_rgb(palette[index])));
        scheme.insert(
            format!("bright{}", title_case(name)),
            Value::String(format_hex_rgb(palette[index + 8])),
        );
    }
    let mut losses = external_losses("Windows Terminal");
    losses.push(
        "cursor text and selection foreground are not representable in a Windows Terminal scheme"
            .to_owned(),
    );
    if !indexed.is_empty() {
        losses.push(format!(
            "{} indexed colors 16..255 are not representable and were omitted",
            indexed.len()
        ));
    }
    artifact_json(
        json!({ "schemes": [Value::Object(scheme)] }),
        ThemeFormat::WindowsTerminal,
        document.name(),
        losses,
    )
}

fn export_kitty(
    document: &ThemeDocument,
    terminal: &Map<String, Value>,
    palette: &[[u8; 3]; 16],
    indexed: &[(u16, String)],
) -> Result<ExportArtifact, FormatError> {
    let mut lines = vec![
        format!("# {}", document.name()),
        format!("foreground {}", terminal_color_string(terminal, "foreground")?),
        format!("background {}", terminal_color_string(terminal, "background")?),
    ];
    for (key, output) in [
        ("cursor", "cursor"),
        ("cursor_text", "cursor_text_color"),
        ("selection_foreground", "selection_foreground"),
        ("selection_background", "selection_background"),
    ] {
        if let Some(value) = terminal.get(key).and_then(Value::as_str) {
            lines.push(format!("{output} {value}"));
        }
    }
    for (index, color) in palette.iter().enumerate() {
        lines.push(format!("color{index} {}", format_hex_rgb(*color)));
    }
    for (index, color) in indexed {
        lines.push(format!("color{index} {color}"));
    }
    artifact_text(
        lines.join("\n") + "\n",
        ThemeFormat::Kitty,
        document.name(),
        external_losses("Kitty"),
    )
}

fn export_ghostty(
    document: &ThemeDocument,
    terminal: &Map<String, Value>,
    palette: &[[u8; 3]; 16],
    indexed: &[(u16, String)],
) -> Result<ExportArtifact, FormatError> {
    let mut lines = vec![
        format!("# {}", document.name()),
        format!("foreground = {}", terminal_color_string(terminal, "foreground")?),
        format!("background = {}", terminal_color_string(terminal, "background")?),
    ];
    for (key, output) in [
        ("cursor", "cursor-color"),
        ("cursor_text", "cursor-text"),
        ("selection_background", "selection-background"),
        ("selection_foreground", "selection-foreground"),
    ] {
        if let Some(value) = terminal.get(key).and_then(Value::as_str) {
            lines.push(format!("{output} = {value}"));
        }
    }
    for (index, color) in palette.iter().enumerate() {
        lines.push(format!("palette = {index}={}", format_hex_rgb(*color)));
    }
    for (index, color) in indexed {
        lines.push(format!("palette = {index}={color}"));
    }
    artifact_text(
        lines.join("\n") + "\n",
        ThemeFormat::Ghostty,
        document.name(),
        external_losses("Ghostty"),
    )
}

fn export_wezterm(
    document: &ThemeDocument,
    terminal: &Map<String, Value>,
    palette: &[[u8; 3]; 16],
    indexed: &[(u16, String)],
) -> Result<ExportArtifact, FormatError> {
    let mut lines = vec![
        "[colors]".to_owned(),
        format!("background = \"{}\"", terminal_color_string(terminal, "background")?),
        format!("foreground = \"{}\"", terminal_color_string(terminal, "foreground")?),
    ];
    for (key, output) in [
        ("cursor_text", "cursor_fg"),
        ("cursor", "cursor_bg"),
        ("selection_foreground", "selection_fg"),
        ("selection_background", "selection_bg"),
    ] {
        if let Some(value) = terminal.get(key).and_then(Value::as_str) {
            lines.push(format!("{output} = \"{value}\""));
        }
    }
    lines.push("ansi = [".to_owned());
    lines.extend(palette[..8].iter().map(|color| format!("  \"{}\",", format_hex_rgb(*color))));
    lines.push("]".to_owned());
    lines.push("brights = [".to_owned());
    lines.extend(palette[8..].iter().map(|color| format!("  \"{}\",", format_hex_rgb(*color))));
    lines.push("]".to_owned());
    if !indexed.is_empty() {
        lines.push("".to_owned());
        lines.push("[colors.indexed]".to_owned());
        lines.extend(indexed.iter().map(|(index, color)| format!("{index} = \"{color}\"")));
    }
    artifact_text(
        lines.join("\n") + "\n",
        ThemeFormat::WezTerm,
        document.name(),
        external_losses("WezTerm"),
    )
}

fn export_alacritty(
    document: &ThemeDocument,
    terminal: &Map<String, Value>,
    palette: &[[u8; 3]; 16],
    indexed: &[(u16, String)],
) -> Result<ExportArtifact, FormatError> {
    let mut lines = vec!["[colors]".to_owned()];
    if !indexed.is_empty() {
        lines.push("indexed_colors = [".to_owned());
        lines.extend(
            indexed
                .iter()
                .map(|(index, color)| format!("  {{ index = {index}, color = \"{color}\" }},")),
        );
        lines.push("]".to_owned());
    }
    lines.extend([
        "".to_owned(),
        "[colors.primary]".to_owned(),
        format!("background = \"{}\"", terminal_color_string(terminal, "background")?),
        format!("foreground = \"{}\"", terminal_color_string(terminal, "foreground")?),
    ]);
    if terminal.get("cursor").and_then(Value::as_str).is_some()
        || terminal.get("cursor_text").and_then(Value::as_str).is_some()
    {
        lines.extend(["".to_owned(), "[colors.cursor]".to_owned()]);
        if let Some(value) = terminal.get("cursor_text").and_then(Value::as_str) {
            lines.push(format!("text = \"{value}\""));
        }
        if let Some(value) = terminal.get("cursor").and_then(Value::as_str) {
            lines.push(format!("cursor = \"{value}\""));
        }
    }
    if terminal.get("selection_background").and_then(Value::as_str).is_some()
        || terminal.get("selection_foreground").and_then(Value::as_str).is_some()
    {
        lines.extend(["".to_owned(), "[colors.selection]".to_owned()]);
        if let Some(value) = terminal.get("selection_foreground").and_then(Value::as_str) {
            lines.push(format!("text = \"{value}\""));
        }
        if let Some(value) = terminal.get("selection_background").and_then(Value::as_str) {
            lines.push(format!("background = \"{value}\""));
        }
    }
    lines.extend(["".to_owned(), "[colors.normal]".to_owned()]);
    for (index, name) in ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"]
        .into_iter()
        .enumerate()
    {
        lines.push(format!("{name} = \"{}\"", format_hex_rgb(palette[index])));
    }
    lines.extend(["".to_owned(), "[colors.bright]".to_owned()]);
    for (index, name) in ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"]
        .into_iter()
        .enumerate()
    {
        lines.push(format!("{name} = \"{}\"", format_hex_rgb(palette[index + 8])));
    }
    artifact_text(
        lines.join("\n") + "\n",
        ThemeFormat::Alacritty,
        document.name(),
        external_losses("Alacritty"),
    )
}

fn terminal_color(terminal: &Map<String, Value>, key: &str) -> Result<Value, FormatError> {
    Ok(Value::String(terminal_color_string(terminal, key)?))
}

fn terminal_color_string(terminal: &Map<String, Value>, key: &str) -> Result<String, FormatError> {
    terminal
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| FormatError::Invalid(format!("terminal.{key} is missing")))
}

fn artifact_json(
    value: Value,
    format: ThemeFormat,
    name: &str,
    losses: Vec<String>,
) -> Result<ExportArtifact, FormatError> {
    let text = serde_json::to_string_pretty(&value)
        .map_err(|error| FormatError::Invalid(error.to_string()))?
        + "\n";
    artifact_text(text, format, name, losses)
}

fn artifact_text(
    text: String,
    format: ThemeFormat,
    name: &str,
    losses: Vec<String>,
) -> Result<ExportArtifact, FormatError> {
    if text.len() > MAX_DOCUMENT_BYTES {
        return Err(FormatError::TooLarge { bytes: text.len() });
    }
    Ok(ExportArtifact {
        text,
        extension: format.extension(),
        summary: format!("{} will be exported as {}", name, format),
        losses,
    })
}

fn external_losses(format: &str) -> Vec<String> {
    vec![
        format!(
            "{format} stores terminal colors only; UI colors, typography, layout and effects are omitted"
        ),
        format!("Pebrel metadata and extension fields are omitted from the {format} file"),
    ]
}
