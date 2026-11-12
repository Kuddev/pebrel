//! 同一终端锁内获取可见网格和图案必要的相邻源行，统一复用渲染快照语义。
use super::*;

pub(super) struct FrameSnapshot {
    pub snap: RenderSnapshot,
    pub art_source: Option<(i64, RenderSnapshot)>,
    pub prompt_line: Option<String>,
    pub history: usize,
    pub dashed: super::super::osc_links::LinkCells,
    pub viewport_top_abs: i64,
    pub mode: TermMode,
    pub reading_epoch: u64,
}

impl TerminalElement {
    /// 在一次锁内通过渲染合同截取快照；锁外解析颜色与绘制。第三项是回滚
    /// 历史行数——滚动条拇指的分母，顺着这一次锁一起取回，免得绘制路径
    /// 为了它再抢一次 `Term` 锁。
    pub(super) fn snapshot(&self, rows: usize, cols: usize, cx: &App) -> Option<FrameSnapshot> {
        let view = self.view.read(cx);
        let session = view.session.as_ref()?;
        let hint_config = view.hint_config.clone();
        let term = session.term.lock();
        let prompt_line = if term.mode().intersects(TermMode::ALT_SCREEN | TermMode::VI) {
            None
        } else {
            let cursor = term.grid().cursor.point;
            crate::display::nebula_prompt_line_from_raw_grid(
                &term,
                cursor,
                &view.suggest.line_buf,
                &view.suggest.suggest_env,
            )
            .or_else(|| {
                if !view.suggest.completion_popup_requested {
                    return None;
                }
                view.suggest.completion_prefix_from_raw_grid(&term, cursor)
            })
            .map(|line| line.input)
        };
        // 分段只反映内容与宽度类，绝不掺入光标状态：per-cell 绘制下反色只是
        // 换色，若让闪烁相位改变分段，整行会随闪烁重塑形而跳字。
        let projection = view.native_results.projection.borrow();
        let source_rows = projection.source_rows(rows);
        let snapshot = RenderSnapshot::capture_rows(
            &term,
            &SnapshotConfig { rows: source_rows as u16, cols: cols as u16 },
            |row| !projection.covers(row),
        );
        let history = term.history_size();
        let dashed = super::super::osc_links::dashed_cells_where(
            &term,
            &hint_config,
            source_rows,
            cols,
            |row| !projection.covers(row),
        );
        let scrollback_floor = term.grid().scrolled_out();
        let image_anchor = scrollback_floor.saturating_add(history) as i64;
        let viewport_top_abs = image_anchor + i64::from(term.viewport_origin_for(rows).0);

        let mode = *term.mode();
        let reading_epoch = term.reading_epoch();
        let identity = (
            std::sync::Arc::as_ptr(&session.term) as usize,
            reading_epoch,
            mode.contains(TermMode::ALT_SCREEN),
            cols as u16,
        );
        let retained = term.reading_span();
        let art_source = view
            .character_art
            .capture_rows(
                identity,
                viewport_top_abs,
                source_rows,
                retained.start.line as i64..retained.end.line as i64,
                &snapshot,
            )
            .map(|range| {
                let source = RenderSnapshot::capture_from(
                    &term,
                    &SnapshotConfig { rows: (range.end - range.start) as u16, cols: cols as u16 },
                    nebula_terminal::index::Line((range.start - image_anchor) as i32),
                );
                (range.start - viewport_top_abs, source)
            });
        Some(FrameSnapshot {
            snap: snapshot,
            art_source,
            prompt_line,
            history,
            dashed,
            viewport_top_abs,
            mode,
            reading_epoch,
        })
    }
}
