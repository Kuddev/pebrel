//! 设置页的彩色品牌图标；只在 DPI/主题变化时后台准备，渲染只读缓存。

use super::*;
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct BrandIcons {
    target: Option<(u32, bool)>,
    images: HashMap<AgentKind, Arc<RenderImage>>,
    task: Option<Task<()>>,
    installed: Option<std::collections::HashSet<AgentKind>>,
    inspection: Option<Task<()>>,
    inspected_at: Option<std::time::Instant>,
}

impl SettingsPane {
    pub(in crate::gpui_shell) fn refresh_provider_installations(&mut self, cx: &mut Context<Self>) {
        self.provider_ui.brands.inspected_at = Some(std::time::Instant::now());
        let work = cx.background_executor().spawn(async {
            AgentKind::ALL
                .into_iter()
                .filter(|agent| crate::ai_hook::integrations::executable(*agent).is_some())
                .collect()
        });
        self.provider_ui.brands.inspection = Some(cx.spawn(async move |this, cx| {
            let installed = work.await;
            let _ = this.update(cx, |this, cx| {
                this.provider_ui.brands.installed = Some(installed);
                this.provider_ui.brands.inspection = None;
                cx.notify();
            });
        }));
    }

    pub(super) fn prepare_provider_brands(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.provider_ui.brands.inspection.is_none()
            && self
                .provider_ui
                .brands
                .inspected_at
                .is_none_or(|at| at.elapsed() >= Duration::from_secs(5))
        {
            self.refresh_provider_installations(cx);
        }
        self.prepare_agent_brands(window, cx);
    }

    pub(in crate::gpui_shell::settings_pane) fn prepare_agent_brands(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let target = ((24.0 * window.scale_factor()).ceil() as u32, cx.theme().is_dark());
        if self.provider_ui.brands.target == Some(target) {
            return;
        }
        self.provider_ui.brands.target = Some(target);
        self.provider_ui.brands.images.clear();
        let work = cx.background_executor().spawn(async move {
            let ink = if target.1 { [224, 228, 236] } else { [40, 45, 55] };
            [
                AgentKind::Claude,
                AgentKind::Codex,
                AgentKind::Gemini,
                AgentKind::OpenCode,
                AgentKind::Grok,
                AgentKind::Pi,
                AgentKind::Kimi,
                AgentKind::Hermes,
                AgentKind::Qoder,
                AgentKind::Qwen,
                AgentKind::CodeBuddy,
                AgentKind::Droid,
                AgentKind::Antigravity,
            ]
            .into_iter()
            .filter_map(|agent| {
                let (mut pixels, width, height) =
                    crate::display::AiLogo::render_brand(source(agent)?, ink, target.0)?;
                for pixel in pixels.chunks_exact_mut(4) {
                    if agent == AgentKind::Kimi && pixel[0] == pixel[1] && pixel[1] == pixel[2] {
                        pixel[..3].copy_from_slice(&ink);
                    }
                    // 夜间保留色相，只把品牌原色轻压暗；不改成单色蒙版。
                    if target.1 {
                        for channel in &mut pixel[..3] {
                            *channel = (u16::from(*channel) * 86 / 100) as u8;
                        }
                    }
                    pixel.swap(0, 2);
                }
                let rgba = image::RgbaImage::from_raw(width, height, pixels)?;
                Some((agent, Arc::new(RenderImage::new([image::Frame::new(rgba)]))))
            })
            .collect::<HashMap<_, _>>()
        });
        self.provider_ui.brands.task = Some(cx.spawn(async move |this, cx| {
            let images = work.await;
            let _ = this.update(cx, |this, cx| {
                if this.provider_ui.brands.target == Some(target) {
                    this.provider_ui.brands.images = images;
                    cx.notify();
                }
            });
        }));
    }

    pub(in crate::gpui_shell::settings_pane) fn provider_brand(
        &self,
        agent: AgentKind,
    ) -> gpui::AnyElement {
        if let Some(image) = self.provider_ui.brands.images.get(&agent) {
            img(image.clone()).size(px(24.0)).into_any_element()
        } else if source(agent).is_some() {
            // Keep the slot stable while the color texture is prepared. Rendering
            // a multicolor SVG as an alpha mask can produce an opaque square.
            div().size(px(24.0)).into_any_element()
        } else {
            super::view::agent_icon(agent).size(px(24.0)).into_any_element()
        }
    }

    pub(super) fn provider_needs_install(&self, agent: AgentKind) -> bool {
        self.provider_ui
            .brands
            .installed
            .as_ref()
            .is_some_and(|installed| !installed.contains(&agent))
    }
}

fn source(agent: AgentKind) -> Option<&'static [u8]> {
    use crate::display::AiLogo;
    Some(match agent {
        AgentKind::Claude => AiLogo::Claude.svg(),
        AgentKind::Codex => AiLogo::OpenAi.svg(),
        AgentKind::Gemini => include_bytes!("../../../../assets/icons/nebula-provider-gemini.svgz"),
        AgentKind::OpenCode => AiLogo::OpenCode.svg(),
        AgentKind::Grok => AiLogo::Grok.svg(),
        AgentKind::Pi => AiLogo::Pi.svg(),
        AgentKind::Kimi => AiLogo::Kimi.svg(),
        AgentKind::Hermes => include_bytes!("../../../../assets/icons/nebula-agent-hermes.svgz"),
        AgentKind::Qoder => include_bytes!("../../../../../extra/logo/ai_qoder.svgz"),
        AgentKind::Qwen => include_bytes!("../../../../../extra/logo/ai_qwen.svgz"),
        AgentKind::CodeBuddy => AiLogo::CodeBuddy.svg(),
        AgentKind::Droid => include_bytes!("../../../../../extra/logo/ai_droid.svgz"),
        AgentKind::Antigravity => AiLogo::Antigravity.svg(),
        _ => return None,
    })
}
