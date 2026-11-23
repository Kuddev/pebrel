//! Active local writers own one canonical target until publication finishes.

use crate::text_document::SaveError;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

fn active() -> &'static Mutex<HashSet<PathBuf>> {
    static ACTIVE: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
    ACTIVE.get_or_init(Mutex::default)
}

pub(super) struct Permit(PathBuf);

pub(super) fn acquire(target: &Path) -> Result<Permit, SaveError> {
    let mut targets = active().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if !targets.insert(target.to_owned()) {
        return Err(SaveError::Busy);
    }
    Ok(Permit(target.to_owned()))
}

impl Drop for Permit {
    fn drop(&mut self) {
        active().lock().unwrap_or_else(|poisoned| poisoned.into_inner()).remove(&self.0);
    }
}
