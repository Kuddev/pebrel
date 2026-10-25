//! 显式启用后才在后台读取和编译；绘制阶段只接收已验证的字节码。
use anyhow::{Context as _, Result, anyhow, bail, ensure};
use nebula_settings::{BackgroundEffectRequest, BackgroundEffects};
use std::{io::Read, sync::Arc};
#[path = "native_compile.rs"]
mod native;
use native::compile as native_compile;

const SOURCE_LIMIT: usize = 64 * 1024;
const GRAIN: &str = r#"
@fragment
fn main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let grain = fract(sin(dot(position.xy, vec2<f32>(12.9898, 78.233))) * 43758.5453);
    return vec4<f32>(vec3<f32>(0.18, 0.22, 0.29) + vec3<f32>(grain * 0.015), 1.0);
}
"#;

pub(super) struct Program {
    pub bytecode: Arc<[u8]>,
    pub animated: bool,
    pub periodic: bool,
}

pub(super) fn compile(settings: &BackgroundEffects) -> Result<Option<Program>> {
    let request = settings.request().map_err(|error| anyhow!(error))?;
    let (source, periodic) = match request {
        BackgroundEffectRequest::Disabled => return Ok(None),
        BackgroundEffectRequest::Grain => (GRAIN.to_owned(), false),
        BackgroundEffectRequest::NeonVortex => (include_str!("neon_vortex.wgsl").to_owned(), true),
        BackgroundEffectRequest::AuroraRibbons => {
            (include_str!("aurora_ribbons.wgsl").to_owned(), true)
        },
        BackgroundEffectRequest::LiquidSilk => (include_str!("liquid_silk.wgsl").to_owned(), true),
        BackgroundEffectRequest::Wgsl(path) => {
            let path = nebula_settings::settings_dir().join(path);
            let file = std::fs::File::open(path).context("open background WGSL")?;
            ensure!(file.metadata()?.is_file(), "background WGSL must be a regular file");
            let mut bytes = Vec::new();
            file.take((SOURCE_LIMIT + 1) as u64).read_to_end(&mut bytes)?;
            ensure!(bytes.len() <= SOURCE_LIMIT, "background WGSL exceeds 64 KiB");
            (String::from_utf8(bytes).context("background WGSL must use UTF-8")?, false)
        },
    };
    let (hlsl, entry, animated) = translate(&source)?;
    Ok(Some(Program { bytecode: native_compile(&hlsl, &entry)?, animated, periodic }))
}

fn translate(source: &str) -> Result<(String, String, bool)> {
    ensure!(source.len() <= SOURCE_LIMIT, "background WGSL exceeds 64 KiB");
    let module = naga::front::wgsl::parse_str(source)
        .map_err(|error| anyhow!(error.emit_to_string(source)))?;
    let vec4 = |ty: naga::Handle<naga::Type>| {
        matches!(
            module.types[ty].inner,
            naga::TypeInner::Vector {
                size: naga::VectorSize::Quad,
                scalar: naga::Scalar { kind: naga::ScalarKind::Float, width: 4 }
            }
        )
    };
    let mut animated = false;
    for (_, variable) in module.global_variables.iter() {
        if variable.binding.is_none() && variable.space == naga::AddressSpace::Private {
            continue;
        }
        let block = vec4(variable.ty)
            || matches!(&module.types[variable.ty].inner,
            naga::TypeInner::Struct { members, span: 16 }
                if members.len() == 1 && members[0].offset == 0 && vec4(members[0].ty));
        ensure!(
            !animated
                && block
                && variable.space == naga::AddressSpace::Uniform
                && variable.binding == Some(naga::ResourceBinding { group: 0, binding: 0 }),
            "background ABI admits only one 16-byte uniform at group 0 / binding 0"
        );
        animated = true;
    }
    ensure!(module.entry_points.len() == 1, "one fragment entry is required");
    let entry = &module.entry_points[0];
    ensure!(entry.stage == naga::ShaderStage::Fragment, "background entry must be a fragment");
    let output = entry.function.result.as_ref().context("missing fragment color")?;
    let color = |ty, binding: &Option<naga::Binding>| {
        vec4(ty) && matches!(binding, Some(naga::Binding::Location { location: 0, .. }))
    };
    let valid_output = match &module.types[output.ty].inner {
        naga::TypeInner::Struct { members, .. } => {
            members.len() == 1 && color(members[0].ty, &members[0].binding)
        },
        _ => color(output.ty, &output.binding),
    };
    ensure!(valid_output, "one location 0 vec4 color is required");
    ensure!(entry.function.arguments.len() <= 1, "only position input is admitted");
    let position = |ty, binding: &Option<naga::Binding>| {
        vec4(ty) && matches!(binding, Some(naga::Binding::BuiltIn(naga::BuiltIn::Position { .. })))
    };
    for argument in &entry.function.arguments {
        let valid = match &module.types[argument.ty].inner {
            naga::TypeInner::Struct { members, .. } => {
                members.len() == 1 && position(members[0].ty, &members[0].binding)
            },
            _ => position(argument.ty, &argument.binding),
        };
        ensure!(valid, "only position input is admitted");
    }
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .map_err(|error| anyhow!(error.to_string()))?;
    let mut options = naga::back::hlsl::Options {
        shader_model: naga::back::hlsl::ShaderModel::V5_0,
        fake_missing_bindings: false,
        ..Default::default()
    };
    if animated {
        options.binding_map.insert(
            naga::ResourceBinding { group: 0, binding: 0 },
            naga::back::hlsl::BindTarget { register: 0, ..Default::default() },
        );
    }
    let pipeline = naga::back::hlsl::PipelineOptions {
        entry_point: Some((naga::ShaderStage::Fragment, entry.name.clone())),
    };
    let mut hlsl = String::new();
    let reflection = naga::back::hlsl::Writer::new(&mut hlsl, &options, &pipeline)
        .write(&module, &info, None)
        .map_err(|error| anyhow!(error.to_string()))?;
    ensure!(hlsl.len() <= 256 * 1024, "translated background exceeds 256 KiB");
    let name = reflection
        .entry_point_names
        .into_iter()
        .next()
        .context("missing translated entry")?
        .map_err(|error| anyhow!("{error:?}"))?;
    Ok((hlsl, name, animated))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_translate_with_the_declared_time_abi() {
        assert!(!translate(GRAIN).unwrap().2);
        for source in [
            include_str!("neon_vortex.wgsl"),
            include_str!("aurora_ribbons.wgsl"),
            include_str!("liquid_silk.wgsl"),
        ] {
            let (hlsl, entry, animated) = translate(source).unwrap();
            assert!(animated);
            assert!(hlsl.contains("register(b0)"));
            assert!(native_compile(&hlsl, &entry).unwrap().starts_with(b"DXBC"));
        }
    }

    #[test]
    fn disabled_custom_source_is_never_opened() {
        let settings =
            BackgroundEffects { wgsl_path: Some("missing.wgsl".into()), ..Default::default() };
        assert!(compile(&settings).unwrap().is_none());
    }

    #[test]
    fn invalid_binding_and_excess_source_are_rejected() {
        assert!(translate(&" ".repeat(SOURCE_LIMIT + 1)).is_err());
        assert!(translate("@group(0) @binding(1) var<uniform> data:vec4<f32>; @fragment fn main()->@location(0) vec4<f32>{return data;}").is_err());
        assert!(translate("@group(0) @binding(0) var<uniform> data:mat4x4<f32>; @fragment fn main()->@location(0) vec4<f32>{return data[0];}").is_err());
    }
}
