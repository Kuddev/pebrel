//! Default drawer cwd binding; auxiliary drawers own independent bindings.
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

    /// 抽屉这一帧该跟随的位置。WSL 先分流：只有 `/mnt/<盘>` 映射到宿主盘，
    /// `/`、`/home`、`/etc` 等始终交给来宾 `find`，不依赖不稳定的 UNC。
    ///
    /// 为什么要这个门：`/mnt/<盘>/…` 已经被 [`Self::active_local_cwd`] 落回宿
    /// 主盘，此时宿主 git 与来宾 git 读的是同一个工作树，而宿主 git 快得多
    /// （来宾路径每次快照都要起一个 `wsl.exe`，`/mnt` 上的 git 本身也慢）。
    /// 只有 `/home/…` 这类来宾自有路径宿主根本读不到，才值得走来宾。
    pub(super) fn side_panel_follow(
        &self,
        cx: &App,
    ) -> (Option<std::path::PathBuf>, Option<crate::shell_detect::WslCwd>) {
        if let Some(wsl) = self.active_wsl_cwd(cx) {
            return match crate::shell_detect::wsl_mounted_host_cwd(&wsl) {
                Some(cwd) => (Some(cwd), None),
                None => (None, Some(wsl)),
            };
        }
        (self.active_local_cwd(cx), None)
    }

    /// 让窗口级共享文件树立即绑定当前 active tab / focused pane。切 pane/tab
    /// 时清掉树内手动浏览根；普通 OSC 7 轮询只在 cwd 真变化时由模型自动清掉。
    pub(super) fn sync_side_panel_to_active(
        &mut self,
        reset_browse_root: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.side_panel.open {
            return false;
        }
        // 远端 pane 的位置不在本机文件系统里。不早退的话，`side_panel_follow`
        // 会拿远端路径去问宿主 `is_dir`（必然为假），于是本地树保留上一个有效
        // 根——用户在 SSH tab 上看到的是**上一个本地目录**的内容，而且没有任何
        // 提示。这正是"远端浏览器识别不到"的观感来源。
        if self
            .tabs
            .get(self.active)
            .and_then(WorkspaceTab::focused_view)
            .is_some_and(|view| view.read(cx).ssh_destination.is_some())
        {
            return false;
        }
        let (cwd, wsl) = self.side_panel_follow(cx);
        let cleared = reset_browse_root && self.side_panel.clear_custom_root();
        self.side_panel.sync_at(cwd, wsl) || cleared
    }
}
