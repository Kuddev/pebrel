//! UI 的西文和中文回退独立于终端字体组；中文只复用本机字库。

use std::borrow::Cow;

use gpui::{App, Font, FontFallbacks, Global, font};
use gpui_component::ActiveTheme as _;

pub(super) const UI_FONT_FAMILY: &str = "Geist";
const UI_FONT_BYTES: &[u8] = include_bytes!("../../../assets/fonts/GeistVF.ttf");

struct UiCjkFallbacks(Option<FontFallbacks>);
impl Global for UiCjkFallbacks {}

pub(super) fn init(cx: &mut App) {
    if let Err(error) = cx.text_system().add_fonts(vec![Cow::Borrowed(UI_FONT_BYTES)]) {
        super::try_write_stderr(format_args!("[pebrel:gpui] cannot register Geist: {error}"));
    }
    // 启动时只读取一次字体族名，不在 render 或鼠标事件里遍历系统字体。
    let installed = cx.text_system().all_font_names();
    let preferred = [
        "HarmonyOS Sans SC",
        "HarmonyOS Sans",
        "MiSans",
        "Source Han Sans SC",
        "思源黑体",
        "Noto Sans CJK SC",
        "Noto Sans SC",
        "PingFang SC",
        "Microsoft YaHei UI",
        "WenQuanYi Micro Hei",
    ];
    let families: Vec<String> = preferred
        .iter()
        .filter_map(|candidate| {
            installed.iter().find(|name| name.eq_ignore_ascii_case(candidate)).cloned()
        })
        .collect();
    cx.set_global(UiCjkFallbacks(
        (!families.is_empty()).then(|| FontFallbacks::from_fonts(families)),
    ));
}

pub(super) fn ui_font(cx: &App) -> Font {
    let mut result = font(cx.theme().font_family.clone());
    result.fallbacks = cx.try_global::<UiCjkFallbacks>().and_then(|fonts| fonts.0.clone());
    result
}
