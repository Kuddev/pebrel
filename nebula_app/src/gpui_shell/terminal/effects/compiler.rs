use anyhow::{Context, Result, ensure};
use std::{io::Read, path::Path, sync::Arc};

#[path = "../../wallpaper/shader/native_compile.rs"]
mod native;

pub const UNIFORM_BYTES: usize = (13 + 256) * 16;
pub const ABI: &str = include_str!("abi.wgsl");

pub struct Program {
    pub passes: Arc<[Arc<[u8]>]>,
}

pub fn load_chain(paths: &[std::path::PathBuf]) -> Result<Program> {
    ensure!(
        !paths.is_empty() && paths.len() <= 8,
        "effects require one through eight source files"
    );
    let mut passes = Vec::new();
    for path in paths {
        let program =
            load(path).with_context(|| format!("compile effect source {}", path.display()))?;
        ensure!(
            passes.len() + program.passes.len() <= 8,
            "effect chain exceeds eight total passes"
        );
        passes.extend(program.passes.iter().cloned());
    }
    Ok(Program { passes: passes.into() })
}

pub fn load(path: &Path) -> Result<Program> {
    let file = std::fs::File::open(path).context("open terminal effect WGSL")?;
    ensure!(file.metadata()?.is_file(), "effect source is not a regular file");
    let mut bytes = Vec::new();
    file.take(64 * 1024 + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 64 * 1024, "effect source exceeds 64 KiB");
    compile(&String::from_utf8(bytes).context("effect source must be UTF-8")?)
}

