//! Versioned theme documents and the serde-facing adapter.
//!
//! The application owns the JSON boundary.  Terminal/theme semantics are kept
//! in one validated document so format adapters and the library store cannot
//! silently invent different defaults.  Unknown JSON fields stay in the value
//! tree and are therefore available to a future schema revision.

use std::fmt;

use nebula_settings::{
    BlurModeName, CursorShapeName, MAX_PANE_CARD_DIVIDER, MAX_PANE_CARD_GUTTER,
    MAX_PANE_CARD_RADIUS, Rgb8, Rgba8, ThemeAppearance, ThemeDefinition, ThemeName,
};
use serde_json::{Map, Value, json};

pub const SCHEMA_VERSION: u64 = 1;
pub const MAX_DOCUMENT_BYTES: usize = 256 * 1024;

const TERMINAL_COLORS: &[&str] = &[
    "background",
    "foreground",
    "cursor",
    "cursor_text",
    "selection_background",
    "selection_foreground",
];
const UI_COLORS: &[&str] = &[
    "background",
    "sidebar",
    "foreground",
    "muted",
    "accent",
    "border",
    "selected",
    "success",
    "warning",
    "danger",
    "frame",
    "info",
];
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeDocument {
    value: Value,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocumentError {
    Json(String),
    TooLarge { bytes: usize },
    Invalid(String),
}

impl fmt::Display for DocumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(message) => write!(f, "invalid theme JSON: {message}"),
            Self::TooLarge { bytes } => {
                write!(f, "theme document is {bytes} bytes; limit is {MAX_DOCUMENT_BYTES}")
            },
            Self::Invalid(message) => write!(f, "invalid theme document: {message}"),
        }
    }
}

impl std::error::Error for DocumentError {}

impl ThemeDocument {
    pub fn from_json_bytes(bytes: &[u8]) -> Result<Self, DocumentError> {
        if bytes.len() > MAX_DOCUMENT_BYTES {
            return Err(DocumentError::TooLarge { bytes: bytes.len() });
        }
        let value: Value = serde_json::from_slice(bytes)
            .map_err(|error| DocumentError::Json(error.to_string()))?;
        Self::from_value(value)
    }

    pub fn from_value(value: Value) -> Result<Self, DocumentError> {
        let mut value = value;
        validate_and_normalize(&mut value)?;
        let bytes =
            serde_json::to_vec(&value).map_err(|error| DocumentError::Json(error.to_string()))?;
        if bytes.len() > MAX_DOCUMENT_BYTES {
            return Err(DocumentError::TooLarge { bytes: bytes.len() });
        }
        let document = Self { value };
        // ThemeDefinition is the settings/runtime authority for scalar limits
        // and resolved semantics.  The JSON checks above only validate the
        // envelope and lossless color representation.
        document.definition()?;
        Ok(document)
    }

    pub fn to_value(&self) -> Value {
        self.value.clone()
    }

    pub fn to_json_bytes(&self) -> Result<Vec<u8>, DocumentError> {
        let bytes = serde_json::to_vec_pretty(&self.value)
            .map_err(|error| DocumentError::Json(error.to_string()))?;
        if bytes.len() > MAX_DOCUMENT_BYTES {
            return Err(DocumentError::TooLarge { bytes: bytes.len() });
        }
        Ok(bytes)
    }

    pub fn name(&self) -> &str {
        self.value.get("name").and_then(Value::as_str).unwrap_or("")
    }

    pub fn appearance(&self) -> &str {
        self.value.get("appearance").and_then(Value::as_str).unwrap_or("dark")
    }

    pub fn id(&self) -> Option<&str> {
        self.value.get("id").and_then(Value::as_str)
    }

    pub fn revision(&self) -> u64 {
        self.value.get("revision").and_then(Value::as_u64).unwrap_or(0)
    }

    pub fn is_builtin(&self) -> bool {
        self.value.get("builtin").and_then(Value::as_bool).unwrap_or(false)
    }

