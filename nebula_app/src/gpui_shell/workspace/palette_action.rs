//! Palette/launcher row actions.
//!
//! Lives beside the palette projection instead of the workspace root: the rows
//! and their commands change together, and `workspace.rs` sits at its line
//! budget. Execution stays in `palette::run_workspace_palette_action`.

#[derive(Clone)]
pub(crate) enum WorkspacePaletteAction {
    TerminalReading {
        commands: bool,
    },
    Shared(crate::display::command_palette::PaletteAction),
    /// 聚焦已经存在的工作区标签，不创建副本。
    FocusTab(usize),
    LayoutRecipes,
    /// 先激活标签，再把焦点交给其中的明确 pane。
    FocusPane {
        tab: usize,
        pane: u64,
    },
    /// 在 frecency 目录中新建终端；已有目录中的 pane 由上面的 FocusPane 命中。
    OpenDirectory(std::path::PathBuf),
    RunAiSession {
        command: String,
        cwd: Option<std::path::PathBuf>,
    },
    /// 启动器混排的 SSH 主机行（数据源 = 共享主机列表权威）。
    LaunchSshHost(String),
    /// 检索无命中时的兜底行：预填 SSH 编辑器，保存成功后自动连接。
    AddSshHostAndConnect(String),
    /// 新建终端弹窗里的一台已检测 shell（旧壳 `ProfileRow::Shell`）。
    LaunchShell(crate::shell_detect::DetectedShell),
    /// 设置页“导入终端目录”落盘的可执行文件快照。
    LaunchProfile(crate::config::ui_config::Profile),
}
