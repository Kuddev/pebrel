//! GPUI 壳的窗口视效：背景模糊 / 窗口透明度 / 壁纸。
//!
//! 语义全部对齐旧壳，但**模糊的实现手段不能照抄旧壳**（见
//! [`background_appearance`]）：一律走 GPUI 自己的
//! [`WindowBackgroundAppearance`]，由平台层落到各自的原生 API。
//! - 透明度 = 壳底色与终端默认背景的 alpha（文字与彩色单元背景保持不
//!   透明，对比度不塌——旧壳 `draw_window_backdrop` 裁定）。
//! - 壁纸 = 底色之上、单元格之下的一层图（旧壳 `renderer::image` 的
//!   fit/alignment/透明度语义；图自身透明度独立于窗口 opacity）。
//!
//! 设置来源是共享层 `nebula_settings`（新增壁纸五键），解码结果按
//! (路径, 文件状态) 缓存；加载在后台串行执行，绘制只复用一张纹理。

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(windows)]
use std::sync::OnceLock;

use gpui::{
    App, Bounds, ContentMask, Corners, Hsla, IntoElement, ParentElement, Pixels, RenderImage,
    Styled, Window, WindowBackgroundAppearance, div, fill, point, px, size,
};
use image::{Frame, RgbaImage};

mod image_loader;
#[cfg(all(test, feature = "gpui-test-support"))]
mod tests;
use nebula_settings::BlurModeName;

use crate::renderer::image::{BackgroundImageAlignment, BackgroundImageFit, wallpaper_rect};

/// App-owned wallpaper loading uses the existing GPUI executor: one job and one
/// latest request, with generation checks before expensive work and publication.
pub struct VisualEffects {
    pub opacity: f32,
    pub blur: BlurModeName,
    wallpaper: Option<Wallpaper>,
    generation: Arc<AtomicU64>,
    loading: bool,
}

impl gpui::Global for VisualEffects {}

impl Drop for VisualEffects {
    fn drop(&mut self) {
        self.generation.fetch_add(1, Ordering::Release);
    }
}

struct Wallpaper {
    path: PathBuf,
    image: Option<Arc<RenderImage>>,
    stamp: Option<image_loader::FileStamp>,
    width: u32,
    height: u32,
    fit: BackgroundImageFit,
    alignment: BackgroundImageAlignment,
    cover_chrome: bool,
    opacity: f32,
}

/// Refresh prepared visual state without reading or decoding image files on the UI thread.
pub fn refresh(cx: &mut App) {
    let rt = nebula_settings::RuntimeSettings::load();
    let (opacity, blur) = cx
        .try_global::<crate::gpui_shell::config::Settings>()
        .map(|settings| (settings.visual_opacity, settings.visual_blur))
        .unwrap_or_else(|| effective_material(&rt));
    update_wallpaper(&rt, opacity, blur, cx);
    apply_window_effects(cx);
    refresh_surface_opacity(cx);
}

fn update_wallpaper(
    rt: &nebula_settings::RuntimeSettings,
    opacity: f32,
    blur: BlurModeName,
    cx: &mut App,
) {
    if !cx.has_global::<VisualEffects>() {
        cx.set_global(VisualEffects {
            opacity,
            blur,
            wallpaper: None,
            generation: Arc::new(AtomicU64::new(0)),
            loading: false,
        });
    }
    let desired = rt.background_image.as_ref().map(PathBuf::from);
    let effects = cx.global_mut::<VisualEffects>();
    effects.opacity = opacity;
    effects.blur = blur;
    let retired = if effects.wallpaper.as_ref().map(|wp| &wp.path) != desired.as_ref() {
        let retired = effects.wallpaper.take().and_then(|wp| wp.image);
        effects.wallpaper = desired.map(|path| Wallpaper {
            path,
            image: None,
            stamp: None,
            width: 1,
            height: 1,
            fit: BackgroundImageFit::default(),
            alignment: BackgroundImageAlignment::default(),
            cover_chrome: false,
            opacity: 1.0,
        });
        retired
    } else {
        None
    };
    if let Some(wp) = effects.wallpaper.as_mut() {
        wp.fit = rt
            .background_image_fit
            .as_deref()
            .and_then(BackgroundImageFit::parse)
            .unwrap_or_default();
        wp.alignment = rt
            .background_image_alignment
            .as_deref()
            .and_then(BackgroundImageAlignment::parse)
            .unwrap_or_default();
        wp.cover_chrome = rt.background_image_cover_chrome;
        wp.opacity = rt.background_image_opacity.clamp(0.0, 1.0);
    }
    effects.generation.fetch_add(1, Ordering::Release);
    retire_image(retired, cx);
    start_load(cx);
}

