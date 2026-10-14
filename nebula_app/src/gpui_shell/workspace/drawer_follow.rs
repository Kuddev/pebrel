//! Focused pane directory identity shared by local file and terminal actions.
//! This extraction preserves existing local/WSL routing.
use super::*;

impl NebulaWorkspace {
    /// 聚焦 tab 的**宿主可见** cwd。必须先识别 WSL，再读 `local_cwd()`：Windows
    /// 会把来宾 `/` 当成当前盘根目录，反过来的顺序会把 WSL `/` 显示成 `D:\`。
    /// WSL `/mnt/<盘>` 与可达 UNC 仍可供复制/Explorer 等宿主操作使用。
    ///
    /// Git 视图不受这里成败影响——它另有来宾直读路径，见 [`Self::active_wsl_cwd`]。
    pub(super) fn active_local_cwd(&self, cx: &App) -> Option<std::path::PathBuf> {
        if let Some(located) = self.active_wsl_cwd(cx) {
            return crate::shell_detect::wsl_host_cwd(&located);
        }
        let view = self.tabs.get(self.active).and_then(WorkspaceTab::focused_view)?;
        view.read(cx).local_cwd()
    }

    /// 聚焦 pane 的 WSL 发行版（spawn 快照）+ 来宾目录；不是 WSL 时为 `None`。
    /// Git 视图拿它在来宾里直接跑 git，不经
    /// 任何 UNC 映射，所以宿主看不见 WSL 文件系统时依然有效。
    pub(super) fn active_wsl_cwd(&self, cx: &App) -> Option<crate::shell_detect::WslCwd> {
        let view = self.tabs.get(self.active).and_then(WorkspaceTab::focused_view)?.read(cx);
        let distro = view.wsl_distro()?.to_owned();
        let guest = crate::shell_detect::wsl_guest_cwd(&view.cwd)?;
        Some(crate::shell_detect::WslCwd { distro, guest: guest.to_owned() })
    }
}