    /// Resolve the versioned envelope into the runtime value object.  The
    /// returned definition owns all data, so renderers can retain it without
    /// reading JSON on a frame path.
    pub fn definition(&self) -> Result<ThemeDefinition, DocumentError> {
        let base = self
            .value
            .get("metadata")
            .and_then(Value::as_object)
            .and_then(|metadata| metadata.get("base"))
            .and_then(Value::as_str)
            .and_then(ThemeName::from_prompt_name)
            .or_else(|| ThemeName::from_prompt_name(self.name()))
            .unwrap_or_default();
        let mut definition = ThemeDefinition::from_builtin(base);
        definition.name = self.name().to_owned();
        definition.appearance = ThemeAppearance::from_settings(self.appearance())
            .ok_or_else(|| DocumentError::Invalid("appearance must be light or dark".to_owned()))?;

        let terminal = object(&self.value, "terminal")?;
        definition.terminal.background = color(terminal, "background")?;
        definition.terminal.foreground = color(terminal, "foreground")?;
        definition.terminal.cursor = optional_color(terminal, "cursor")?;
        definition.terminal.cursor_text = optional_color(terminal, "cursor_text")?;
        definition.terminal.cursor_stroke = optional_color(terminal, "cursor_stroke")?;
        definition.terminal.selection_background =
            optional_color(terminal, "selection_background")?;
        definition.terminal.selection_foreground =
            optional_color(terminal, "selection_foreground")?;

        let palette = terminal.get("palette").and_then(Value::as_array).ok_or_else(|| {
            DocumentError::Invalid("terminal.palette must be an array".to_owned())
        })?;
        for (index, value) in palette.iter().enumerate() {
            definition.terminal.palette.colors[index] =
                parse_hex_rgb(value.as_str().ok_or_else(|| {
                    DocumentError::Invalid(format!("terminal.palette[{index}] must be a color"))
                })?)
                .ok_or_else(|| {
                    DocumentError::Invalid(format!("terminal.palette[{index}] must be a color"))
                })?;
        }
        if let Some(indexed) = terminal.get("indexed").and_then(Value::as_object) {
            for (key, value) in indexed {
                let index = key.parse::<usize>().map_err(|_| {
                    DocumentError::Invalid(format!("indexed color key {key} is not an integer"))
                })?;
                let color = parse_hex_rgb(value.as_str().ok_or_else(|| {
                    DocumentError::Invalid(format!("indexed color {key} must be a color"))
                })?)
                .ok_or_else(|| {
                    DocumentError::Invalid(format!("indexed color {key} must be a color"))
                })?;
                definition.terminal.palette.colors[index] = color;
            }
        }

        let ui = object(&self.value, "ui")?;
        definition.ui.derive = ui.get("derive").and_then(Value::as_bool).unwrap_or(true);
        definition.ui.background = color(ui, "background")?;
        definition.ui.shell = color(ui, "sidebar")?;
        definition.ui.foreground = color(ui, "foreground")?;
        definition.ui.muted = color(ui, "muted")?;
        definition.ui.accent = color(ui, "accent")?;
        definition.ui.selection = rgba(ui, "selected")?;
        definition.ui.line = rgba(ui, "border")?;
        definition.ui.frame =
            optional_color(ui, "frame")?.unwrap_or(rgb_from_rgba(definition.ui.line));
        definition.ui.error = color(ui, "danger")?;
        definition.ui.success = color(ui, "success")?;
        definition.ui.warning = color(ui, "warning")?;
        definition.ui.info =
            optional_color(ui, "info")?.unwrap_or(definition.terminal.palette.colors[14]);

        let typography = object(&self.value, "typography")?;
        // These fields are part of the shared model even when the typography
        // override is disabled.  Reading them unconditionally keeps explicit
        // `ligatures: false` and UI font choices lossless across a round trip.
        definition.typography.ligatures = required_bool(typography, "ligatures")?;
        definition.typography.ui_font_family = optional_string(typography, "ui_font_family")?;
        definition.typography.ui_font_size =
            optional_number(typography, "ui_font_size")?.map(|value| value as f32);
        if typography.get("enabled").and_then(Value::as_bool).unwrap_or(false) {
            definition.typography.font_family = optional_string(typography, "font_family")?;
            definition.typography.font_size =
                optional_number(typography, "font_size")?.map(|value| value as f32);
            definition.typography.font_weight =
                optional_number(typography, "font_weight")?.map(|value| value as u16);
            definition.typography.line_height =
                optional_number(typography, "line_height")?.map(|value| value as f32);
            definition.typography.letter_spacing =
                optional_number(typography, "letter_spacing")?.map(|value| value as f32);
        }

        let layout = object(&self.value, "layout")?;
        definition.layout.card.radius = number(layout, "radius")? as f32;
        definition.layout.card.gutter = number(layout, "gutter")? as f32;
        definition.layout.card.divider = number(layout, "divider_width")? as f32;
        definition.layout.card.shadow =
            layout.get("shadow").and_then(Value::as_bool).unwrap_or(false);
        definition.layout.padding_x =
            optional_number(layout, "padding_x")?.map(|value| value as f32);
        definition.layout.padding_y =
            optional_number(layout, "padding_y")?.map(|value| value as f32);

        let effects = object(&self.value, "effects")?;
        definition.effects.opacity = effects
            .get("opacity")
            .filter(|value| !value.is_null())
            .map(|_| number(effects, "opacity").map(|value| value as f32 / 100.0))
            .transpose()?;
        definition.effects.blur = if effects.contains_key("blur") {
            match effects.get("blur") {
                Some(Value::Null) | None => None,
                Some(value) => Some(
                    BlurModeName::from_settings(value.as_str().ok_or_else(|| {
                        DocumentError::Invalid("effects.blur must be a string or null".to_owned())
                    })?)
                    .ok_or_else(|| DocumentError::Invalid("effects.blur is invalid".to_owned()))?,
                ),
            }
        } else {
            match effects.get("material").and_then(Value::as_str) {
                Some("glass") => Some(BlurModeName::Mica),
                _ => None,
            }
        };
        definition.effects.cursor_shape = match effects.get("cursor_shape") {
            None | Some(Value::Null) => None,
            Some(value) => Some(
                CursorShapeName::from_settings(value.as_str().ok_or_else(|| {
                    DocumentError::Invalid(
                        "effects.cursor_shape must be a string or null".to_owned(),
                    )
                })?)
                .ok_or_else(|| {
                    DocumentError::Invalid("effects.cursor_shape is invalid".to_owned())
                })?,
            ),
        };
        definition.effects.background_image =
            effects.get("background_image").and_then(Value::as_str).map(str::to_owned);
        definition.effects.background_image_opacity = effects
            .get("background_image_opacity")
            .filter(|value| !value.is_null())
            .map(|_| number(effects, "background_image_opacity").map(|value| value as f32 / 100.0))
            .transpose()?;
        definition.effects.background_image_fit =
            effects.get("background_image_fit").and_then(Value::as_str).map(str::to_owned);
        definition.effects.background_image_alignment =
            effects.get("background_image_alignment").and_then(Value::as_str).map(str::to_owned);
        definition.effects.background_image_cover_chrome =
            effects.get("background_image_cover_chrome").and_then(Value::as_bool);
        definition.validate().map_err(|error| DocumentError::Invalid(error.to_string()))?;
        Ok(definition)
    }