fn start_load(cx: &mut App) {
    let effects = cx.global_mut::<VisualEffects>();
    if effects.loading {
        return;
    }
    let Some(wp) = effects.wallpaper.as_ref() else { return };
    let request = image_loader::Request {
        path: wp.path.clone(),
        cached: wp.stamp.clone(),
        generation: effects.generation.clone(),
        version: effects.generation.load(Ordering::Acquire),
    };
    let version = request.version;
    effects.loading = true;
    let task = cx.background_executor().spawn(async move { image_loader::load(request) });
    cx.spawn(async move |cx| {
        let result = task.await;
        cx.update(|cx| {
            let Some(effects) = cx.try_global::<VisualEffects>() else { return };
            let stale = effects.generation.load(Ordering::Acquire) != version;
            cx.global_mut::<VisualEffects>().loading = false;
            if stale {
                start_load(cx);
                return;
            }
            match result {
                Ok(Some(loaded)) => {
                    let Some(wp) = cx.global_mut::<VisualEffects>().wallpaper.as_mut() else {
                        return;
                    };
                    wp.width = loaded.layout_width;
                    wp.height = loaded.layout_height;
                    wp.stamp = Some(loaded.stamp);
                    let image = Arc::new(RenderImage::new([Frame::new(loaded.pixels)]));
                    let retired = wp.image.replace(image);
                    retire_image(retired, cx);
                    refresh_surface_opacity(cx);
                    cx.refresh_windows();
                },
                Ok(None) => {},
                Err(error) => {
                    log::warn!("background image load failed: {error:?}");
                    show_load_error(error, cx);
                },
            }
        });
    })
    .detach();
}

fn refresh_surface_opacity(cx: &mut App) {
    if cx.has_global::<gpui_component::Theme>()
        && cx.has_global::<crate::gpui_shell::config::Settings>()
    {
        crate::gpui_shell::theme::reapply_prepared_surface_opacity(cx);
    }
}

fn retire_image(image: Option<Arc<RenderImage>>, cx: &mut App) {
    if let Some(image) = image {
        // Settings callbacks may have taken the current window out of App.windows.
        // Defer until all windows are back, and invalidate cached scene replay.
        cx.defer(move |cx| {
            cx.drop_image(image, None);
            cx.refresh_windows();
        });
    }
}

fn show_load_error(error: image_loader::LoadError, cx: &mut App) {
    use crate::i18n::Message;
    cx.defer(move |cx| {
        let message = match error {
            image_loader::LoadError::TooLarge => Message::WallpaperTooLarge,
            _ => Message::WallpaperLoadFailed,
        };
        let text = crate::gpui_shell::config::ui_language(cx).text(message);
        if let Some(handle) = cx.windows().first() {
            let _ = handle.update(cx, |_, window, cx| {
                crate::gpui_shell::toast::toast(
                    window,
                    cx,
                    crate::gpui_shell::toast::ToastKind::Warning,
                    text,
                );
            });
        }
    });
}

/// 当前窗口透明度（无全局时视为不透明）。
#[allow(dead_code)]
pub fn window_opacity(cx: &App) -> f32 {
    cx.try_global::<VisualEffects>().map(|v| v.opacity).unwrap_or(1.0)
}

/// 拖不透明度滑块的快路径：只把新值写进视效全局。
///
/// 不读设置文件、不重建壁纸纹理、不碰窗口级模糊——透明度只影响我们自己绘制的
/// 像素 alpha 与壳色 token，那些都是纯浪费。调用方负责紧接着调
/// [`crate::gpui_shell::theme::reapply_shell_opacity`] 与 `cx.notify()`。
pub fn set_opacity_live(opacity: f32, cx: &mut App) {
    if cx.has_global::<VisualEffects>() {
        cx.global_mut::<VisualEffects>().opacity = opacity.clamp(0.0, 1.0);
    }
}

/// Preserve the original extended-wallpaper scrim: shell and card surfaces
/// retain the user's opacity, capped at 0.78, while text remains opaque.
/// Before an image is ready (or after clearing it), use the normal surface.
pub fn chrome_surface_opacity(cx: &App) -> f32 {
    let Some(effects) = cx.try_global::<VisualEffects>() else {
        return 1.0;
    };
    if effects.wallpaper.as_ref().is_some_and(|wp| wp.cover_chrome && wp.image.is_some()) {
        return effects.opacity.clamp(0.0, 1.0).min(0.78);
    }
    effects.opacity.clamp(0.0, 1.0)
}

