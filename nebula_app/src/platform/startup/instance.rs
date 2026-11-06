//! Process launch intent shared by runtime discovery and workspace ownership.

use std::sync::OnceLock;

static NEW_INSTANCE: OnceLock<bool> = OnceLock::new();

/// Called once after CLI parsing, before runtime servers or windows exist.
#[cfg(feature = "gpui-shell")]
pub(crate) fn initialize(new_instance: bool) {
    NEW_INSTANCE.set(new_instance).expect("instance launch intent initialized twice");
}

pub(crate) fn explicitly_requested() -> bool {
    NEW_INSTANCE.get().copied().unwrap_or(false)
}

/// Private instances own a child-only runtime endpoint and an unsaved workspace.
/// Elevation remains an independent reason to require the same isolation.
pub(crate) fn is_private() -> bool {
    explicitly_requested() || crate::platform::elevation::requires_isolation()
}