    /// Alias used by consumers that distinguish parsing from runtime resolve.
    pub fn resolve(&self) -> Result<ThemeDefinition, DocumentError> {
        self.definition()
    }

    /// Replace known theme values while retaining unknown vendor fields and
    /// the document identity/revision held by this envelope.
    pub fn with_definition(&self, definition: &ThemeDefinition) -> Result<Self, DocumentError> {
        let replacement = from_definition(definition)?;
        let mut value = self.value.clone();
        for key in ["name", "appearance", "terminal", "ui", "typography", "layout", "effects"] {
            if let Some(next) = replacement.value.get(key) {
                if let Some(current) = value.get_mut(key) {
                    let preserved = match key {
                        // Unknown vendor fields are retained by `merge_value`;
                        // every known field is owned by ThemeDefinition and
                        // must be replaced by the new snapshot.
                        "typography" | "layout" | "effects" => &[][..],
                        _ => &[][..],
                    };
                    merge_value(current, next, preserved);
                } else {
                    value[key] = next.clone();
                }
            }
        }
        if let Some(base) = replacement
            .value
            .get("metadata")
            .and_then(Value::as_object)
            .and_then(|metadata| metadata.get("base"))
        {
            if !value.get("metadata").is_some_and(Value::is_object) {
                value["metadata"] = json!({});
            }
            value["metadata"]["base"] = base.clone();
        }
        Self::from_value(value)
    }

    pub fn with_id_and_revision(&self, id: impl Into<String>, revision: u64) -> Self {
        let mut value = self.value.clone();
        if let Some(object) = value.as_object_mut() {
            object.insert("id".to_owned(), Value::String(id.into()));
            object.insert("revision".to_owned(), Value::Number(revision.into()));
            object.insert("builtin".to_owned(), Value::Bool(false));
        }
        Self { value }
    }

    pub fn with_name(&self, name: impl Into<String>) -> Result<Self, DocumentError> {
        let mut value = self.value.clone();
        value["name"] = Value::String(name.into());
        Self::from_value(value)
    }

    pub fn fork(
        &self,
        id: impl Into<String>,
        name: impl Into<String>,
    ) -> Result<Self, DocumentError> {
        let mut value = self.value.clone();
        if let Some(object) = value.as_object_mut() {
            object.insert("id".to_owned(), Value::String(id.into()));
            object.insert("revision".to_owned(), Value::Number(0.into()));
            object.insert("builtin".to_owned(), Value::Bool(false));
            object.insert("name".to_owned(), Value::String(name.into()));
            object.entry("metadata".to_owned()).or_insert_with(|| json!({}));
            if let Some(metadata) = object.get_mut("metadata").and_then(Value::as_object_mut) {
                metadata.insert("based_on".to_owned(), json!(self.name()));
            }
        }
        Self::from_value(value)
    }

    pub fn color(&self, section: &str, key: &str) -> Option<Rgb8> {
        self.value
            .get(section)
            .and_then(Value::as_object)
            .and_then(|object| object.get(key))
            .and_then(Value::as_str)
            .and_then(|value| {
                parse_hex_rgb(value).or_else(|| parse_hex_rgba(value).map(rgb_from_rgba))
            })
    }