/// 开窗参数用。GPUI 通用层在窗口创建时就会把这个值下发到平台层
/// （`gpui::Window::new` → `platform_window.set_background_appearance`）。
/// Mica / Mica Alt 因此从首帧就走平台原生 backdrop，不再先挂一层普通透明背景。
pub fn initial_background_appearance() -> WindowBackgroundAppearance {
    let runtime = nebula_settings::RuntimeSettings::load();
    background_appearance(effective_material(&runtime).1)
}

fn effective_material(runtime: &nebula_settings::RuntimeSettings) -> (f32, BlurModeName) {
    let resolved = crate::gpui_shell::theme::ResolvedTheme::from_runtime(runtime, runtime.theme);
    (resolved.effective_opacity(runtime), resolved.effective_blur(runtime))
}

/// 模糊开关 → 窗口背景外观。**唯一落笔点**，启动与热应用共用。
///
/// # 两条 Windows 原生通道为什么要分开
///
/// Mica / Mica Alt 分别使用 GPUI 的 `MicaBackdrop` / `MicaAltBackdrop`，由平台层
/// 映射到 `DWMSBT_MAINWINDOW` / `DWMSBT_TABBEDWINDOW`。Nebula 不读取壁纸文件，
/// 也不自行猜测多显示器排布。
///
/// Aero/Acrylic 继续使用 GPUI 已验证的 AccentPolicy 通道。Aero 额外使用
/// `DwmEnableBlurBehindWindow` + 半透明深色玻璃配方；GPUI 的
/// DirectComposition 窗口上，`DWMSBT_TRANSIENTWINDOW` 会形成不透明灰板，不能
/// 因为 Mica 与它同属 system backdrop 就混用；切换档位时会显式清理另一条通道。
///
/// # 不透明度 100% 时看不到模糊是正交结果，不是失效
///
/// Acrylic 层在窗口内容**下方**。`opacity=1.00` 下我们画的像素完全不透明，
/// 模糊层被整块盖住——此时开关在画面上零变化是必然的。验收模糊必须先把
/// 不透明度调到 100% 以下，否则任何实现都会被判成"没修复"。

///
/// # 关闭材质时为什么仍是 `Transparent`
///
/// GPUI 的 Windows renderer 会按这个枚举选择清屏 alpha：`Opaque` 固定以
/// alpha=1 清空交换链，场景中后续绘制的透明像素无法把它重新变透明。因此
/// `None` 也必须保留透明交换链；紧随其后的 Windows 原生清理会关闭 WCA 与
/// DWMSBT，最终语义是“窗口可透明，但没有任何模糊材质”。
///
/// # 哪些档位要窗口保持可透
///
/// `Aero` / `Acrylic` 的材质由 DWM 画在窗口内容**下方**，内容不透就看不见，所以
/// 要 `Blurred`——这个返回值决定 GPUI 平台层预写哪套 AccentPolicy（启动即生效，
/// 不必等 [`refresh`] 补第二次），随后 [`apply_windows_accent_policy`] 覆写成
/// state 3 / state 4。
///
/// `Mica` / `Mica Alt` 在 Windows 11 22H2 起直接使用 GPUI 原生枚举；较旧系统
/// 依次回退为经典模糊或普通透明。不能回退到 `Opaque`，否则客户区像素会遮住
/// DWM 在窗口下方合成的材质。
fn background_appearance(blur: BlurModeName) -> WindowBackgroundAppearance {
    #[cfg(windows)]
    {
        match blur {
            BlurModeName::Aero | BlurModeName::Acrylic => WindowBackgroundAppearance::Blurred,
            BlurModeName::Mica if windows_build_number() >= 22_621 => {
                WindowBackgroundAppearance::MicaBackdrop
            },
            BlurModeName::MicaAlt if windows_build_number() >= 22_621 => {
                WindowBackgroundAppearance::MicaAltBackdrop
            },
            BlurModeName::Mica | BlurModeName::MicaAlt if windows_build_number() >= 17_763 => {
                WindowBackgroundAppearance::Blurred
            },
            BlurModeName::Mica | BlurModeName::MicaAlt => WindowBackgroundAppearance::Transparent,
            BlurModeName::None => WindowBackgroundAppearance::Transparent,
        }
    }
    #[cfg(not(windows))]
    {
        if blur.enabled() {
            WindowBackgroundAppearance::Blurred
        } else {
            WindowBackgroundAppearance::Transparent
        }
    }
}

