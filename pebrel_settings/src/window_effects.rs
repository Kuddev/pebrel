//! Window material preferences and their opacity compatibility contract.

/// 窗口背景模糊材质：关 / Mica 系列 / Aero / Acrylic。
///
/// 五档对应不同的 DWM 成本模型，落点在
/// `gpui_shell::wallpaper::apply_windows_accent_policy`：
/// - `None`：AccentPolicy 全零 + `DWMSBT_NONE`，透明交换链直接按内容 alpha 合成。
/// - `Mica` / `MicaAlt`：系统合成器提供壁纸 backdrop、模糊和色调，
///   Nebula 不读取壁纸文件，也不在客户区仿画材质；不逐帧采样其他窗口。
/// - `Aero`：实时 BlurBehind + 半透明深色玻璃色调。
/// - `Acrylic`：`ACCENT_ENABLE_ACRYLICBLURBEHIND`。DWM 对窗口后方**实时
///   内容**逐帧高斯模糊 + 噪点 + 饱和度。观感最好，也是唯一能透出后方其他
///   窗口的档位；2026-08-22 实测这一档把 dwm.exe 顶到 28% 均值。
///
/// # 缺省为什么是 Mica 而不是 Acrylic
///
/// 2026-08-22 用户实测：3K@165Hz 下 Acrylic 档 `nebula.exe` 22.7% +
/// `dwm.exe` 28.3%，关掉模糊后卡顿立刻好转。默认档必须是性能安全的那个，
/// 想要实时透视的用户显式选高质量。
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
/// 窗口背景材质。**按 DWM 每帧成本递增排列**，不是质量递进——五者是五套成本
/// 模型，不存在"越靠后越好"：
///
/// - `None`：无材质。窗口按不透明度直接透出后方内容，不模糊。
/// - `Mica`：Windows 系统壁纸 backdrop；由 DWM 合成，不含后方其他窗口。
/// - `MicaAlt`：Mica 的更强色调变体，适合带标签栏的窗口。
/// - `Aero`：实时模糊窗口**后方的真实内容**，并叠加 Win32 深色玻璃色调。
/// - `Acrylic`：实时模糊 + tint/噪点/饱和度，最贵。
pub enum BlurModeName {
    #[default]
    None,
    Mica,
    MicaAlt,
    Aero,
    Acrylic,
}