    pub fn palette(&self) -> Option<[[u8; 3]; 16]> {
        let array = self.value.get("terminal")?.get("palette")?.as_array()?;
        if array.len() != 16 {
            return None;
        }
        let mut output = [[0; 3]; 16];
        for (index, value) in array.iter().enumerate() {
            output[index] = parse_hex_rgb(value.as_str()?)?;
        }
        Some(output)
    }

    pub fn indexed(&self) -> Option<&Map<String, Value>> {
        self.value.get("terminal")?.get("indexed")?.as_object()
    }
}

pub fn builtin_documents() -> Vec<ThemeDocument> {
    ThemeName::BUILTIN.into_iter().filter_map(|theme| builtin_document(theme).ok()).collect()
}

pub fn builtin_document(theme: ThemeName) -> Result<ThemeDocument, DocumentError> {
    let definition = ThemeDefinition::from_builtin(theme);
    let mut document = from_definition(&definition)?;
    let mut value = document.to_value();
    value["id"] = Value::String(format!("builtin-{}", slug(theme.prompt_name())));
    value["builtin"] = Value::Bool(true);
    value["revision"] = Value::Number(0.into());
    value["metadata"]["origin"] = Value::String("builtin".to_owned());
    value["metadata"]["base"] = Value::String(theme.prompt_name().to_owned());
    document = ThemeDocument::from_value(value)?;
    Ok(document)
}

/// Build a complete native envelope for an external terminal palette.  The
/// remaining values come from the default settings theme, while the imported
/// background and foreground remain byte-for-byte explicit.
pub fn seed_document(
    name: &str,
    background: Rgb8,
    foreground: Rgb8,
) -> Result<ThemeDocument, DocumentError> {
    let base = ThemeName::default();
    let mut definition = ThemeDefinition::from_builtin(base);
    definition.name = name.to_owned();
    definition.appearance = if perceived_luminance(background) > 0.55 {
        ThemeAppearance::Light
    } else {
        ThemeAppearance::Dark
    };
    definition.terminal.background = background;
    definition.terminal.foreground = foreground;
    from_definition(&definition)
}

pub fn from_definition(definition: &ThemeDefinition) -> Result<ThemeDocument, DocumentError> {
    let terminal = &definition.terminal;
    let ui = &definition.ui;
    let palette =
        terminal.palette.colors[..16].iter().copied().map(format_hex_rgb).collect::<Vec<_>>();
    let indexed = terminal.palette.colors[16..]
        .iter()
        .enumerate()
        .map(|(offset, color)| ((16 + offset).to_string(), Value::String(format_hex_rgb(*color))))
        .collect::<Map<_, _>>();
    let value = json!({
        "schema_version": SCHEMA_VERSION,
        "revision": 0,
        "builtin": false,
        "name": definition.name.as_str(),
        "appearance": definition.appearance.settings_value(),
        "terminal": {
            "background": format_hex_rgb(terminal.background),
            "foreground": format_hex_rgb(terminal.foreground),
            "cursor": terminal.cursor.map(format_hex_rgb),
            "cursor_text": terminal.cursor_text.map(format_hex_rgb),
            "cursor_stroke": terminal.cursor_stroke.map(format_hex_rgb),
            "selection_background": terminal.selection_background.map(format_hex_rgb),
            "selection_foreground": terminal.selection_foreground.map(format_hex_rgb),
            "palette": palette,
            "indexed": indexed,
        },
        "ui": {
            "derive": ui.derive,
            "background": format_hex_rgb(ui.background),
            "sidebar": format_hex_rgb(ui.shell),
            "foreground": format_hex_rgb(ui.foreground),
            "muted": format_hex_rgb(ui.muted),
            "accent": format_hex_rgb(ui.accent),
            "border": format_hex_rgba(ui.line),
            "selected": format_hex_rgba(ui.selection),
            "frame": format_hex_rgb(ui.frame),
            "success": format_hex_rgb(ui.success),
            "warning": format_hex_rgb(ui.warning),
            "danger": format_hex_rgb(ui.error),
            "info": format_hex_rgb(ui.info),
        },
        "typography": {
            "enabled": definition.typography.font_family.is_some()
                || definition.typography.font_size.is_some()
                || definition.typography.font_weight.is_some()
                || definition.typography.line_height.is_some()
                || definition.typography.letter_spacing.is_some()
                || definition.typography.ui_font_family.is_some()
                || definition.typography.ui_font_size.is_some()
                || !definition.typography.ligatures,
            "font_family": definition.typography.font_family.as_deref(),
            "font_size": definition.typography.font_size,
            "font_weight": definition.typography.font_weight,
            "line_height": definition.typography.line_height,
            "letter_spacing": definition.typography.letter_spacing,
            "ligatures": definition.typography.ligatures,
            "ui_font_family": definition.typography.ui_font_family.as_deref(),
            "ui_font_size": definition.typography.ui_font_size,
        },
        "layout": { "padding_x": definition.layout.padding_x, "padding_y": definition.layout.padding_y, "radius": definition.layout.card.radius, "gutter": definition.layout.card.gutter, "shadow": definition.layout.card.shadow, "divider_width": definition.layout.card.divider },
        "effects": {
            "material": if definition.effects.blur.is_some_and(BlurModeName::enabled) { "glass" } else { "solid" },
            "blur": definition.effects.blur.map(BlurModeName::settings_value),
            "opacity": definition.effects.opacity.map(|value| value * 100.0),
            "background_image": definition.effects.background_image.as_deref(),
            "background_image_opacity": definition.effects.background_image_opacity.map(|value| value * 100.0),
            "background_image_fit": definition.effects.background_image_fit.as_deref(),
            "background_image_alignment": definition.effects.background_image_alignment.as_deref(),
            "background_image_cover_chrome": definition.effects.background_image_cover_chrome,
            "cursor_shape": definition.effects.cursor_shape.map(CursorShapeName::settings_value)
        },
        "metadata": { "base": definition.base.prompt_name() },
        "extensions": {},
    });
    ThemeDocument::from_value(value)
}

