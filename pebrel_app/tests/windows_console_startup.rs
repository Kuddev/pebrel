//! A console-subsystem target inherits the parent's Ctrl+C state automatically.
//! A GUI target calling AttachConsole would reset that state and hide the bug.
#![cfg(windows)]
#![windows_subsystem = "console"]

#[path = "../src/platform/startup/console.rs"]
mod console;
use console::prepare_console_for_gui;

#[path = "../src/platform/startup/console_tests.rs"]
mod console_tests;
