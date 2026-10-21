//! Per-App observations of snapshots actually consumed by TerminalElement paint.

use std::cell::RefCell;
use std::collections::HashMap;

use gpui::{App, Bounds, Entity, EntityId, Global, Pixels, Point, Size, size};
use nebula_terminal::render::RenderSnapshot;

use super::{TermLayout, TerminalView};

#[derive(Clone)]
pub(in crate::gpui_shell) struct PaintedSnapshot {
    pub count: usize,
    pub text: String,
    pub grid: (u16, u16),
    pub cursor: Option<(u16, u16)>,
    pub cursor_visible: bool,
    pub selection: Vec<(u16, u16, u16)>,
    pub origin: Point<Pixels>,
    pub cell_size: Size<Pixels>,
}

#[derive(Default)]
struct CaptureProbe(RefCell<HashMap<EntityId, PaintedSnapshot>>);
impl Global for CaptureProbe {}

pub(in crate::gpui_shell) fn enable(cx: &mut App) {
    cx.set_global(CaptureProbe::default());
}

pub(in crate::gpui_shell) fn last(entity: EntityId, cx: &App) -> PaintedSnapshot {
    cx.global::<CaptureProbe>().0.borrow().get(&entity).expect("terminal was painted").clone()
}

pub(super) fn record(
    view: &Entity<TerminalView>,
    snapshot: &RenderSnapshot,
    bounds: Bounds<Pixels>,
    layout: &TermLayout,
    cx: &App,
) {
    let Some(probe) = cx.try_global::<CaptureProbe>() else { return };
    let mut observations = probe.0.borrow_mut();
    let count = observations.get(&view.entity_id()).map_or(1, |previous| previous.count + 1);
    observations.insert(
        view.entity_id(),
        PaintedSnapshot {
            count,
            text: snapshot
                .segments
                .iter()
                .flat_map(|segment| &segment.cells)
                .map(|cell| cell.text.as_str())
                .collect(),
            grid: (snapshot.rows, snapshot.cols),
            cursor: snapshot.cursor.as_ref().map(|cursor| (cursor.row, cursor.col)),
            cursor_visible: view.read(cx).cursor_visible(),
            selection: snapshot
                .selection_runs
                .iter()
                .map(|run| (run.row, run.start, run.end))
                .collect(),
            origin: bounds.origin,
            cell_size: size(layout.cell_width, layout.line_height),
        },
    );
}