/// `GetVersionEx` 会受应用兼容清单影响；RtlGetVersion 才能可靠决定公开的
/// `DWMWA_SYSTEMBACKDROP_TYPE` 是否存在。缓存结果，避免热应用时重复进内核。
#[cfg(windows)]
fn windows_build_number() -> u32 {
    static BUILD: OnceLock<u32> = OnceLock::new();
    *BUILD.get_or_init(|| {
        use windows_sys::Wdk::System::SystemServices::RtlGetVersion;
        use windows_sys::Win32::System::SystemInformation::OSVERSIONINFOW;

        let mut info: OSVERSIONINFOW = unsafe { std::mem::zeroed() };
        info.dwOSVersionInfoSize = std::mem::size_of_val(&info) as u32;
        let status = unsafe { RtlGetVersion(&mut info) };
        if status == 0 { info.dwBuildNumber } else { 0 }
    })
}

/// 已经真正落到窗口上的模糊档位。拖不透明度滑块会每帧走一遍 [`refresh`]，而
/// 窗口级材质是**跨进程**调用（`SetWindowCompositionAttribute` 两次 +
/// `DwmSetWindowAttribute`）。不做门控就等于每帧和 DWM 往返三次，滑块直接
/// 拖成幻灯片——2026-08-21 实测。档位没变时一次都不碰。
struct AppliedBlur {
    blur: BlurModeName,
    windows: HashSet<gpui::WindowId>,
}

impl gpui::Global for AppliedBlur {}

/// 把窗口层效果应用到所有窗口。透明度完全由绘制像素 alpha 控制，因此模糊
/// 开关与 0%..100% 透明度互不绑死、无需跨帧时序补丁。
///
/// # 必须 `defer`：否则热切换整条链路静默失效
///
/// 设置页开关是在**某个窗口自己的 update 回调里**点的（点击 → `toggle` →
/// `persist` → `emit(Changed)` → `on_settings_event` → `apply_runtime_settings`
/// → `apply_chrome_theme` → [`refresh`] → 这里），此时该窗口已经被
/// `App::update_window` 从 slot 里 take 出来（`gpui/src/app.rs`：
/// `cx.windows.get_mut(id)?.take()?`），对同一 handle 再 update 只会拿到
/// `Err("window not found")`。
///
/// 2026-08-21 定案：这里原先写的是 `let _ = handle.update(..)`，把那个 Err 连同
/// 整个 `set_background_appearance` 一起吞掉了——**启动时模糊有效（走
/// `WindowOptions` 的 [`initial_background_appearance`]，不经过 update），运行中
/// 点开关却完全没反应，且已经开着的 Acrylic 也关不掉**。这正是"关了还带模糊"
/// 和"不切换实时生效"的同一个根因；旧壳 winit 直接对 HWND 落 API，没有这层
/// 借用模型，所以一直是丝滑的。
///
/// [`App::defer`] 把应用推到本轮 effect cycle 末尾，那时窗口已归还 slot。
/// update 失败不再静默：留 warn，避免同一个坑第三次被当成"DWM 不生效"。
///
/// # 模糊态没变时只处理新窗口
///
/// 不透明度/壁纸改动也会走到这里，但它们只影响我们自己绘制的像素，窗口级
/// 模糊属性一个字节都不用改。透明度是滑块，一次拖拽几十上百个事件，所以这条
/// 短路是拖拽手感的必要条件，不是可选优化。多窗口下则按 `WindowId` 补应用新窗，
/// 避免全局档位相同就让第二个窗口漏掉原生 backdrop。
fn apply_window_effects(cx: &mut App) {
    // 全局缺失时按"关"处理而不是缺省档：这条路径只在极早期或异常态走到，
    // 宁可少一层材质，也不要凭空给窗口开上模糊再被 refresh 纠正一次。
    let blur = cx.try_global::<VisualEffects>().map(|v| v.blur).unwrap_or(BlurModeName::None);
    let appearance = background_appearance(blur);
    cx.defer(move |cx| {
        // 必须在 defer 后枚举：触发设置变更的窗口此时才重新放回 App 窗口表。
        let handles = cx.windows();
        let window_ids = handles.iter().map(|handle| handle.window_id()).collect::<HashSet<_>>();
        let already_applied = cx
            .try_global::<AppliedBlur>()
            .filter(|applied| applied.blur == blur)
            .map(|applied| applied.windows.clone())
            .unwrap_or_default();
        let pending = handles
            .into_iter()
            .filter(|handle| !already_applied.contains(&handle.window_id()))
            .collect::<Vec<_>>();
        cx.set_global(AppliedBlur { blur, windows: window_ids });
        for handle in pending {
            if let Err(err) = handle.update(cx, |_, window, _| {
                window.set_background_appearance(appearance);
                #[cfg(windows)]
                apply_windows_accent_policy(window, blur, appearance);
                window.refresh();
            }) {
                log::warn!("failed to apply window visual effects: {err}");
            }
        }
    });
}

