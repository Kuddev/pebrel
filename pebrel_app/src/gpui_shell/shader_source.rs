//! GLSL is parsed into Naga IR before the existing renderer ABI is validated.

use anyhow::{Result, anyhow, ensure};
use std::path::Path;

pub(super) const SOURCE_LIMIT: usize = 64 * 1024;

#[derive(Clone, Copy)]
pub(super) enum Language {
    Wgsl,
    Glsl,
}

impl Language {
    pub fn for_path(path: &Path) -> Self {
        match path.extension().and_then(|extension| extension.to_str()) {
            Some(extension)
                if extension.eq_ignore_ascii_case("glsl")
                    || extension.eq_ignore_ascii_case("frag") =>
            {
                Self::Glsl
            },
            // Existing custom WGSL filenames remain accepted.
            _ => Self::Wgsl,
        }
    }
}

const BACKGROUND_ABI: &str = r#"
layout(std140, set = 0, binding = 0) uniform PebrelBackground {
    vec4 data;
} background;
#define iResolution vec3(background.data.xy, 1.0)
#define iTime background.data.z
#define iParameter background.data.w
"#;

pub(super) fn background(source: &str, language: Language) -> Result<naga::Module> {
    ensure!(source.len() <= SOURCE_LIMIT, "shader source exceeds 64 KiB");
    match language {
        Language::Wgsl => naga::front::wgsl::parse_str(source)
            .map_err(|error| anyhow!(error.emit_to_string(source))),
        Language::Glsl => glsl(source, BACKGROUND_ABI),
    }
}

pub(super) fn terminal(source: &str, language: Language, wgsl_abi: &str) -> Result<String> {
    ensure!(source.len() <= SOURCE_LIMIT, "shader source exceeds 64 KiB");
    if matches!(language, Language::Wgsl) {
        return Ok(format!("{wgsl_abi}\n{source}"));
    }
    let module = glsl(source, include_str!("terminal/effects/abi.glsl"))?;
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .map_err(|error| anyhow!(error.to_string()))?;
    let wgsl = naga::back::wgsl::write_string(
        &module,
        &info,
        naga::back::wgsl::WriterFlags::EXPLICIT_TYPES,
    )?;
    ensure!(wgsl.len() <= 256 * 1024, "translated shader exceeds 256 KiB");
    Ok(wgsl)
}

fn glsl(source: &str, abi: &str) -> Result<naga::Module> {
    // A complete GLSL 450 fragment owns its declarations. A versionless snippet
    // uses mainImage with the supplied ABI; includes never read additional files.
    let complete = source.lines().any(|line| line.trim_start().starts_with("#version"));
    let input = if complete {
        source.to_owned()
    } else {
        format!(
            "#version 450\n{abi}\nlayout(location = 0) out vec4 pebrel_color;\n\
             {source}\nvoid main() {{ mainImage(pebrel_color, gl_FragCoord.xy); }}\n"
        )
    };
    naga::front::glsl::Frontend::default()
        .parse(&naga::front::glsl::Options::from(naga::ShaderStage::Fragment), &input)
        .map_err(|errors| anyhow!(errors.emit_to_string(&input)))
}
