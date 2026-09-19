//! Native theme library boundary.
//!
//! `document` owns the versioned Pebrel envelope, `formats` owns bounded
//! import/export adapters, and `store` owns durable custom-theme files.  The
//! settings crate remains the single runtime definition authority.

pub(crate) mod document;
mod formats;
pub(crate) mod preferences;
mod store;

pub use document::{
    DocumentError, MAX_DOCUMENT_BYTES, SCHEMA_VERSION, ThemeDocument, builtin_document,
    builtin_documents, format_hex_rgb, format_hex_rgba, from_definition, parse_hex_rgb,
    parse_hex_rgba, seed_document,
};
pub use formats::{
    ExportArtifact, FormatError, ImportCandidate, ImportDiagnostic, Inspection,
    MAX_IMPORT_CANDIDATES, MAX_INPUT_BYTES, ThemeFormat, export, inspect,
};
pub use store::{
    MAX_CUSTOM_THEMES, RevisionPrecondition, StoreDiagnostic, StoreError, THEME_FILE_SUFFIX,
    ThemeLibrarySnapshot, ThemeLibraryStore,
};

#[cfg(test)]
mod tests;

#[cfg(test)]
mod format_tests;