/// 显式落下 Windows 材质属性。
///
/// # 五档各自写什么
///
/// | 档位 | AccentPolicy | SYSTEMBACKDROP | DWM 每帧成本 |
/// |---|---|---|---|
/// | `None` | 全零 | `DWMSBT_NONE` | 无 |
/// | `Aero` | state 3 + 玻璃色调 | `DWMSBT_NONE` | 整窗实时玻璃模糊 |
/// | `Mica` | 全零 | `DWMSBT_MAINWINDOW` | 系统壁纸 backdrop |
/// | `Mica Alt` | 全零 | `DWMSBT_TABBEDWINDOW` | 强色调系统壁纸 backdrop |
/// | `Acrylic` | state 4 + 非零 alpha | `DWMSBT_NONE` | 实时模糊 + tint/噪点/饱和 |
///
/// `Mica` / `Mica Alt` 使用公开 DWM 属性。Nebula 不读取
/// `SPI_GETDESKWALLPAPER` 或 `TranscodedWallpaper`；显示器选择、壁纸排布、模糊和
/// 色调全部交给系统合成器。Windows 不支持该属性时退回 Acrylic，避免透明空洞。
///
/// 两条通道**必须互斥**：同时开 Acrylic 与 system backdrop 时 DWM 的行为未
/// 定义（实测表现为 backdrop 赢，Acrylic 被吞）。所以每档都要把另一条显式
/// 写回中性值，不能只写自己那条。
///
/// `SetWindowCompositionAttribute` 未进入公开 SDK，所以和 GPUI 上游一样动态取
/// 函数地址；backdrop 则用公开 DWM API。任一步失败都留日志，避免把 API 失败
/// 再次误判成"设置没有热应用"。
#[cfg(windows)]
fn apply_windows_accent_policy(
    window: &Window,
    blur: BlurModeName,
    appearance: WindowBackgroundAppearance,
) {
    use windows_sys::Win32::Foundation::{BOOL, HWND};
    use windows_sys::Win32::Graphics::Dwm::{
        DWM_BB_ENABLE, DWM_BLURBEHIND, DWMSBT_MAINWINDOW, DWMSBT_NONE, DWMSBT_TABBEDWINDOW,
        DWMWA_SYSTEMBACKDROP_TYPE, DwmEnableBlurBehindWindow, DwmSetWindowAttribute,
    };
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetWindowPos,
    };
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct AccentPolicy {
        state: u32,
        flags: u32,
        gradient_color: u32,
        animation_id: u32,
    }

    #[repr(C)]
    struct WindowCompositionAttributeData {
        attribute: u32,
        data: *mut core::ffi::c_void,
        size: usize,
    }

    type SetWindowCompositionAttribute =
        unsafe extern "system" fn(HWND, *mut WindowCompositionAttributeData) -> BOOL;

    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };
    let hwnd = handle.hwnd.get() as *mut core::ffi::c_void;
    let set_attribute: Option<SetWindowCompositionAttribute> = unsafe {
        let user32 = GetModuleHandleA(c"user32.dll".as_ptr() as *const u8);
        if user32.is_null() {
            None
        } else {
            GetProcAddress(user32, c"SetWindowCompositionAttribute".as_ptr() as *const u8)
                .map(|procedure| std::mem::transmute(procedure))
        }
    };
    if set_attribute.is_none() {
        log::warn!("SetWindowCompositionAttribute is unavailable in user32.dll");
    }

    let apply_accent = |mut accent: AccentPolicy, phase: &str| {
        let Some(set_attribute) = set_attribute else { return };
        let mut data = WindowCompositionAttributeData {
            attribute: 19, // WCA_ACCENT_POLICY
            data: &mut accent as *mut _ as *mut core::ffi::c_void,
            size: std::mem::size_of::<AccentPolicy>(),
        };
        // SAFETY: hwnd 来自当前存活的 GPUI 窗口，数据在调用期间保持有效。
        if unsafe { set_attribute(hwnd, &mut data) } == 0 {
            log::warn!("SetWindowCompositionAttribute({phase}) failed");
        }
    };

    let disabled_accent = AccentPolicy {
        state: 0,
        // 与 GPUI 非 Acrylic 路径一致，清理旧材质时保留标准边框绘制语义。
        flags: 2,
        gradient_color: 0,
        animation_id: 0,
    };
    let system_material_requested = matches!(
        appearance,
        WindowBackgroundAppearance::MicaBackdrop | WindowBackgroundAppearance::MicaAltBackdrop
    );

    // 必须先移除旧 WCA 层。反过来先写 DWMSBT 时，Aero/Acrylic 的
    // AccentPolicy 会阻止 DWM 接纳新材质，事后再清也不会自动重算 frame。
    if system_material_requested {
        apply_accent(disabled_accent, "clear-before-system-backdrop");
    }

    let blur_behind = DWM_BLURBEHIND {
        dwFlags: DWM_BB_ENABLE,
        fEnable: i32::from(blur == BlurModeName::Aero),
        hRgnBlur: std::ptr::null_mut(),
        fTransitionOnMaximized: 0,
    };
    // 对所有档位都显式 enable/disable，避免从 Aero 热切换后遗留玻璃层。
    let blur_behind_result = unsafe { DwmEnableBlurBehindWindow(hwnd, &blur_behind) };
    let backdrop: i32 = match appearance {
        WindowBackgroundAppearance::MicaBackdrop => DWMSBT_MAINWINDOW,
        WindowBackgroundAppearance::MicaAltBackdrop => DWMSBT_TABBEDWINDOW,
        _ => DWMSBT_NONE,
    };
    // 公开 system-backdrop 属性仅存在于 22621+。旧系统的回退只走 WCA，
    // 不应把预期的 E_INVALIDARG 记录成运行时故障。
    let backdrop_result = (windows_build_number() >= 22_621).then(|| unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE as u32,
            &backdrop as *const _ as *const core::ffi::c_void,
            std::mem::size_of::<i32>() as u32,
        )
    });
    let system_material_available =
        system_material_requested && backdrop_result.is_some_and(|result| result >= 0);

    let accent = match blur {
        BlurModeName::Acrylic => AccentPolicy {
            state: 4, // ACCENT_ENABLE_ACRYLICBLURBEHIND
            flags: 0,
            // alpha=0 会让部分 DWM 版本直接跳过 Acrylic。
            gradient_color: 0x0100_0000,
            animation_id: 0,
        },
        // Aero 使用 Win32 公开接口组合：实时 BlurBehind + 约 60% 深色玻璃色调。
        BlurModeName::Aero => AccentPolicy {
            state: 3, // ACCENT_ENABLE_BLURBEHIND
            flags: 0,
            gradient_color: 0x982B_2B2B,
            animation_id: 0,
        },
        // 1809..22H2 回退到经典模糊；新系统若原生 backdrop 调用失败，
        // 同样保留 Acrylic 兜底。成功的系统材质不能再叠第二层 AccentPolicy。
        BlurModeName::Mica | BlurModeName::MicaAlt
            if matches!(appearance, WindowBackgroundAppearance::Blurred)
                || (system_material_requested && !system_material_available) =>
        {
            AccentPolicy { state: 4, flags: 0, gradient_color: 0x0100_0000, animation_id: 0 }
        },
        BlurModeName::Mica | BlurModeName::MicaAlt | BlurModeName::None => disabled_accent,
    };

    if !(system_material_requested && system_material_available) {
        apply_accent(accent, "final");
    }

    // 重绘 GPUI 内容不足以让 DWM 重新读取 DWMSBT。材质切换是低频操作，
    // 在 AppliedBlur 门控后刷新一次非客户区 frame，不影响透明度滑块性能。
    if backdrop_result.is_some() {
        let frame_result = unsafe {
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            )
        };
        if frame_result == 0 {
            log::warn!("failed to refresh the window frame after changing system backdrop");
        }
    }

    if let Some(backdrop_result) = backdrop_result.filter(|result| *result < 0) {
        if matches!(blur, BlurModeName::Mica | BlurModeName::MicaAlt) {
            log::warn!(
                "system {blur:?} is unavailable (HRESULT=0x{:08X}); falling back to Acrylic",
                backdrop_result as u32
            );
        } else {
            log::warn!(
                "DwmSetWindowAttribute(DWMWA_SYSTEMBACKDROP_TYPE={backdrop}) failed: HRESULT=0x{:08X}",
                backdrop_result as u32
            );
        }
    }
    if blur_behind_result < 0 {
        log::warn!(
            "DwmEnableBlurBehindWindow(enable={}) failed: HRESULT=0x{:08X}",
            blur == BlurModeName::Aero,
            blur_behind_result as u32
        );
    }
}

