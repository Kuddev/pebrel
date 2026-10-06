#[path = "../build/i18n.rs"]
mod catalog_builder;
#[path = "../src/i18n/mod.rs"]
mod i18n;
#[path = "../src/display/side_panel/notice.rs"]
mod panel_notice;
#[path = "../src/gpui_shell/workspace/vcs_panel/relative_time.rs"]
mod vcs_relative_time;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::hint::black_box;

thread_local! {
    static TRACK_ALLOCATIONS: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

struct CountingAllocator;

fn record_allocation() {
    let _ = TRACK_ALLOCATIONS.try_with(|tracking| {
        if tracking.get() {
            let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
        }
    });
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record_allocation();
        unsafe { System.realloc(pointer, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[test]
fn first_and_repeated_translation_lookups_allocate_nothing() {
    ALLOCATIONS.with(|count| count.set(0));
    TRACK_ALLOCATIONS.with(|tracking| tracking.set(true));
    for language in i18n::UiLanguage::ALL {
        for _ in 0..1_000 {
            black_box(language.text(black_box(i18n::Message::SettingsSidebarNetwork)));
            black_box(language.text(black_box(i18n::Message::VcsChanges)));
            black_box(language.text(black_box(i18n::Message::VcsCommitPlaceholder)));
            black_box(language.tr(black_box("settings.sidebar.network")));
            black_box(language.pick(black_box("网络"), black_box("Network")));
            black_box(language.pick(black_box("新文案"), black_box("Unmigrated text")));
        }
    }
    TRACK_ALLOCATIONS.with(|tracking| tracking.set(false));
    assert_eq!(ALLOCATIONS.with(Cell::get), 0);
}

#[test]
fn translation_lookup_fits_a_small_stack() {
    const CHILD: &str = "PEBREL_I18N_SMALL_STACK_CHILD";
    if std::env::var_os(CHILD).is_some() {
        std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(|| {
                for language in i18n::UiLanguage::ALL {
                    assert!(
                        !black_box(language).text(black_box(i18n::Message::VcsChanges)).is_empty()
                    );
                    assert!(!black_box(language).tr(black_box("vcs.changes")).is_empty());
                    assert!(
                        !black_box(language)
                            .pick(black_box("紫罗兰"), black_box("Violet"))
                            .is_empty()
                    );
                }
            })
            .unwrap()
            .join()
            .unwrap();
        return;
    }
    // A stack overflow aborts the process. Isolate the regression so a failure
    // reports the child status instead of taking down the whole contract suite.
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "translation_lookup_fits_a_small_stack", "--nocapture"])
        .env(CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "translation lookup exceeded a 64 KiB stack: {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn embedded_translations_stay_within_the_initial_payload_budget() {
    assert!(i18n::TRANSLATED_BYTES < 256 * 1024);
    assert!(i18n::MESSAGE_COUNT >= 200);
}

#[test]
fn vcs_messages_follow_resolved_english_and_fall_back_for_partial_locales() {
    let english = i18n::UiLanguage::for_locale(Some("en-GB"));
    assert_eq!(english.tr("vcs.changes"), "Changes");
    assert_eq!(english.tr("vcs.commit_placeholder"), "Commit message...");
    assert_eq!(i18n::UiLanguage::ZhCn.tr("vcs.changes"), "变更");
    assert_eq!(i18n::UiLanguage::FrFr.tr("vcs.changes"), "Changes");
    assert_eq!(english.tr_args("vcs.refresh_status", &[("vcs", "Git")]), "Refresh Git status");
}

#[test]
fn background_messages_keep_chinese_text_through_catalog_generation() {
    let catalog: serde_json::Value =
        serde_json::from_str(include_str!("../i18n/zh-CN.json")).unwrap();
    for (key, value) in catalog["wallpaper"].as_object().unwrap() {
        let text = value.as_str().unwrap();
        // 问号替换仍是合法 UTF-8/JSON，结构校验单独通过也不能证明中文未损坏。
        assert!(text.chars().any(|ch| ('\u{3400}'..='\u{9fff}').contains(&ch)), "{key}");
        assert!(!text.contains("??") && !text.contains('\u{fffd}'), "{key}");
    }
    assert_eq!(i18n::UiLanguage::ZhCn.text(i18n::Message::WallpaperShaderSource), "WGSL 文件");
    assert_eq!(i18n::UiLanguage::ZhCn.text(i18n::Message::WallpaperGif), "GIF 动图");
    assert_eq!(i18n::UiLanguage::ZhCn.text(i18n::Message::WallpaperMediaSelecting), "选择中…");
}
