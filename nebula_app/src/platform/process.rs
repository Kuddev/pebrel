//! 从本进程启动控制台子进程时的统一抑制入口。
//!
//! Pebrel 是 `windows_subsystem = "windows"` 的 GUI 进程（见 `main.rs`），
//! **自己没有控制台可以给子进程继承**；`cargo test --bin pebrel` 的测试二进制
//! 从同一个 crate root 编出来，同样没有。于是任何没带 `CREATE_NO_WINDOW` 的
//! 控制台子进程（`git`、`ssh -G`、`wsl.exe`…）都会被 Windows 分配一个新控制台
//! ——在默认终端应用是 Windows Terminal 的机器上，那就是**用户屏幕上弹一整扇
//! 窗口**。2026-09-14 实测：整跑一次测试弹出 86 个窗口。
//!
//! 用法：构造完参数、`spawn()` 之前过一道。
//!
//! ```ignore
//! let mut command = Command::new("git");
//! command.args(["status"]);
//! crate::platform::process::hidden_command(&mut command).output()?;
//! ```
//!
//! **不要**给需要与用户交互的子进程加这个（`pebrel ssh <host>` 的交互会话就
//! 靠继承父控制台工作，见 `ssh::run`）；同理，`notepad` / `explorer` / `open`
//! 那几处是**故意**要给用户看见窗口的。

use std::process::Command;

/// `CREATE_NO_WINDOW` 的**唯一定义处**。
///
/// 需要它的地方几乎都该直接调 [`hidden_command`]；把这个常量单独导出，是为了
/// 那些 `std::process::Command` 之外的类型——`tokio::process::Command`
/// （`ssh_proxy.rs` 的代理命令）不是同一个类型，只能自己 `creation_flags`，
/// 但至少取值仍然只有这一个来源。别再写 `0x0800_0000`。
#[cfg(windows)]
pub(crate) const CREATE_NO_WINDOW: u32 = windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

/// 抑制子进程的控制台窗口；非 Windows 上是空操作。
pub(crate) fn hidden_command(command: &mut Command) -> &mut Command {
    hidden_command_with(command, 0)
}

/// 同 [`hidden_command`]，但额外叠加调用方自己的创建标志。
///
/// `creation_flags` 是**整体替换**而不是按位或，所以调用方给的 `extra_flags`
/// 必须自己带全（例如 daemon 需要的 `CREATE_NEW_PROCESS_GROUP`）；本函数负责
/// 保证 `CREATE_NO_WINDOW` 一定在里面。
pub(crate) fn hidden_command_with(command: &mut Command, extra_flags: u32) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;

        command.creation_flags(CREATE_NO_WINDOW | extra_flags);
    }
    #[cfg(not(windows))]
    let _ = extra_flags;
    command
}