// ---- 以下一组只服务已停用的 [`paint_glass_overlay`]（见其文档）。按用户要求
// ---- 保留实现，因此统一标 `dead_code`，不要因为"没人用"就删掉。

/// 噪点 tile 边长（物理像素）。越大平铺次数越少、内存越高：512 时 3K 屏约
/// 28 次 `paint_image`，1MB 纹理——两头都便宜。
#[allow(dead_code)]
const NOISE_TILE_PX: u32 = 512;
/// 噪点强度。Acrylic 自带的颗粒非常细微（目测 3~5%），高于 ~8% 会从"玻璃"
/// 变成"脏"。
#[allow(dead_code)]
const NOISE_ALPHA: u8 = 14;
/// 白色 tint 浓度。暗色主题要更厚——Mica 的壁纸色调在暗色下偏沉，正是它
/// "不够透亮"的主因；亮色主题本就够亮，加太多会过曝。
#[allow(dead_code)]
const TINT_ALPHA_DARK: f32 = 0.08;
#[allow(dead_code)]
const TINT_ALPHA_LIGHT: f32 = 0.05;

thread_local! {
    /// 噪点 tile 只依赖上面几个常量，进程内生成一次即可。用 `thread_local`
    /// 而不是 `OnceLock`：`RenderImage` 只在渲染线程用，不必为跨线程共享去
    /// 背 `Send + Sync` 的约束。
    #[allow(dead_code)]
    static NOISE_TILE: std::cell::RefCell<Option<Arc<RenderImage>>> =
        const { std::cell::RefCell::new(None) };
}

