//! 原生窗口材质适配；窗口枚举和热应用生命周期仍由界面拥有。
use gpui::{Window, WindowBackgroundAppearance};
use nebula_settings::BlurModeName;
#[cfg(windows)]
use std::sync::OnceLock;

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
/// 不必等 [`crate::gpui_shell::wallpaper::refresh`] 补第二次），随后 [`apply_windows_accent_policy`] 覆写成
/// state 3 / state 4。
///
/// `Mica` / `Mica Alt` 在 Windows 11 22H2 起直接使用 GPUI 原生枚举；较旧系统
/// 依次回退为经典模糊或普通透明。不能回退到 `Opaque`，否则客户区像素会遮住
/// DWM 在窗口下方合成的材质。
pub(crate) fn background_appearance(blur: BlurModeName) -> WindowBackgroundAppearance {
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
/// | `Acrylic` | controller 成功时清零，否则 state 4 | `DWMSBT_NONE` | 实时 Acrylic |
///
/// `Mica` / `Mica Alt` 使用公开 DWM 属性。Nebula 不读取
/// `SPI_GETDESKWALLPAPER` 或 `TranscodedWallpaper`；显示器选择、壁纸排布、模糊和
/// 色调全部交给系统合成器。Windows 不支持该属性时退回 Acrylic，避免透明空洞。
/// Acrylic 在 Windows 11 22H2+ 优先使用可用的 DesktopAcrylicController；缺少
/// Windows App Runtime 或绑定失败时保留 AccentPolicy 回退。控制器只随窗口和
/// 材质档位变化创建/释放，不跟随透明度滑块或焦点重建。
///
/// 两条通道**必须互斥**：同时开 Acrylic 与 system backdrop 时 DWM 的行为未
/// 定义（实测表现为 backdrop 赢，Acrylic 被吞）。所以每档都要把另一条显式
/// 写回中性值，不能只写自己那条。
///
/// `SetWindowCompositionAttribute` 未进入公开 SDK，所以和 GPUI 上游一样动态取
/// 函数地址；backdrop 则用公开 DWM API。任一步失败都留日志，避免把 API 失败
/// 再次误判成"设置没有热应用"。
#[cfg(windows)]
pub(crate) fn apply_windows_accent_policy(
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
        let Some(set_attribute) = set_attribute else { return false };
        let mut data = WindowCompositionAttributeData {
            attribute: 19, // WCA_ACCENT_POLICY
            data: &mut accent as *mut _ as *mut core::ffi::c_void,
            size: std::mem::size_of::<AccentPolicy>(),
        };
        // SAFETY: hwnd 来自当前存活的 GPUI 窗口，数据在调用期间保持有效。
        if unsafe { set_attribute(hwnd, &mut data) } == 0 {
            log::warn!("SetWindowCompositionAttribute({phase}) failed");
            return false;
        }
        true
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
    let accent_cleared = if system_material_requested || blur == BlurModeName::Acrylic {
        apply_accent(disabled_accent, "clear-before-system-backdrop")
    } else {
        false
    };

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

    let acrylic_requested = blur == BlurModeName::Acrylic
        || (matches!(blur, BlurModeName::Mica | BlurModeName::MicaAlt)
            && !system_material_available
            && appearance != WindowBackgroundAppearance::Transparent);
    let controller_available = acrylic_requested
        && windows_build_number() >= 22_621
        && accent_cleared
        && backdrop_result.is_some_and(|result| result >= 0)
        && blur_behind_result >= 0
        && crate::platform::acrylic::apply(window.window_handle().window_id(), hwnd as isize);

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

    if !controller_available && !(system_material_requested && system_material_available) {
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

#[cfg(not(windows))]
pub(crate) fn apply_windows_accent_policy(
    _: &Window,
    _: BlurModeName,
    _: WindowBackgroundAppearance,
) {
    // 其它平台的材质完全由 GPUI 的 background appearance 通道处理。
}
