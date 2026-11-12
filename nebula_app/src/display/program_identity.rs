//! Program identity, icons, and logo preparation shared by both UI shells.

use super::command_completion::extract_program;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AiLogo {
    Claude,
    OpenAi,
    OpenCode,
    Pi,
    Grok,
    Antigravity,
    Trae,
    OhMyPi,
    CodeBuddy,
    Kimi,
}

impl AiLogo {
    pub(crate) const ALL: [Self; 10] = [
        Self::Claude,
        Self::OpenAi,
        Self::OpenCode,
        Self::Pi,
        Self::Grok,
        Self::Antigravity,
        Self::Trae,
        Self::OhMyPi,
        Self::CodeBuddy,
        Self::Kimi,
    ];

    /// 每种标志只嵌入一个无损压缩的矢量源，两个壳按当前 DPI 共用它。
    pub(crate) fn svg(self) -> &'static [u8] {
        match self {
            Self::Claude => include_bytes!("../../../extra/logo/ai_claude.svgz"),
            Self::OpenAi => include_bytes!("../../assets/icons/nebula-agent-openai.svgz"),
            Self::OpenCode => include_bytes!("../../assets/icons/nebula-agent-opencode.svgz"),
            Self::Pi => include_bytes!("../../assets/icons/nebula-agent-pi.svgz"),
            Self::Grok => include_bytes!("../../assets/icons/nebula-agent-grok.svgz"),
            Self::Antigravity => include_bytes!("../../../extra/logo/ai_antigravity.svgz"),
            Self::Trae => include_bytes!("../../../extra/logo/ai_trae.svgz"),
            Self::OhMyPi => include_bytes!("../../../extra/logo/ai_omp.svgz"),
            Self::CodeBuddy => include_bytes!("../../../extra/logo/ai_codebuddy.svgz"),
            Self::Kimi => include_bytes!("../../../extra/logo/ai_kimi.svgz"),
        }
    }

    pub(crate) fn render(self, ink: [u8; 3], size: u32) -> Option<(Vec<u8>, u32, u32)> {
        let size = size.clamp(1, 512);
        let source_ink = if matches!(self, Self::OpenCode) { [255; 3] } else { ink };
        let mut pixels = rasterize_svg(self.svg(), source_ink, size * 4)?;
        self.tint_pixels(&mut pixels, ink);
        Some(prepare_ai_logo_texture(&pixels, size * 4, size * 4, size))
    }

    pub(crate) fn render_brand(svg: &[u8], ink: [u8; 3], size: u32) -> Option<(Vec<u8>, u32, u32)> {
        let size = size.clamp(1, 512);
        let pixels = rasterize_svg(svg, ink, size * 4)?;
        Some(prepare_ai_logo_texture(&pixels, size * 4, size * 4, size))
    }

    /// 历史原图只留在测试中核对来源，产品不再嵌入 PNG。
    #[cfg(test)]
    pub(crate) fn png(self, light_ink: bool) -> &'static [u8] {
        match self {
            Self::Claude => include_bytes!("../../../extra/logo/ai_claude.png"),
            Self::OpenAi => include_bytes!("../../../extra/logo/ai_openai.png"),
            Self::OpenCode => include_bytes!("../../../extra/logo/ai_opencode.png"),
            Self::Pi => include_bytes!("../../../extra/logo/ai_pi.png"),
            Self::Grok if light_ink => include_bytes!("../../../extra/logo/ai_grok_light.png"),
            Self::Grok => include_bytes!("../../../extra/logo/ai_grok_dark.png"),
            Self::Antigravity => include_bytes!("../../../extra/logo/ai_antigravity.png"),
            Self::Trae => include_bytes!("../../../extra/logo/ai_trae.png"),
            Self::OhMyPi => include_bytes!("../../../extra/logo/ai_omp.png"),
            Self::CodeBuddy => include_bytes!("../../../extra/logo/ai_codebuddy.png"),
            Self::Kimi => include_bytes!("../../../extra/logo/ai_kimi.png"),
        }
    }

    fn tint_preserves_luma(self) -> Option<bool> {
        match self {
            Self::OpenAi | Self::Pi => Some(false),
            // OpenCode stores a luma map: the frame is white, the inner block gray.
            // Kimi's letterform is solid white with a small blue accent: embedded
            // unchanged it vanishes on light themes, so it rides the luma map too —
            // the K tracks the theme ink at full strength, the accent dims with it.
            Self::OpenCode | Self::Kimi => Some(true),
            Self::Claude
            | Self::Grok
            | Self::Antigravity
            | Self::Trae
            | Self::OhMyPi
            | Self::CodeBuddy => None,
        }
    }

    /// Reuse a prepared texture only when neither its source nor tint depends on theme.
    pub(crate) fn shares_theme_texture(self) -> bool {
        matches!(
            self,
            Self::Claude | Self::Antigravity | Self::Trae | Self::OhMyPi | Self::CodeBuddy
        )
    }

    pub(crate) fn tint_pixels(self, pixels: &mut [u8], ink: [u8; 3]) {
        let Some(preserve_luma) = self.tint_preserves_luma() else { return };
        for pixel in pixels.chunks_exact_mut(4) {
            let luma = if preserve_luma { u16::from(pixel[0]) } else { 255 };
            for channel in 0..3 {
                pixel[channel] = (u16::from(ink[channel]) * luma / 255) as u8;
            }
        }
    }
}

