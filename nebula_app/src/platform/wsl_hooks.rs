//! Guest hook setup is a Windows transport capability. Provider policy stays in ai_hook.
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::{prepare, setup_cli};

#[cfg(not(windows))]
pub(crate) fn prepare(_options: &mut nebula_terminal::tty::Options) {}
