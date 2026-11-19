//! Runtime 的自动化接口不等于插件接口；新增方法须先审查其核心所有权与授权范围。

use serde_json::json;

use super::{Error, Result};

/// 当前命令切片只开放无副作用的能力概览，不把 Runtime 的新增接口自动授予插件。
pub(super) const RUNTIME_METHODS: &[&str] = &["runtime.describe"];

pub(super) fn require_runtime_method(method: &str) -> Result<()> {
    if RUNTIME_METHODS.contains(&method) {
        return Ok(());
    }
    let mut error = Error::new(
        "unsupported_plugin_capability",
        format!("Runtime method is not exposed to plugins: {method}"),
    );
    error.details = Some(json!({
        "method": method,
        "available_methods": RUNTIME_METHODS,
    }));
    Err(error)
}