/// 保留 SVG 的原色，currentColor 则由使用位置的主题文字色提供。
fn rasterize_svg(svg: &[u8], ink: [u8; 3], size: u32) -> Option<Vec<u8>> {
    let options = resvg::usvg::Options {
        default_size: resvg::usvg::Size::from_wh(24.0, 24.0)?,
        style_sheet: Some(format!("svg {{ color: rgb({}, {}, {}); }}", ink[0], ink[1], ink[2])),
        ..Default::default()
    };
    let tree = resvg::usvg::Tree::from_data(svg, &options).ok()?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size)?;
    let scale = size as f32 / tree.size().width().max(tree.size().height());
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let mut pixels = pixmap.data().to_vec();
    // tiny-skia 使用预乘 alpha，纹理接口要求 straight RGBA。
    for pixel in pixels.chunks_exact_mut(4) {
        let alpha = u32::from(pixel[3]);
        if alpha > 0 {
            for channel in &mut pixel[..3] {
                *channel = ((u32::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
    }
    Some(pixels)
}

pub(crate) fn prepare_ai_logo_texture(
    rgba: &[u8],
    width: u32,
    height: u32,
    target_size: u32,
) -> (Vec<u8>, u32, u32) {
    use image::imageops::FilterType;

    let target_size = target_size.max(1);
    let source = image::RgbaImage::from_raw(width, height, rgba.to_vec())
        .expect("decoded logo dimensions must match its RGBA buffer");
    let resized = image::imageops::resize(&source, target_size, target_size, FilterType::Lanczos3);
    let pixels = resized.as_raw();
    let (mass, weighted_x, weighted_y) = pixels.chunks_exact(4).enumerate().fold(
        (0.0, 0.0, 0.0),
        |(mass, weighted_x, weighted_y), (index, pixel)| {
            let alpha = f64::from(pixel[3]);
            let x = (index as u32 % target_size) as f64 + 0.5;
            let y = (index as u32 / target_size) as f64 + 0.5;
            (mass + alpha, weighted_x + x * alpha, weighted_y + y * alpha)
        },
    );
    if mass == 0.0 {
        return (resized.into_raw(), target_size, target_size);
    }

    let center = f64::from(target_size) / 2.0;
    let shift_x = (center - weighted_x / mass).round() as i32;
    let shift_y = (center - weighted_y / mass).round() as i32;
    if shift_x == 0 && shift_y == 0 {
        return (resized.into_raw(), target_size, target_size);
    }

    let mut centered = vec![0; pixels.len()];
    for y in 0..target_size as i32 {
        for x in 0..target_size as i32 {
            let dest_x = x + shift_x;
            let dest_y = y + shift_y;
            if !(0..target_size as i32).contains(&dest_x)
                || !(0..target_size as i32).contains(&dest_y)
            {
                continue;
            }
            let source_index = ((y as u32 * target_size + x as u32) * 4) as usize;
            let dest_index = ((dest_y as u32 * target_size + dest_x as u32) * 4) as usize;
            centered[dest_index..dest_index + 4]
                .copy_from_slice(&pixels[source_index..source_index + 4]);
        }
    }
    (centered, target_size, target_size)
}

pub(crate) fn ai_logo_for_program(program: &str) -> Option<AiLogo> {
    use crate::ai_agents::AgentKind;

    let normalized =
        extract_program(program).unwrap_or_else(|| program.trim().to_ascii_lowercase());
    match AgentKind::parse(&normalized)? {
        AgentKind::Claude => Some(AiLogo::Claude),
        AgentKind::Codex => Some(AiLogo::OpenAi),
        AgentKind::OpenCode => Some(AiLogo::OpenCode),
        AgentKind::Pi => Some(AiLogo::Pi),
        AgentKind::Grok => Some(AiLogo::Grok),
        AgentKind::Antigravity => Some(AiLogo::Antigravity),
        AgentKind::Trae => Some(AiLogo::Trae),
        AgentKind::OhMyPi => Some(AiLogo::OhMyPi),
        AgentKind::CodeBuddy => Some(AiLogo::CodeBuddy),
        AgentKind::Kimi => Some(AiLogo::Kimi),
        _ => None,
    }
}

pub(crate) fn ai_logo(program: &str) -> Option<AiLogo> {
    if cfg!(any(not(feature = "png"), target_os = "macos")) {
        return None;
    }
    ai_logo_for_program(program)
}

pub(crate) fn program_icon(program: &str) -> &'static str {
    match program {
        "claude" => "\u{f0ce5}",
        "codex" => "\u{f02d8}",
        "gemini" => "\u{f0ce6}",
        "copilot" => "\u{f4b8}",
        "cursor" | "cursor-agent" => "\u{f0ec3}",
        "aider" | "goose" | "crush" | "ollama" => "\u{f06a9}",
        "opencode" | "trae-cli" => "\u{f489}",
        "pi" | "omp" | "oh-my-pi" => "\u{f135}",
        "codebuddy" | "cbc" | "codebuddy-code" | "codebuddy-lowmem" => "\u{f06a9}",
        "kimi" | "kimi-code" => "\u{f186}",
        "git" | "lazygit" => "\u{f418}",
        "vim" | "nvim" | "vi" | "hx" | "nano" => "\u{e62b}",
        "ssh" | "mosh" => "\u{f489}",
        "cargo" | "rustc" => "\u{e7a8}",
        "node" | "npm" | "pnpm" | "yarn" | "bun" | "deno" => "\u{e718}",
        "python" | "python3" | "pip" | "uv" => "\u{e73c}",
        "docker" | "podman" => "\u{e7b0}",
        _ => "\u{f04b}",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compressed_vectors_render_at_physical_sizes_on_both_themes() {
        for logo in AiLogo::ALL {
            let svg = resvg::usvg::decompress_svgz(logo.svg()).unwrap();
            assert!(std::str::from_utf8(&svg).unwrap().contains("<svg"));
            for ink in [[236, 239, 245], [35, 40, 50]] {
                for size in [16, 18, 24, 27, 36, 48] {
                    let (pixels, width, height) = logo.render(ink, size).unwrap();
                    assert_eq!(
                        (width, height, pixels.len()),
                        (size, size, (size * size * 4) as usize)
                    );
                    assert!(pixels.chunks_exact(4).any(|p| p[3] > 128), "{logo:?} {size}");
                    assert!(
                        pixels.chunks_exact(4).any(|p| p[3] > 0 && p[3] < 255),
                        "{logo:?} {size}"
                    );
                }
            }
        }
    }

    fn decode_png(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
        let image = image::load_from_memory(bytes).unwrap().into_rgba8();
        (image.width(), image.height(), image.into_raw())
    }

    fn logo_for_command(command: &str) -> Option<AiLogo> {
        extract_program(command).as_deref().and_then(ai_logo_for_program)
    }

    #[test]
    fn running_program_identity_normalizes_grok_without_changing_fallbacks() {
        for command in [
            "grok",
            "grok-cli",
            "GROK.CMD",
            "C:\\Tools\\GROK-CLI.EXE --help",
            "/usr/local/bin/grok",
        ] {
            assert_eq!(logo_for_command(command), Some(AiLogo::Grok), "{command}");
        }

        assert_eq!(logo_for_command("codex"), Some(AiLogo::OpenAi));
        assert_eq!(logo_for_command("C:\\Tools\\CLAUDE.EXE --resume"), Some(AiLogo::Claude));
        assert_eq!(program_icon("cargo"), "\u{e7a8}");
        assert_eq!(logo_for_command("unlisted-program"), None);
        assert_eq!(program_icon("unlisted-program"), "\u{f04b}");
        assert_eq!(extract_program(""), None);
    }

    #[test]
    fn official_png_logos_preserve_original_pixels_and_dimensions() {
        use sha2::{Digest, Sha256};

        // RGBA hashes come from the original assets, before lossless recompression.
        let assets = [
            (
                AiLogo::Grok.png(false),
                [
                    0x24, 0x43, 0x83, 0x65, 0x70, 0x1b, 0x58, 0xb6, 0x7b, 0x8c, 0x76, 0x55, 0x8a,
                    0x23, 0xc9, 0xf9, 0x53, 0x50, 0xe1, 0xea, 0x8e, 0x98, 0x23, 0xd9, 0x8e, 0xef,
                    0xec, 0xb0, 0xd9, 0xa8, 0xf1, 0x1a,
                ],
            ),
            (
                AiLogo::Grok.png(true),
                [
                    0x7e, 0x98, 0xa2, 0x1c, 0xf5, 0x0f, 0x61, 0x57, 0x9d, 0x91, 0xe6, 0xd1, 0xde,
                    0x7b, 0x1c, 0x4e, 0xf5, 0x13, 0x9b, 0xef, 0x8f, 0x0f, 0xda, 0xf3, 0x5b, 0x47,
                    0x02, 0xd0, 0xf7, 0x57, 0x25, 0x76,
                ],
            ),
            (
                AiLogo::Kimi.png(false),
                [
                    0x63, 0xcf, 0x88, 0x24, 0x8f, 0x3b, 0x30, 0x32, 0x1c, 0xb7, 0x19, 0x9e, 0xcf,
                    0x8b, 0x30, 0x7c, 0x01, 0x08, 0x94, 0x21, 0xb6, 0x47, 0xc5, 0x42, 0x04, 0xe8,
                    0x0b, 0xde, 0xb9, 0x6d, 0x92, 0x01,
                ],
            ),
        ];

        for (png, expected_sha256) in assets {
            let (width, height, rgba) = decode_png(png);
            assert_eq!(Sha256::digest(&rgba).as_slice(), expected_sha256);
            assert_eq!((width, height), (1024, 1024));
            assert!(rgba.chunks_exact(4).any(|pixel| pixel[3] != 0));
        }
    }

    #[test]
    fn official_grok_logos_prepare_sharp_optically_centered_physical_textures() {
        for png in [AiLogo::Grok.png(false), AiLogo::Grok.png(true)] {
            let (width, height, rgba) = decode_png(png);
            let (rgba, width, height) = prepare_ai_logo_texture(&rgba, width, height, 18);

            assert_eq!((width, height, rgba.len()), (18, 18, 18 * 18 * 4));
            let alpha_levels = rgba
                .chunks_exact(4)
                .map(|pixel| pixel[3])
                .filter(|alpha| *alpha > 0)
                .collect::<std::collections::HashSet<_>>();
            assert!(alpha_levels.len() >= 8);

            let (mass, weighted_x, weighted_y) = rgba.chunks_exact(4).enumerate().fold(
                (0.0, 0.0, 0.0),
                |(mass, weighted_x, weighted_y), (index, pixel)| {
                    let alpha = f64::from(pixel[3]);
                    let x = (index as u32 % width) as f64 + 0.5;
                    let y = (index as u32 / width) as f64 + 0.5;
                    (mass + alpha, weighted_x + x * alpha, weighted_y + y * alpha)
                },
            );
            let center = f64::from(width) / 2.0;
            assert!((weighted_x / mass - center).abs() <= 0.5);
            assert!((weighted_y / mass - center).abs() <= 0.5);
        }
    }

    #[test]
    fn antigravity_aliases_share_the_running_program_logo() {
        for command in [
            "antigravity",
            "agy",
            "antigravity-cli",
            "AGY.CMD --help",
            "C:\\Tools\\ANTIGRAVITY-CLI.EXE",
            "/usr/local/bin/antigravity",
        ] {
            assert_eq!(ai_logo_for_program(command), Some(AiLogo::Antigravity), "{command}");
        }
        for command in ["gravity", "antigravity-helper", "agy-not-an-agent"] {
            assert_eq!(ai_logo_for_program(command), None, "{command}");
        }
    }

    #[test]
    fn official_antigravity_logo_keeps_its_pixels_and_colors_on_both_themes() {
        use sha2::{Digest, Sha256};

        let png = AiLogo::Antigravity.png(false);
        assert_eq!(png, AiLogo::Antigravity.png(true));
        let (width, height, source) = decode_png(png);
        // Hash the original decoded RGBA, including colors beneath zero alpha.
        assert_eq!(
            Sha256::digest(&source).as_slice(),
            [
                0x9b, 0x57, 0x4d, 0x4a, 0xe9, 0x4a, 0x38, 0x65, 0xca, 0x94, 0x18, 0xfd, 0x6c, 0x7e,
                0x1f, 0x5f, 0xc4, 0xb7, 0x6e, 0x81, 0xf3, 0x0d, 0x78, 0x6d, 0x10, 0x5e, 0x71, 0x1f,
                0xa0, 0x4a, 0x25, 0x52
            ]
        );
        assert_eq!((width, height), (540, 540));
        assert!(source.chunks_exact(4).any(|pixel| pixel[3] == 0));
        assert!(source.chunks_exact(4).any(|pixel| pixel[3] == 255));
        for ink in [[236, 239, 245], [35, 40, 50]] {
            let mut rgba = source.clone();
            AiLogo::Antigravity.tint_pixels(&mut rgba, ink);
            assert_eq!(rgba, source);
            for size in [16, 18, 24, 27, 36] {
                let (pixels, actual_width, actual_height) =
                    prepare_ai_logo_texture(&rgba, width, height, size);
                assert_eq!((actual_width, actual_height), (size, size));
                assert_eq!(pixels.len(), (size * size * 4) as usize);
                assert!(
                    pixels
                        .chunks_exact(4)
                        .any(|pixel| { pixel[3] > 128 && pixel[0].abs_diff(pixel[2]) > 50 })
                );
            }
        }
    }

    #[test]
    fn shared_tint_preserves_color_assets_and_luminance_maps() {
        let source = [255, 255, 255, 128, 128, 128, 128, 64];
        let ink = [100, 200, 240];
        for logo in [AiLogo::Claude, AiLogo::Grok, AiLogo::Antigravity, AiLogo::Trae] {
            let mut pixels = source;
            logo.tint_pixels(&mut pixels, ink);
            assert_eq!(pixels, source);
        }
        for logo in [AiLogo::OpenAi, AiLogo::Pi] {
            let mut pixels = source;
            logo.tint_pixels(&mut pixels, ink);
            assert_eq!(pixels, [100, 200, 240, 128, 100, 200, 240, 64]);
        }
        for logo in [AiLogo::OpenCode, AiLogo::Kimi] {
            let mut pixels = source;
            logo.tint_pixels(&mut pixels, ink);
            assert_eq!(pixels, [100, 200, 240, 128, 50, 100, 120, 64]);
        }
    }

    #[test]
    fn vector_sourced_logos_keep_color_and_antialiased_edges_at_tab_sizes() {
        for logo in [AiLogo::Claude, AiLogo::Trae, AiLogo::OhMyPi, AiLogo::Kimi] {
            let (width, height, source) = decode_png(logo.png(false));
            assert_eq!((width, height), (1024, 1024));
            for size in [16, 18, 24, 27, 36, 48] {
                let (pixels, width, height) = prepare_ai_logo_texture(&source, width, height, size);
                assert_eq!((width, height), (size, size));
                assert!(pixels.chunks_exact(4).any(|p| p[3] == 0));
                assert!(pixels.chunks_exact(4).any(|p| p[3] == 255));
                let edge_levels: std::collections::HashSet<_> = pixels
                    .chunks_exact(4)
                    .map(|p| p[3])
                    .filter(|alpha| *alpha > 0 && *alpha < 255)
                    .collect();
                assert!(edge_levels.len() >= 8, "{logo:?} at {size}px");
                // Kimi 走亮度映射(白字形在浅色主题下不可见),不参与保色断言;
                // 它的染色行为由 shared_tint_preserves_color_assets_and_luminance_maps 钉住。
                if logo != AiLogo::Kimi {
                    for ink in [[236, 239, 245], [35, 40, 50]] {
                        let mut tinted = pixels.clone();
                        logo.tint_pixels(&mut tinted, ink);
                        assert_eq!(tinted, pixels);
                    }
                }
            }
        }
        assert_eq!(logo_for_command("TRAE-CLI.EXE"), Some(AiLogo::Trae));
        assert_eq!(logo_for_command("trae-cli-helper"), None);
        for command in ["omp", "oh-my-pi", r"C:\tools\OMP.EXE"] {
            assert_eq!(logo_for_command(command), Some(AiLogo::OhMyPi));
        }
        for command in ["kimi", "kimi-code", r"C:\tools\KIMI.EXE --help"] {
            assert_eq!(logo_for_command(command), Some(AiLogo::Kimi), "{command}");
        }
        assert_eq!(logo_for_command("kimi-helper"), None);
        assert_eq!(program_icon("kimi"), "\u{f186}");
        assert_eq!(program_icon("kimi-code"), "\u{f186}");
    }

    #[test]
    fn codebuddy_commands_share_the_color_logo_without_matching_helpers() {
        for command in [
            "codebuddy",
            "cbc",
            "codebuddy-code",
            "codebuddy-lowmem",
            r"C:\tools\CODEBUDDY.CMD --help",
        ] {
            assert_eq!(logo_for_command(command), Some(AiLogo::CodeBuddy), "{command}");
        }
        for command in ["cbc-prewarm", "codebuddy-helper", "cat codebuddy.md"] {
            assert_eq!(logo_for_command(command), None, "{command}");
        }
    }

    #[test]
    fn codebuddy_square_logo_smooths_edges_without_recoloring_the_brand() {
        let logo = AiLogo::CodeBuddy;
        let (width, height, source) = decode_png(logo.png(false));
        assert_eq!((width, height), (1024, 1024));
        let original = image::RgbaImage::from_raw(width, height, source.clone()).unwrap();
        let partial_coverage =
            |pixels: &[u8]| pixels.chunks_exact(4).filter(|p| p[3] > 0 && p[3] < 255).count();
        for size in [16, 18, 24, 27, 36, 48] {
            let (pixels, width, height) = prepare_ai_logo_texture(&source, width, height, size);
            assert_eq!((width, height), (size, size));
            assert!(pixels.chunks_exact(4).any(|p| p[3] == 0));
            assert!(pixels.chunks_exact(4).any(|p| p[3] == 255));
            // A symmetric rounded square has fewer distinct edge alpha levels
            // than the asymmetric marks above. Compare actual fractional edge
            // coverage with a deliberately aliased nearest-neighbor sample.
            let aliased = image::imageops::resize(
                &original,
                size,
                size,
                image::imageops::FilterType::Nearest,
            );
            assert!(partial_coverage(&pixels) > partial_coverage(aliased.as_raw()), "{size}px");
            assert!(pixels.chunks_exact(4).any(|p| p[3] > 128 && p[2] > p[0].saturating_add(50)));
            for ink in [[236, 239, 245], [35, 40, 50]] {
                let mut tinted = pixels.clone();
                logo.tint_pixels(&mut tinted, ink);
                assert_eq!(tinted, pixels);
            }
        }
    }
}