fn validate_and_normalize(value: &mut Value) -> Result<(), DocumentError> {
    let root = value
        .as_object_mut()
        .ok_or_else(|| DocumentError::Invalid("root must be an object".to_owned()))?;
    reject_reserved_keys(root)?;
    if root.get("schema_version").and_then(Value::as_u64) != Some(SCHEMA_VERSION) {
        return Err(DocumentError::Invalid(format!("schema_version must be {SCHEMA_VERSION}")));
    }
    let name = root.get("name").and_then(Value::as_str).ok_or_else(|| missing("name"))?;
    validate_name(name)?;
    let appearance =
        root.get("appearance").and_then(Value::as_str).ok_or_else(|| missing("appearance"))?;
    if !matches!(appearance, "light" | "dark") {
        return Err(DocumentError::Invalid("appearance must be light or dark".to_owned()));
    }
    if let Some(id) = root.get("id").and_then(Value::as_str) {
        validate_id(id)?;
    } else if root.contains_key("id") {
        return Err(DocumentError::Invalid("id must be a string".to_owned()));
    }
    if root.contains_key("revision") && root.get("revision").and_then(Value::as_u64).is_none() {
        return Err(DocumentError::Invalid("revision must be a non-negative integer".to_owned()));
    }
    validate_terminal(root.get_mut("terminal").ok_or_else(|| missing("terminal"))?)?;
    validate_ui(root.get_mut("ui").ok_or_else(|| missing("ui"))?)?;
    validate_typography(root.get("typography").ok_or_else(|| missing("typography"))?)?;
    validate_layout(root.get("layout").ok_or_else(|| missing("layout"))?)?;
    validate_effects(root.get("effects").ok_or_else(|| missing("effects"))?)?;
    for key in ["metadata", "extensions"] {
        if let Some(value) = root.get(key) {
            if !value.is_object() {
                return Err(DocumentError::Invalid(format!("{key} must be an object")));
            }
        }
    }
    Ok(())
}

fn validate_terminal(value: &mut Value) -> Result<(), DocumentError> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| DocumentError::Invalid("terminal must be an object".to_owned()))?;
    reject_reserved_keys(object)?;
    for key in TERMINAL_COLORS {
        if object.get(*key).is_some_and(|value| !value.is_null()) {
            normalize_color(object, key)?;
        } else if matches!(*key, "background" | "foreground") {
            return Err(missing(&format!("terminal.{key}")));
        }
    }
    let palette = object.get_mut("palette").ok_or_else(|| missing("terminal.palette"))?;
    let values = palette
        .as_array_mut()
        .ok_or_else(|| DocumentError::Invalid("terminal.palette must be an array".to_owned()))?;
    if values.len() != 16 {
        return Err(DocumentError::Invalid(
            "terminal.palette must contain exactly 16 colors".to_owned(),
        ));
    }
    for (index, value) in values.iter_mut().enumerate() {
        normalize_color_value(value, &format!("terminal.palette[{index}]"), false)?;
    }
    if let Some(indexed) = object.get_mut("indexed") {
        let values = indexed.as_object_mut().ok_or_else(|| {
            DocumentError::Invalid("terminal.indexed must be an object".to_owned())
        })?;
        reject_reserved_keys(values)?;
        for (index, value) in values.iter_mut() {
            let number: u16 = index.parse().map_err(|_| {
                DocumentError::Invalid(format!("indexed color key {index} is not an integer"))
            })?;
            if !(16..=255).contains(&number) {
                return Err(DocumentError::Invalid(format!(
                    "indexed color key {index} must be 16..255"
                )));
            }
            normalize_color_value(value, &format!("terminal.indexed.{index}"), false)?;
        }
    }
    Ok(())
}

