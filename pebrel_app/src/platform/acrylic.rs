//! Optional native Acrylic lifecycle behind a platform-neutral shell interface.
//! Windows owns the material; other platforms retain their existing GPUI backdrop.

#[cfg(windows)]
mod bindings;
#[cfg(windows)]
mod composition;
#[cfg(windows)]
mod controller;
#[cfg(all(test, windows))]
mod gpui_tests;
#[cfg(windows)]
mod runtime;
#[cfg(all(test, windows))]
mod tests;
#[cfg(windows)]
mod transitions;

#[cfg(all(test, windows))]
pub(crate) use controller::test_snapshot;
#[cfg(windows)]
pub(crate) use controller::{RunGuard, apply, init, remove};

#[cfg(not(windows))]
#[derive(Default)]
pub(crate) struct RunGuard;

#[cfg(not(windows))]
pub(crate) fn init(_: &mut gpui::App) {}

#[cfg(not(windows))]
pub(crate) fn remove(_: gpui::WindowId) {}
