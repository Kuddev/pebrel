//! Persistent preferences for the command-history store.

use crate::RawSettings;

pub(crate) const COMMAND_HISTORY_PERSIST: &str = "command_history_persist";

impl RawSettings {
    /// Keep saving command history unless the saved preference opts out.
    pub fn command_history_persists(&self) -> bool {
        self.bool_on(COMMAND_HISTORY_PERSIST).unwrap_or(true)
    }
}