fn validate_ui(value: &mut Value) -> Result<(), DocumentError> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| DocumentError::Invalid("ui must be an object".to_owned()))?;
    reject_reserved_keys(object)?;
    if object.get("derive").and_then(Value::as_bool).is_none() {
        return Err(DocumentError::Invalid("ui.derive must be boolean".to_owned()));
    }
    for key in UI_COLORS {
        if object.get(*key).is_some_and(|value| !value.is_null()) {
            normalize_color_value(
                object.get_mut(*key).expect("checked above"),
                &format!("ui.{key}"),
                matches!(*key, "selected" | "border"),
            )?;
        }
    }
    for key in [
        "background",
        "sidebar",
        "foreground",
        "muted",
        "accent",
        "selected",
        "border",
        "success",
        "warning",
        "danger",
    ] {
        if !object.contains_key(key) {
            return Err(missing(&format!("ui.{key}")));
        }
    }
    Ok(())
}

fn validate_typography(value: &Value) -> Result<(), DocumentError> {
    let object = value
        .as_object()
        .ok_or_else(|| DocumentError::Invalid("typography must be an object".to_owned()))?;
    required_bool(object, "enabled")?;
    required_bool(object, "ligatures")?;
    optional_string_with_limit(object, "font_family", 256)?;
    optional_string_with_limit(object, "ui_font_family", 256)?;
    optional_bounded_number(object, "font_size", 4.0, 96.0)?;
    optional_bounded_number(object, "ui_font_size", 10.0, 20.0)?;
    optional_bounded_integer(object, "font_weight", 100, 900)?;
    optional_bounded_number(object, "line_height", 0.5, 3.0)?;
    if object.contains_key("letter_spacing") && !object["letter_spacing"].is_null() {
        optional_bounded_number(object, "letter_spacing", 0.0, 8.0)?;
    }
    Ok(())
}

fn validate_layout(value: &Value) -> Result<(), DocumentError> {
    let object = value
        .as_object()
        .ok_or_else(|| DocumentError::Invalid("layout must be an object".to_owned()))?;
    optional_bounded_number(object, "padding_x", 0.0, 48.0)?;
    optional_bounded_number(object, "padding_y", 0.0, 40.0)?;
    bounded_number(object, "radius", 0.0, f64::from(MAX_PANE_CARD_RADIUS))?;
    bounded_number(object, "gutter", 0.0, f64::from(MAX_PANE_CARD_GUTTER))?;
    bounded_number(object, "divider_width", 0.0, f64::from(MAX_PANE_CARD_DIVIDER))
}

fn validate_effects(value: &Value) -> Result<(), DocumentError> {
    let object = value
        .as_object()
        .ok_or_else(|| DocumentError::Invalid("effects must be an object".to_owned()))?;
    let material = required_string(object, "material", 16)?;
    if !matches!(material, "solid" | "glass") {
        return Err(DocumentError::Invalid("effects.material must be solid or glass".to_owned()));
    }
    if object.contains_key("opacity") && !object["opacity"].is_null() {
        bounded_number(object, "opacity", 0.0, 100.0)?;
    }
    if let Some(blur) = object.get("blur") {
        if !blur.is_null() {
            let blur = blur.as_str().ok_or_else(|| {
                DocumentError::Invalid("effects.blur must be a string or null".to_owned())
            })?;
            if BlurModeName::from_settings(blur).is_none() {
                return Err(DocumentError::Invalid("effects.blur is invalid".to_owned()));
            }
        }
    }
    optional_string_with_limit(object, "background_image", 4096)?;
    optional_string_with_limit(object, "background_image_fit", 64)?;
    optional_string_with_limit(object, "background_image_alignment", 64)?;
    optional_bounded_number(object, "background_image_opacity", 0.0, 100.0)?;
    if let Some(value) = object.get("background_image_cover_chrome") {
        if !value.is_null() && !value.is_boolean() {
            return Err(DocumentError::Invalid(
                "effects.background_image_cover_chrome must be boolean or null".to_owned(),
            ));
        }
    }
    if let Some(cursor_shape) = object.get("cursor_shape") {
        if !cursor_shape.is_null() {
            let cursor_shape = cursor_shape.as_str().ok_or_else(|| {
                DocumentError::Invalid("effects.cursor_shape must be a string or null".to_owned())
            })?;
            if CursorShapeName::from_settings(cursor_shape).is_none() {
                return Err(DocumentError::Invalid("effects.cursor_shape is invalid".to_owned()));
            }
        }
    }
    Ok(())
}

fn normalize_color(object: &mut Map<String, Value>, key: &str) -> Result<(), DocumentError> {
    let value = object.get_mut(key).ok_or_else(|| missing(key))?;
    normalize_color_value(value, key, false)
}

