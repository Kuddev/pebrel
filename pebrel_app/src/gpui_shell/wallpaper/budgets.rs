//! 视频、背景 shader 和终端后处理共享同一组惰性预算，避免每种效果另开额度。
use gpui::{App, StreamImageBudget, StreamImageBudgets};
use std::sync::Arc;
pub(in crate::gpui_shell::wallpaper) struct GlobalBudgets {
    pub(in crate::gpui_shell::wallpaper) cpu: Arc<StreamImageBudget>,
    pub(in crate::gpui_shell::wallpaper) gpu: Arc<StreamImageBudget>,
    pub(in crate::gpui_shell::wallpaper) decoder: Arc<StreamImageBudget>,
}
impl gpui::Global for GlobalBudgets {}

pub(in crate::gpui_shell::wallpaper) fn gpu_budget(cx: &mut App) -> Arc<StreamImageBudget> {
    if !cx.has_global::<GlobalBudgets>() {
        cx.set_global(GlobalBudgets {
            cpu: StreamImageBudget::with_allocation_limit(96 * 1024 * 1024, 16),
            gpu: StreamImageBudget::new(64 * 1024 * 1024),
            decoder: StreamImageBudget::with_allocation_limit(1, 1),
        });
    }
    cx.global::<GlobalBudgets>().gpu.clone()
}

struct CompileBudget(Arc<StreamImageBudget>);
impl gpui::Global for CompileBudget {}

pub(in crate::gpui_shell::wallpaper) fn compiler_budget(cx: &mut App) -> StreamImageBudgets {
    if !cx.has_global::<CompileBudget>() {
        cx.set_global(CompileBudget(StreamImageBudget::with_allocation_limit(1, 1)));
    }
    StreamImageBudgets::new(
        StreamImageBudget::with_allocation_limit(1, 1),
        cx.global::<CompileBudget>().0.clone(),
    )
}
