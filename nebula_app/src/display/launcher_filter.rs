//! 启动器（Ctrl+K / 三点菜单）顶部筛选栏的取值。
//!
//! 从 `command_palette.rs` 搬出来的：那个文件早超过 2000 行软目标，被
//! `architecture/file-budgets.txt` 以「零增长」例外锁定，继续往里加东西会被
//! `scripts/check_architecture.py --base` 直接判错。筛选栏这个枚举自成一格——
//! 它只描述「看哪一类」，不碰渲染也不碰候选排序——是那里最该先搬的一块。

/// 启动器的筛选栏。`All` 之外每一项都对应一类候选行。
///
/// `Shell` 与 `Profiles` 是**分开**的两栏：搜索框的 placeholder 一直写着
/// 「搜索 Shell、**配置**和 SSH 主机」，三个名词对应三个分类，而 `Shell` 此前
/// 把 quick-launch profile 也算进去，与这句文案自相矛盾——用户点「Shell」
/// 想看的是一台 shell，不是他的项目入口。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherFilter {
    All,
    Ssh,
    Shell,
    Profiles,
}

impl LauncherFilter {
    pub(crate) fn label(self, language: crate::display::UiLanguage) -> &'static str {
        match self {
            Self::All => language.pick("全部", "All"),
            Self::Ssh => "SSH",
            Self::Shell => "Shell",
            Self::Profiles => language.pick("配置", "Profiles"),
        }
    }

    pub(super) fn next(self, delta: i32) -> Self {
        const FILTERS: [LauncherFilter; 4] = [
            LauncherFilter::All,
            LauncherFilter::Ssh,
            LauncherFilter::Shell,
            LauncherFilter::Profiles,
        ];
        let index = FILTERS.iter().position(|filter| *filter == self).unwrap_or(0) as i32;
        FILTERS[(index + delta).rem_euclid(FILTERS.len() as i32) as usize]
    }
}