fn normalize_color_value(value: &mut Value, path: &str, alpha: bool) -> Result<(), DocumentError> {
    let text = value
        .as_str()
        .ok_or_else(|| DocumentError::Invalid(format!("{path} must be a #RRGGBB color")))?;
    if alpha {
        let color = parse_hex_rgba(text).ok_or_else(|| {
            DocumentError::Invalid(format!("{path} must be a #RRGGBB or #RRGGBBAA color"))
        })?;
        *value = Value::String(format_hex_rgba(color));
    } else {
        let color = parse_hex_rgb(text)
            .ok_or_else(|| DocumentError::Invalid(format!("{path} must be a #RRGGBB color")))?;
        *value = Value::String(format_hex_rgb(color));
    }
    Ok(())
}

fn merge_value(current: &mut Value, replacement: &Value, preserved: &[&str]) {
    match (current, replacement) {
        (Value::Object(current), Value::Object(replacement)) => {
            for (key, value) in replacement {
                if preserved.contains(&key.as_str()) {
                    continue;
                }
                if let Some(current) = current.get_mut(key) {
                    merge_value(current, value, preserved);
                } else {
                    current.insert(key.clone(), value.clone());
                }
            }
        },
        (current, replacement) => *current = replacement.clone(),
    }
}

fn required_bool<'a>(object: &'a Map<String, Value>, key: &str) -> Result<bool, DocumentError> {
    object
        .get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| DocumentError::Invalid(format!("{key} must be boolean")))
}

fn required_string<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    max: usize,
) -> Result<&'a str, DocumentError> {
    let value = object
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| DocumentError::Invalid(format!("{key} must be a string")))?;
    if value.is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(DocumentError::Invalid(format!(
            "{key} must be 1..{max} characters without controls"
        )));
    }
    Ok(value)
}

fn optional_string<'a>(
    object: &'a Map<String, Value>,
    key: &str,
) -> Result<Option<String>, DocumentError> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => {
            optional_string_with_limit(object, key, 256).map(|value| value.map(str::to_owned))
        },
    }
}

fn optional_string_with_limit<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    max: usize,
) -> Result<Option<&'a str>, DocumentError> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let value = value
                .as_str()
                .ok_or_else(|| DocumentError::Invalid(format!("{key} must be a string or null")))?;
            if value.is_empty() || value.len() > max || value.chars().any(char::is_control) {
                return Err(DocumentError::Invalid(format!(
                    "{key} must be 1..{max} characters without controls"
                )));
            }
            Ok(Some(value))
        },
    }
}

fn bounded_number(
    object: &Map<String, Value>,
    key: &str,
    min: f64,
    max: f64,
) -> Result<(), DocumentError> {
    let value = object
        .get(key)
        .and_then(Value::as_f64)
        .ok_or_else(|| DocumentError::Invalid(format!("{key} must be a number")))?;
    if !value.is_finite() || !(min..=max).contains(&value) {
        return Err(DocumentError::Invalid(format!("{key} must be in {min}..={max}")));
    }
    Ok(())
}

fn optional_number(object: &Map<String, Value>, key: &str) -> Result<Option<f64>, DocumentError> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => number(object, key).map(Some),
    }
}

fn optional_bounded_number(
    object: &Map<String, Value>,
    key: &str,
    min: f64,
    max: f64,
) -> Result<(), DocumentError> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(()),
        Some(_) => bounded_number(object, key, min, max),
    }
}

fn optional_bounded_integer(
    object: &Map<String, Value>,
    key: &str,
    min: u64,
    max: u64,
) -> Result<(), DocumentError> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(()),
        Some(value) => {
            let number = value.as_u64().ok_or_else(|| {
                DocumentError::Invalid(format!("{key} must be an integer or null"))
            })?;
            if !(min..=max).contains(&number) {
                return Err(DocumentError::Invalid(format!("{key} must be in {min}..={max}")));
            }
            Ok(())
        },
    }
}

fn validate_name(name: &str) -> Result<(), DocumentError> {
    if name.is_empty() || name.len() > 128 || name.chars().any(char::is_control) {
        return Err(DocumentError::Invalid(
            "name must be 1..128 characters without controls".to_owned(),
        ));
    }
    Ok(())
}

fn validate_id(id: &str) -> Result<(), DocumentError> {
    if id.is_empty()
        || id.len() > 128
        || !id.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
    {
        return Err(DocumentError::Invalid("id contains unsupported characters".to_owned()));
    }
    Ok(())
}

fn reject_reserved_keys(object: &Map<String, Value>) -> Result<(), DocumentError> {
    if let Some(key) =
        object.keys().find(|key| matches!(key.as_str(), "__proto__" | "constructor" | "prototype"))
    {
        return Err(DocumentError::Invalid(format!("reserved key {key} is not allowed")));
    }
    Ok(())
}

fn missing(key: &str) -> DocumentError {
    DocumentError::Invalid(format!("missing {key}"))
}