/// 生成（并缓存）噪点 tile。
#[allow(dead_code)]
fn noise_tile() -> Arc<RenderImage> {
    NOISE_TILE.with(|slot| {
        if let Some(tile) = slot.borrow().as_ref() {
            return tile.clone();
        }
        let mut buffer = RgbaImage::new(NOISE_TILE_PX, NOISE_TILE_PX);
        // LCG（数值出自 Numerical Recipes）：确定性、零依赖。噪点只要"看起来
        // 随机"，不需要统计学质量；确定性还让同一台机器每次启动的颗粒一致。
        let mut state: u32 = 0x9E37_79B9;
        for pixel in buffer.chunks_exact_mut(4) {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            // 取高位：LCG 的低位周期极短，直接用会出现肉眼可见的条带。
            let luma = (state >> 24) as u8;
            // GPUI 的图像帧是预乘 alpha，颜色必须先乘进去，否则半透明噪点
            // 整体偏亮。三通道同值，所以不必加载壁纸时那样 swap 成 BGRA。
            let premultiplied = ((u16::from(luma) * u16::from(NOISE_ALPHA)) / 255) as u8;
            pixel[0] = premultiplied;
            pixel[1] = premultiplied;
            pixel[2] = premultiplied;
            pixel[3] = NOISE_ALPHA;
        }
        let tile = Arc::new(RenderImage::new([Frame::new(buffer)]));
        *slot.borrow_mut() = Some(tile.clone());
        tile
    })
}

/// 【已停用，保留作参考】Mica 档的玻璃增强层：白 tint + 噪点颗粒。
///
/// # 为什么停用
///
/// 这层的前提是"系统 Mica 已经提供了壁纸模糊，我们只补噪点与 tint"。2026-08-22
/// 实测证明前提不成立——系统 Mica 在本壳上从未生效，底下是一块纯色兜底，于是
/// 这层只是在纯色上再刷一层雾，观感上离 Mica 更远（用户判定"明显不是 Mica"）。
///
/// Mica 现在由 DWM 的系统 backdrop 完整合成，噪点与 tint 不应在客户区重复叠加，
/// 所以这层不再有调用点。代码按用户要求保留，供其他材质实验复用。
///
/// 调玻璃感只动本文件顶部那四个常量，不要改绘制顺序：tint 必须在噪点之下，
/// 否则颗粒会被 tint 冲淡到看不见。
#[allow(dead_code)]
pub fn paint_glass_overlay(bounds: Bounds<Pixels>, window: &mut Window, cx: &App) {
    let Some(effects) = cx.try_global::<VisualEffects>() else { return };
    if !matches!(effects.blur, BlurModeName::Mica | BlurModeName::MicaAlt) {
        return;
    }

    let is_light = crate::gpui_shell::theme::resolved_skin(cx).is_light;
    let tint = if is_light { TINT_ALPHA_LIGHT } else { TINT_ALPHA_DARK };
    window.paint_quad(fill(bounds, Hsla { h: 0.0, s: 0.0, l: 1.0, a: tint }));

    // 平铺而不是"按窗口尺寸生成一张大图"：后者在 3K 屏上是 25MB 纹理，且每次
    // resize 都要重新填充六百万像素——resize 是交互路径，不能挂这种活。
    //
    // `paint_image` 内部会 `bounds.scale(scale_factor)`，即入参是**逻辑**像素。
    // tile 是按物理像素生成的，所以这里必须先除以 scale_factor 才能得到 1:1
    // 的落点——否则在 3K/200% 屏上每个噪点会被 GPU 放大成 2×2 像素块，颗粒
    // 糊成噪斑（旧壳"禁 GPU 拉伸"那条清晰度铁律同源）。
    let tile = noise_tile();
    let scale = window.scale_factor().max(0.5);
    let step = px(NOISE_TILE_PX as f32 / scale);
    let right = bounds.origin.x + bounds.size.width;
    let bottom = bounds.origin.y + bounds.size.height;
    window.with_content_mask(Some(ContentMask { bounds }), |window| {
        let mut y = bounds.origin.y;
        while y < bottom {
            let mut x = bounds.origin.x;
            while x < right {
                let _ = window.paint_image(
                    Bounds::new(point(x, y), size(step, step)),
                    Bounds::new(point(x, y), size(step, step)),
                    Corners::default(),
                    tile.clone(),
                    0,
                    false,
                );
                x += step;
            }
            y += step;
        }
    });
}

