use anyhow::{Context, Result, ensure};
use std::{io::Read, path::Path, sync::Arc};

pub const UNIFORM_BYTES: usize = (13 + 256) * 16;
pub const ABI: &str = include_str!("abi.wgsl");

pub struct Program {
    pub passes: Arc<[gpui::WgslPostprocessPass]>,
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
    let input: Arc<str> = format!("{ABI}\n{source}").into();
    let (module, _) = gpui::validate_postprocess_wgsl_module(&input, UNIFORM_BYTES)?;
    // 通用 ABI 由渲染器统一校验；这里只保留产品自己的变量名称合同。
    for (_, global) in module.global_variables.iter() {
        if let Some(binding) = &global.binding {
            let expected = if binding.binding == 0 { "frame" } else { "surface" };
            ensure!(
                global.name.as_deref() == Some(expected),
                "effect product binding name changed"
            );
        }
    }
    let passes = module
        .entry_points
        .into_iter()
        .map(|entry| gpui::WgslPostprocessPass {
            // 同一文件的入口共享源程序，后端可跨窗口尺寸复用其编译结果。
            source: input.clone(),
            entry: entry.name.into(),
        })
        .collect::<Vec<_>>();
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
        assert_eq!(&*program.passes[0].entry, "first");
        assert_eq!(&*program.passes[1].entry, "second");
        assert!(Arc::ptr_eq(&program.passes[0].source, &program.passes[1].source));
        assert!(program.passes[0].source.starts_with(ABI));
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
        assert_ne!(first_pass.source, second_pass.source);
        let forward = load_chain(&[first.clone(), second.clone()]).unwrap();
        assert_eq!(forward.passes[0].source, first_pass.source);
        assert_eq!(forward.passes[1].source, second_pass.source);
        let reverse = load_chain(&[second, first]).unwrap();
        assert_eq!(reverse.passes[0].source, second_pass.source);
        assert_eq!(reverse.passes[1].source, first_pass.source);
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