impl BlurModeName {
    pub fn from_settings(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "none" | "off" | "0" | "false" | "no" => Some(Self::None),
            "mica" => Some(Self::Mica),
            "mica-alt" => Some(Self::MicaAlt),
            "aero" | "blurbehind" => Some(Self::Aero),
            "acrylic" => Some(Self::Acrylic),
            // 旧壳的布尔开关：`blur=1` 只表达"我要模糊"，并不表达要哪种材质。
            // 迁到 Mica 而非 Acrylic——本次改动的初衷就是降 GPU 占用，把存量
            // 用户留在最贵的那一档等于没修。想要实时透视的人显式改成 aero。
            "1" | "true" | "yes" | "on" => Some(Self::Mica),
            _ => None,
        }
    }

    pub fn settings_value(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Mica => "mica",
            Self::MicaAlt => "mica-alt",
            Self::Aero => "aero",
            Self::Acrylic => "acrylic",
        }
    }

    /// 是否需要窗口内容保留透明像素（除 `None` 外都要，否则材质被自己盖住）。
    pub fn enabled(self) -> bool {
        !matches!(self, Self::None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RawSettings, RuntimeSettings};

    #[test]
    fn blur_mode_parses_mica_alt_and_migrates_legacy_bool() {
        assert_eq!(BlurModeName::from_settings("none"), Some(BlurModeName::None));
        assert_eq!(BlurModeName::from_settings("off"), Some(BlurModeName::None));
        assert_eq!(BlurModeName::from_settings("mica"), Some(BlurModeName::Mica));
        assert_eq!(BlurModeName::from_settings("mica-alt"), Some(BlurModeName::MicaAlt));
        assert_eq!(BlurModeName::from_settings("aero"), Some(BlurModeName::Aero));
        // 未公开 API 的名字（`ACCENT_ENABLE_BLURBEHIND`）也认，手改配置的人
        // 可能照着实现名写。
        assert_eq!(BlurModeName::from_settings("blurbehind"), Some(BlurModeName::Aero));
        assert_eq!(BlurModeName::from_settings("acrylic"), Some(BlurModeName::Acrylic));
        assert_eq!(BlurModeName::from_settings("frosted"), None);
        // 大小写与空白由 from_settings 归一（设置文件是手改的）。
        assert_eq!(BlurModeName::from_settings("  Acrylic "), Some(BlurModeName::Acrylic));
        assert_eq!(BlurModeName::from_settings(" AERO "), Some(BlurModeName::Aero));

        // 旧壳布尔值迁移：关保持关，开落到 Mica（不是最贵的 Acrylic）。
        assert_eq!(BlurModeName::from_settings("0"), Some(BlurModeName::None));
        assert_eq!(BlurModeName::from_settings("false"), Some(BlurModeName::None));
        assert_eq!(BlurModeName::from_settings("1"), Some(BlurModeName::Mica));
        assert_eq!(BlurModeName::from_settings("true"), Some(BlurModeName::Mica));
        assert_eq!(
            RuntimeSettings::from_raw(&RawSettings::from_text("blur=1\nopacity=1.00\n")).opacity,
            1.0
        );
        assert_eq!(
            RuntimeSettings::from_raw(&RawSettings::from_text("blur=mica\nopacity=1.00\n")).opacity,
            1.0
        );

        // 认不出的值回落到默认关闭，不主动启用系统材质。
        assert_eq!(
            RuntimeSettings::from_raw(&RawSettings::from_text("blur=frosted\n")).blur,
            BlurModeName::None
        );
        assert_eq!(
            RuntimeSettings::from_raw(&RawSettings::from_text("blur=acrylic\n")).blur,
            BlurModeName::Acrylic
        );

        // settings_value 与 from_settings 必须互为逆运算，否则设置页存盘后
        // 重开会掉档。
        for mode in [
            BlurModeName::None,
            BlurModeName::Mica,
            BlurModeName::MicaAlt,
            BlurModeName::Aero,
            BlurModeName::Acrylic,
        ] {
            assert_eq!(BlurModeName::from_settings(mode.settings_value()), Some(mode));
        }

        assert!(!BlurModeName::None.enabled());
        assert!(BlurModeName::Mica.enabled());
        assert!(BlurModeName::MicaAlt.enabled());
        assert!(BlurModeName::Aero.enabled());
        assert!(BlurModeName::Acrylic.enabled());
    }

    #[test]
    fn opacity_defaults_to_opaque_and_preserves_explicit_values_for_every_material() {
        for blur in ["none", "mica", "mica-alt", "aero", "acrylic", "1", "true"] {
            let raw = format!("blur={blur}\n");
            let runtime = RuntimeSettings::from_raw(&RawSettings::from_text(&raw));
            assert_eq!(runtime.opacity, 1.0, "{blur}");
            assert_eq!(Some(runtime.blur), BlurModeName::from_settings(blur));
            for opacity in [0.0, 0.65, 1.0] {
                let raw = format!("blur={blur}\nopacity={opacity}\n");
                let runtime = RuntimeSettings::from_raw(&RawSettings::from_text(&raw));
                assert_eq!(runtime.opacity, opacity, "{raw}");
            }
        }
        let invalid =
            RuntimeSettings::from_raw(&RawSettings::from_text("blur=unknown\nopacity=invalid\n"));
        assert_eq!(invalid.blur, BlurModeName::None);
        assert_eq!(invalid.opacity, 1.0);
    }
}