/// Card-only wallpaper sits above the card background. Extended wallpaper sits
/// below the shell/card surfaces, preserving the original visible scrim.
/// Both modes reuse one image; opacity and layout never rebake its pixels.
pub fn card_layer(cx: &App) -> impl IntoElement {
    layer(false, cx)
}

pub fn window_layer(cx: &App) -> impl IntoElement {
    layer(true, cx)
}

fn layer(under_chrome: bool, cx: &App) -> impl IntoElement {
    let opacity = cx
        .try_global::<VisualEffects>()
        .and_then(|effects| effects.wallpaper.as_ref())
        .map_or(1.0, |wp| wp.opacity);
    // Canvas's style.paint does not apply element opacity in pinned GPUI;
    // Div owns that scope for its child, without baking alpha into the image.
    div().absolute().inset_0().opacity(opacity).child(
        gpui::canvas(
            |_, _, _| (),
            move |bounds, _, window, cx| paint_wallpaper(bounds, under_chrome, window, cx),
        )
        .size_full(),
    )
}

fn image_bounds(wp: &Wallpaper, anchor: Bounds<Pixels>, scale: f32) -> Bounds<Pixels> {
    let (x, y, width, height) = wallpaper_rect(
        f32::from(anchor.size.width) * scale,
        f32::from(anchor.size.height) * scale,
        wp.width as f32,
        wp.height as f32,
        wp.fit,
        wp.alignment,
    );
    Bounds::new(
        anchor.origin + point(px(x / scale), px(y / scale)),
        size(px(width / scale), px(height / scale)),
    )
}

fn paint_wallpaper(bounds: Bounds<Pixels>, under_chrome: bool, window: &mut Window, cx: &App) {
    let Some(effects) = cx.try_global::<VisualEffects>() else { return };
    let Some(wp) = effects.wallpaper.as_ref() else { return };
    // In the previous CPU crop path an offset card produced an empty overlay
    // (negative crop offsets cast to u32). Repainting the source here would
    // remove the visible scrim. Preserve that appearance directly, without
    // retaining the broken crop arithmetic or an empty card-sized bitmap.
    if wp.opacity <= 0.0 || under_chrome != wp.cover_chrome {
        return;
    }
    let Some(image) = wp.image.as_ref() else { return };
    let anchor = if wp.cover_chrome {
        Bounds::new(point(px(0.0), px(0.0)), window.viewport_size())
    } else {
        bounds
    };
    let image_bounds = image_bounds(wp, anchor, window.scale_factor().max(0.5));
    let radius = if under_chrome { px(0.0) } else { crate::gpui_shell::theme::card_radius(cx) };
    let corners = image_corners(bounds, image_bounds, radius);
    if let Err(error) = window.paint_image(bounds, image_bounds, corners, image.clone(), 0, false) {
        log::warn!("background image paint failed: {error}");
    }
}

fn image_corners(bounds: Bounds<Pixels>, image: Bounds<Pixels>, radius: Pixels) -> Corners<Pixels> {
    // GPUI rounds the visible intersection. Interior letterbox edges are square;
    // only corners shared with the card inherit its radius.
    let left = image.left() <= bounds.left();
    let right = image.right() >= bounds.right();
    let top = image.top() <= bounds.top();
    let bottom = image.bottom() >= bounds.bottom();
    Corners {
        top_left: if left && top { radius } else { px(0.0) },
        top_right: if right && top { radius } else { px(0.0) },
        bottom_left: if left && bottom { radius } else { px(0.0) },
        bottom_right: if right && bottom { radius } else { px(0.0) },
    }
}
