use anyhow::{Context, Result, bail, ensure};
use std::{ffi::CString, sync::Arc};

pub(super) fn compile(hlsl: &str, entry: &str) -> Result<Arc<[u8]>> {
    use windows::{
        Win32::Graphics::Direct3D::{Fxc::*, ID3DInclude},
        core::{PCSTR, s},
    };
    let entry = CString::new(entry)?;
    let mut bytecode = None;
    let mut diagnostics = None;
    let result = unsafe {
        D3DCompile(
            hlsl.as_ptr().cast(),
            hlsl.len(),
            PCSTR::null(),
            None,
            None::<&ID3DInclude>,
            PCSTR(entry.as_ptr().cast()),
            s!("ps_4_1"),
            D3DCOMPILE_ENABLE_STRICTNESS | D3DCOMPILE_OPTIMIZATION_LEVEL3,
            0,
            &mut bytecode,
            Some(&mut diagnostics),
        )
    };
    if let Err(error) = result {
        let detail = diagnostics
            .map(|blob| unsafe {
                String::from_utf8_lossy(std::slice::from_raw_parts(
                    blob.GetBufferPointer().cast(),
                    blob.GetBufferSize().min(4096),
                ))
                .into_owned()
            })
            .unwrap_or_default();
        bail!("background native compilation failed: {error}: {detail}");
    }
    let blob = bytecode.context("native compiler returned no bytecode")?;
    let bytes = unsafe {
        std::slice::from_raw_parts(blob.GetBufferPointer().cast::<u8>(), blob.GetBufferSize())
    };
    ensure!(bytes.len() <= 64 * 1024 && bytes.starts_with(b"DXBC"), "invalid background bytecode");
    Ok(Arc::from(bytes))
}
