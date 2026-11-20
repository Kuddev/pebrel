//! Normalize the process attribute before any terminal children inherit it.

pub(crate) fn prepare_console_for_gui() -> std::io::Result<()> {
    // The NULL-handler ignore flag is inherited by ConPTY shells. Normalize it
    // once, before any workers or terminals exist; never toggle it around spawn.
    // SAFETY: this changes only this process's console attribute, not its parent
    // or any existing child. The call also works without an attached console.
    if unsafe { windows_sys::Win32::System::Console::SetConsoleCtrlHandler(None, 0) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}
