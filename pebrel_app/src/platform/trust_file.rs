//! Replacement of an existing trust file must preserve its access controls.
use std::io;
use std::path::Path;

#[cfg(windows)]
mod windows;

pub(crate) fn replace(source: &Path, destination: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        windows::replace(source, destination)
    }
    #[cfg(not(windows))]
    {
        crate::atomic_file::replace(source, destination)
    }
}
