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
    /// 启动器四个 chip 的计数。
    ///
    /// `Shell` 与 `Profiles` 分开数——搜索框的 placeholder 写着「搜索 Shell、
    /// **配置**和 SSH 主机」，三个名词对应三个分类；此前 `Shell` 把 quick-launch
    /// profile 一并算进去，点进去看到的不是一台 shell。
    pub(super) fn launcher_chip_counts(
        &self,
    ) -> [(crate::display::command_palette::LauncherFilter, usize); 4] {
        use crate::display::command_palette::LauncherFilter;
        let rows = self.palette_override.as_deref().unwrap_or(&[]);
        let count = |matched: fn(&WorkspacePaletteAction) -> bool| {
            rows.iter().filter(|row| matched(&row.action)).count()
        };
        let shell = count(|action| matches!(action, WorkspacePaletteAction::LaunchShell(_)));
        let profiles = count(|action| matches!(action, WorkspacePaletteAction::LaunchProfile(_)));
        let ssh = count(|action| matches!(action, WorkspacePaletteAction::LaunchSshHost(_)));
        [
            (LauncherFilter::All, shell + profiles + ssh),
            (LauncherFilter::Ssh, ssh),
            (LauncherFilter::Shell, shell),
            (LauncherFilter::Profiles, profiles),
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
