//! 命令恢复只消费已确认的 shell 提交，不从运行徽章推断提交。

use super::TerminalView;
use nebula_terminal::term::TermMode;

impl TerminalView {
    pub(crate) fn restore_output_batch(
        path: std::path::PathBuf,
        targets: Vec<(String, gpui::WeakEntity<Self>)>,
        cx: &mut gpui::App,
    ) {
        if targets.is_empty() {
            return;
        }
        let read = cx
            .background_executor()
            .spawn(async move { crate::recent_output::storage::Archive::read_from(&path) });
        cx.spawn(async move |cx| {
            let mut archive = match read.await {
                Ok(archive) => archive,
                Err(error) => {
                    if error.kind() != std::io::ErrorKind::NotFound {
                        log::warn!("Could not restore command output: {error}");
                    }
                    return;
                },
            };
            for (id, target) in targets {
                let Some(records) = archive.take_records(&id) else { continue };
                let _ = target.update(cx, |view, cx| {
                    if view.exited.is_some() {
                        return;
                    }
                    if let Err(error) = view.restore_recent_output(records, cx) {
                        log::warn!("Invalid saved command output: {error}");
                    }
                });
            }
        })
        .detach();
    }

    pub(crate) fn set_recent_output_enabled(&mut self, enabled: bool) {
        self.recent_output_enabled = enabled;
        if !enabled {
            self.clear_recent_output();
        }
    }

    pub(crate) fn clear_recent_output(&mut self) {
        self.recent_output = crate::recent_output::RecentOutput::default();
        self.recent_output_restore_closed = true;
    }

    pub(crate) fn restore_recent_output(
        &mut self,
        records: Vec<crate::recent_output::CommandRecord>,
        cx: &mut gpui::Context<Self>,
    ) -> Result<bool, &'static str> {
        if !self.recent_output_enabled || self.recent_output_restore_closed {
            return Ok(false);
        }
        let Some(session) = &self.session else { return Ok(false) };
        let recent = crate::recent_output::RecentOutput::try_from_records(records)?;
        let mut term = session.term.lock();
        if term.mode().contains(TermMode::ALT_SCREEN) {
            return Ok(false);
        }
        recent.restore(term.grid_mut())?;
        // 恢复内容位于主屏之前；只移动视口，不改 ConPTY 的绝对光标坐标。
        // 新输出逐步占用主屏下方空行，输入不能提前隐藏恢复内容。
        let grid = term.grid();
        use nebula_terminal::grid::Dimensions;
        let room = grid.screen_lines().saturating_sub(grid.cursor.point.line.0 as usize + 1);
        let offset = room.min(grid.history_size());
        term.scroll_display(nebula_terminal::grid::Scroll::Bottom);
        term.scroll_display(nebula_terminal::grid::Scroll::Delta(offset as i32));
        self.restored_viewport_offset = (offset > 0).then_some(offset);
        self.recent_output = recent;
        self.recent_output_restore_closed = true;
        drop(term);
        cx.notify();
        Ok(true)
    }

    pub(super) fn follow_restored_viewport(&mut self) {
        use nebula_terminal::grid::{Dimensions, Scroll};
        let Some(_) = self.restored_viewport_offset else { return };
        let Some(session) = &self.session else { return };
        let mut term = session.term.lock();
        if term.mode().intersects(TermMode::ALT_SCREEN | TermMode::VI) {
            self.restored_viewport_offset = None;
            return;
        }
        let grid = term.grid();
        let room = grid.screen_lines().saturating_sub(grid.cursor.point.line.0 as usize + 1);
        // 恢复跟随期间用空白容量容纳历史；宽度重排可能增加历史行数，
        // 不能沿用恢复时的旧 offset，否则会截掉长命令的开头。
        let current = grid.display_offset();
        let offset = room.min(grid.history_size());
        if current != offset {
            term.scroll_display(Scroll::Delta(offset as i32 - current as i32));
        }
        self.restored_viewport_offset = (offset > 0).then_some(offset);
    }

    pub(super) fn begin_recent_output(&mut self) {
        if !self.recent_output_enabled {
            return;
        }
        let Some(session) = &self.session else { return };
        let term = session.term.lock();
        if !term.mode().intersects(TermMode::ALT_SCREEN | TermMode::VI) {
            self.recent_output_restore_closed = true;
            self.recent_output.finish(term.grid());
            self.recent_output.begin(term.grid());
        }
    }

    pub(super) fn finish_recent_output(&mut self) {
        let Some(session) = &self.session else { return };
        let term = session.term.lock();
        self.recent_output.finish(term.primary_grid());
    }

    pub(crate) fn recent_output_snapshot(&self) -> Vec<crate::recent_output::CommandRecord> {
        if !self.recent_output_enabled {
            return Vec::new();
        }
        let Some(session) = &self.session else { return Vec::new() };
        let term = session.term.lock();
        self.recent_output.snapshot(term.primary_grid())
    }
}