pub fn compile(source: &str) -> Result<Program> {
    ensure!(source.len() <= 64 * 1024, "effect source exceeds 64 KiB");
    let input = format!("{ABI}\n{source}");
    let module = naga::front::wgsl::parse_str(&input)
        .map_err(|e| anyhow::anyhow!(e.emit_to_string(&input)))?;
    ensure!(
        (1..=8).contains(&module.entry_points.len()),
        "effects require one through eight fragment entry points"
    );
    for (_, global) in module.global_variables.iter() {
        if global.space == naga::AddressSpace::Private && global.binding.is_none() {
            continue;
        }
        let binding = global
            .binding
            .as_ref()
            .context("effect globals must use the declared frame/surface ABI")?;
        ensure!(
            binding.group == 0 && binding.binding <= 1,
            "effect globals must use the declared frame/surface ABI"
        );
        if binding.binding == 0 {
            ensure!(
                global.name.as_deref() == Some("frame")
                    && global.space == naga::AddressSpace::Uniform
                    && matches!(&module.types[global.ty].inner, naga::TypeInner::Struct { span, .. } if *span as usize == UNIFORM_BYTES),
                "effect frame layout changed"
            );
        } else {
            ensure!(
                global.name.as_deref() == Some("surface")
                    && global.space == naga::AddressSpace::Handle
                    && matches!(
                        module.types[global.ty].inner,
                        naga::TypeInner::Image {
                            dim: naga::ImageDimension::D2,
                            arrayed: false,
                            class: naga::ImageClass::Sampled {
                                kind: naga::ScalarKind::Float,
                                multi: false
                            }
                        }
                    ),
                "effect surface binding changed"
            );
        }
    }
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)?;
    let mut options = naga::back::hlsl::Options {
        shader_model: naga::back::hlsl::ShaderModel::V5_0,
        fake_missing_bindings: false,
        ..Default::default()
    };
    for binding in [0, 1] {
        options.binding_map.insert(
            naga::ResourceBinding { group: 0, binding },
            naga::back::hlsl::BindTarget { register: 0, ..Default::default() },
        );
    }
    let mut passes = Vec::new();
    for entry in &module.entry_points {
        ensure!(entry.stage == naga::ShaderStage::Fragment, "only fragment effects are supported");
        ensure!(entry.function.arguments.len() <= 1, "effect input is the local fragment position");
        for argument in &entry.function.arguments {
            ensure!(
                matches!(
                    argument.binding,
                    Some(naga::Binding::BuiltIn(naga::BuiltIn::Position { .. }))
                ),
                "effect input is the local fragment position"
            );
        }
        let result = entry.function.result.as_ref().context("missing effect color")?;
        ensure!(
            matches!(result.binding, Some(naga::Binding::Location { location: 0, .. }))
                && matches!(
                    module.types[result.ty].inner,
                    naga::TypeInner::Vector {
                        size: naga::VectorSize::Quad,
                        scalar: naga::Scalar { kind: naga::ScalarKind::Float, width: 4 }
                    }
                ),
            "effect output must be location 0 vec4<f32>"
        );
        let mut hlsl = String::new();
        let pipeline = naga::back::hlsl::PipelineOptions {
            entry_point: Some((naga::ShaderStage::Fragment, entry.name.clone())),
        };
        let reflected = naga::back::hlsl::Writer::new(&mut hlsl, &options, &pipeline)
            .write(&module, &info, None)?;
        ensure!(hlsl.len() <= 256 * 1024, "translated effect exceeds its byte budget");
        let name = reflected
            .entry_point_names
            .into_iter()
            .next()
            .context("missing native effect entry")?
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        passes.push(native::compile(&hlsl, &name)?);
    }
    Ok(Program { passes: passes.into() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actual_frame_palette_sampling_and_ordered_passes_compile() {
        let source = r#"
@fragment fn first(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    return sample_surface(p.xy / frame.viewport.xy) + frame.palette[frame.flags.y % 256u] * 0.01;
}
@fragment fn second(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    return load_surface(vec2<i32>(p.xy)) + frame.cursor_color * 0.01;
}"#;
        let program = compile(source).unwrap();
        assert_eq!(program.passes.len(), 2);
        assert!(program.passes.iter().all(|code| code.starts_with(b"DXBC")));
        assert_ne!(program.passes[0], program.passes[1]);
    }

    #[test]
    fn resources_and_entry_contracts_are_enforced() {
        assert!(compile("@group(1) @binding(0) var extra:texture_2d<f32>; @fragment fn main()->@location(0) vec4<f32>{return vec4<f32>(0.0);}").is_err());
        assert!(compile("@compute @workgroup_size(1) fn main(){}").is_err());
        assert!(compile("@group(0) @binding(1) var<storage,read> extra:array<f32>; @fragment fn main()->@location(0) vec4<f32>{return vec4<f32>(extra[0]);}").is_err());
        assert!(compile(&" ".repeat(64 * 1024 + 1)).is_err());
        let too_many = (0..9)
            .map(|i| {
                format!("@fragment fn p{i}()->@location(0) vec4<f32>{{return vec4<f32>(1.0);}}\n")
            })
            .collect::<String>();
        assert!(compile(&too_many).is_err());
    }

    #[test]
    fn files_keep_their_order_and_independent_entry_names() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first.wgsl");
        let second = directory.path().join("second.wgsl");
        std::fs::write(
            &first,
            "@fragment fn main()->@location(0) vec4<f32>{return vec4<f32>(0.25);}",
        )
        .unwrap();
        std::fs::write(
            &second,
            "@fragment fn main()->@location(0) vec4<f32>{return vec4<f32>(0.75);}",
        )
        .unwrap();
        let first_pass = load(&first).unwrap().passes[0].clone();
        let second_pass = load(&second).unwrap().passes[0].clone();
        assert_ne!(first_pass, second_pass);
        assert_eq!(
            &*load_chain(&[first.clone(), second.clone()]).unwrap().passes,
            &[first_pass.clone(), second_pass.clone()],
        );
        assert_eq!(&*load_chain(&[second, first]).unwrap().passes, &[second_pass, first_pass],);
    }

    #[test]
    fn later_source_failure_rejects_the_entire_chain() {
        let directory = tempfile::tempdir().unwrap();
        let valid = directory.path().join("valid.wgsl");
        let invalid = directory.path().join("invalid.wgsl");
        std::fs::write(
            &valid,
            "@fragment fn main()->@location(0) vec4<f32>{return vec4<f32>(1.0);}",
        )
        .unwrap();
        std::fs::write(&invalid, "not WGSL").unwrap();
        let error = load_chain(&[valid.clone(), invalid]).err().unwrap();
        assert!(error.to_string().contains("invalid.wgsl"));
        assert!(load_chain(&[valid, directory.path().join("missing.wgsl")]).is_err());
    }

    #[test]
    fn total_pass_limit_applies_across_files_not_only_within_each_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("four-passes.wgsl");
        let source = (0..4)
            .map(|i| {
                format!(
                    "@fragment fn pass{i}()->@location(0) vec4<f32>{{return vec4<f32>(1.0);}}\n"
                )
            })
            .collect::<String>();
        std::fs::write(&path, source).unwrap();
        assert_eq!(load_chain(&[path.clone(), path.clone()]).unwrap().passes.len(), 8);
        let error = load_chain(&[path.clone(), path.clone(), path.clone()]).err().unwrap();
        assert!(error.to_string().contains("eight total passes"));
        assert!(load_chain(&[]).is_err());
        assert!(load_chain(&vec![path; 9]).is_err());
    }
}
