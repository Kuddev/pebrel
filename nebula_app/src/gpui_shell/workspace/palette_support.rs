//! 调色板（Ctrl+K 启动器 / Quick Jump）的分区计数。
//!
//! 从 `workspace.rs` 搬出来的：那个文件早已超过 2000 行软目标，被
//! `architecture/file-budgets.txt` 以「零增长」例外锁定——继续往里加东西
//! 会被 `scripts/check_architecture.py --base` 直接判错。计数逻辑不碰窗口、
//! 不碰渲染，只读一份已经算好的行快照，是这里最该先搬的一块。
//!
//! 放在子模块里访问 `NebulaWorkspace` 的私有字段是成立的：Rust 的可见性
//! 规则允许后代模块读取祖先模块的私有项，所以无需为搬家放开任何字段。

use super::{NebulaWorkspace, QuickJumpFilter, WorkspacePaletteAction};

impl NebulaWorkspace {
    /// 启动器三个 chip 的计数。
    ///
    /// `Shell` 目前把 `LaunchProfile` 也算进去——这是既有口径：搜索框的
    /// placeholder 写的是「搜索 Shell、配置和 SSH 主机」，说明 profile 在
    /// 用户心智里属于 Shell 一栏。
    pub(super) fn launcher_chip_counts(
        &self,
    ) -> [(crate::display::command_palette::LauncherFilter, usize); 3] {
        use crate::display::command_palette::LauncherFilter;
        let rows = self.palette_override.as_deref().unwrap_or(&[]);
        let shell = rows
            .iter()
            .filter(|row| {
                matches!(
                    row.action,
                    WorkspacePaletteAction::LaunchShell(_)
                        | WorkspacePaletteAction::LaunchProfile(_)
                )
            })
            .count();
        let ssh = rows
            .iter()
            .filter(|row| matches!(row.action, WorkspacePaletteAction::LaunchSshHost(_)))
            .count();
        [
            (LauncherFilter::All, shell + ssh),
            (LauncherFilter::Ssh, ssh),
            (LauncherFilter::Shell, shell),
        ]
    }

    /// Quick Jump 五个 chip 的计数；每个 scope 自己判定匹配，避免这里再抄一遍
    /// `QuickJumpFilter::matches` 的语义。
    pub(super) fn quick_jump_chip_counts(&self) -> [(QuickJumpFilter, usize); 5] {
        let rows = self.palette_override.as_deref().unwrap_or(&[]);
        QuickJumpFilter::ALL.map(|filter| {
            let count = rows.iter().filter(|row| filter.matches(&row.action)).count();
            (filter, count)
        })
    }
}