pub fn parse_hex_rgb(value: &str) -> Option<Rgb8> {
    let text = value.strip_prefix('#')?;
    let text = match text.len() {
        3 => text.chars().flat_map(|character| [character, character]).collect::<String>(),
        6 => text.to_owned(),
        _ => return None,
    };
    if !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some([
        u8::from_str_radix(&text[0..2], 16).ok()?,
        u8::from_str_radix(&text[2..4], 16).ok()?,
        u8::from_str_radix(&text[4..6], 16).ok()?,
    ])
}

pub fn parse_hex_rgba(value: &str) -> Option<Rgba8> {
    let text = value.strip_prefix('#')?;
    let text = match text.len() {
        4 => text.chars().flat_map(|character| [character, character]).collect::<String>(),
        6 => format!("{text}ff"),
        8 => text.to_owned(),
        _ => return None,
    };
    if !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some([
        u8::from_str_radix(&text[0..2], 16).ok()?,
        u8::from_str_radix(&text[2..4], 16).ok()?,
        u8::from_str_radix(&text[4..6], 16).ok()?,
        u8::from_str_radix(&text[6..8], 16).ok()?,
    ])
}

pub fn format_hex_rgb(value: Rgb8) -> String {
    format!("#{:02x}{:02x}{:02x}", value[0], value[1], value[2])
}

pub fn format_hex_rgba(value: Rgba8) -> String {
    if value[3] == 255 {
        format_hex_rgb([value[0], value[1], value[2]])
    } else {
        format!("#{:02x}{:02x}{:02x}{:02x}", value[0], value[1], value[2], value[3])
    }
}

fn object<'a>(value: &'a Value, key: &str) -> Result<&'a Map<String, Value>, DocumentError> {
    value
        .get(key)
        .and_then(Value::as_object)
        .ok_or_else(|| DocumentError::Invalid(format!("missing {key} object")))
}

fn color(object: &Map<String, Value>, key: &str) -> Result<Rgb8, DocumentError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .and_then(parse_hex_rgb)
        .ok_or_else(|| DocumentError::Invalid(format!("{key} must be a #RGB or #RRGGBB color")))
}

fn optional_color(object: &Map<String, Value>, key: &str) -> Result<Option<Rgb8>, DocumentError> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value.as_str().and_then(parse_hex_rgb).map(Some).ok_or_else(|| {
            DocumentError::Invalid(format!("{key} must be a #RGB or #RRGGBB color"))
        }),
    }
}

fn rgba(object: &Map<String, Value>, key: &str) -> Result<Rgba8, DocumentError> {
    object.get(key).and_then(Value::as_str).and_then(parse_hex_rgba).ok_or_else(|| {
        DocumentError::Invalid(format!("{key} must be a #RGB[A] or #RRGGBB[AA] color"))
    })
}

fn number(object: &Map<String, Value>, key: &str) -> Result<f64, DocumentError> {
    object
        .get(key)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .ok_or_else(|| DocumentError::Invalid(format!("{key} must be a finite number")))
}

fn rgb_from_rgba(value: Rgba8) -> Rgb8 {
    [value[0], value[1], value[2]]
}

fn perceived_luminance(color: Rgb8) -> f32 {
    (0.2126 * f32::from(color[0]) + 0.7152 * f32::from(color[1]) + 0.0722 * f32::from(color[2]))
        / 255.0
}

fn slug(value: &str) -> String {
    let mut slug = String::new();
    for character in value.chars() {
        if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
            slug.push(character.to_ascii_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() { "theme".to_owned() } else { slug.to_owned() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_produce_fifteen_valid_documents() {
        let documents = builtin_documents();
        assert_eq!(documents.len(), 15);
        assert!(documents.iter().all(|document| document.palette().is_some()));
    }

    #[test]
    fn unknown_fields_survive_validation_and_round_trip() {
        let mut value = builtin_documents().remove(0).to_value();
        value["vendor"] = json!({ "future": true });
        value["terminal"]["vendor_color"] = json!("keep");
        let document = ThemeDocument::from_value(value.clone()).unwrap();
        assert_eq!(document.to_value()["vendor"], value["vendor"]);
        assert_eq!(document.to_value()["terminal"]["vendor_color"], "keep");
    }

    #[test]
    fn bad_explicit_colors_and_index_boundaries_are_rejected() {
        let mut value = builtin_documents().remove(0).to_value();
        value["terminal"]["foreground"] = json!("#gg0000");
        assert!(ThemeDocument::from_value(value).is_err());
        let mut value = builtin_documents().remove(0).to_value();
        value["terminal"]["indexed"]["15"] = json!("#112233");
        assert!(ThemeDocument::from_value(value).is_err());
        let mut value = builtin_documents().remove(0).to_value();
        value["terminal"]["indexed"]["255"] = json!("#112233");
        assert!(ThemeDocument::from_value(value).is_ok());
    }
}
