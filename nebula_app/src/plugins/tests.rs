use std::fs;
use std::time::Duration;

use serde_json::{Value, json};

use super::*;

const NATIVE: &str = r#"
manifest_version = 1
api_version = 1
id = "example.overview"
name = "运行时概览"
version = "0.1.0"
permissions = ["runtime.describe"]
[[commands]]
id = "describe"
title = "运行时概览"
method = "runtime.describe"
"#;

fn package(manifest: &str, source: Option<&[u8]>) -> (tempfile::TempDir, Package) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("plugin.toml"), manifest).unwrap();
    if let Some(source) = source {
        fs::write(dir.path().join("init.lua"), source).unwrap();
    }
    let package = Package::open(dir.path()).unwrap();
    (dir, package)
}

fn lua_manifest() -> String {
    NATIVE
        .replace("permissions =", "entry = 'init.lua'\npermissions =")
        .replace("method = \"runtime.describe\"", "handler = \"describe\"")
}

fn run_lua(source: &str) -> Result<Execution> {
    let (_dir, package) = package(&lua_manifest(), Some(source.as_bytes()));
    execute(&package, "describe", json!({}), Duration::from_secs(5), &mut |_, _, _| {
        panic!("this fixture must not call Runtime")
    })
}

#[test]
fn resource_command_uses_native_result_without_a_lua_vm() {
    let (_dir, package) = package(NATIVE, None);
    let initial_vms = lua::vm_creation_count();
    let execution = execute(
        &package,
        "describe",
        json!({"label":"中文"}),
        Duration::from_secs(1),
        &mut |method, params, _| {
            assert_eq!(method, "runtime.describe");
            assert_eq!(params, json!({"label":"中文"}));
            Ok(json!({"version": 1}))
        },
    )
    .unwrap();
    assert_eq!(execution.engine, "native");
    assert_eq!(execution.lua_memory_bytes, 0);
    assert_eq!(execution.instruction_ticks, 0);
    assert_eq!(lua::vm_creation_count(), initial_vms);
}

#[test]
fn mixed_package_native_command_does_not_parse_lua() {
    let manifest = format!(
        "{}\n[[commands]]\nid='dynamic'\ntitle='动态'\nhandler='dynamic'\n",
        NATIVE.replace("permissions =", "entry = 'init.lua'\npermissions =")
    );
    let (_dir, package) = package(&manifest, Some(b"not valid Lua !!"));
    let initial_vms = lua::vm_creation_count();
    let execution =
        execute(&package, "describe", json!({}), Duration::from_secs(1), &mut |_, _, _| {
            Ok(Value::Null)
        })
        .unwrap();
    assert_eq!(execution.lua_memory_bytes, 0);
    assert_eq!(lua::vm_creation_count(), initial_vms);
}

#[test]
fn lua_command_yields_runtime_requests_and_returns_utf8_data() {
    let (_dir, package) = package(&lua_manifest(), Some(br#"
        return { describe = function(ctx, args)
            local result = ctx.runtime.call('runtime.describe', {label=args.label})
            return {label=args.label, count=#result.capabilities, empty=ctx.array(), nothing=ctx.null}
        end }
    "#));
    let mut calls = 0;
    let execution = execute(
        &package,
        "describe",
        json!({"label":"工作台"}),
        Duration::from_secs(1),
        &mut |_, params, _| {
            calls += 1;
            assert_eq!(params["label"], "工作台");
            Ok(json!({"capabilities":["one","two"]}))
        },
    )
    .unwrap();
    assert_eq!(calls, 1);
    assert_eq!(execution.result, json!({"label":"工作台", "count":2, "empty":[], "nothing":null}));
    assert!(execution.lua_memory_bytes > 0 && execution.lua_memory_bytes <= MAX_LUA_MEMORY);
}

#[test]
fn missing_capability_fails_before_native_dispatch() {
    let (_dir, package) = package(
        &lua_manifest(),
        Some(
            br#"
        return { describe = function(ctx) return ctx.runtime.call('pane.close', {}) end }
    "#,
        ),
    );
    let error = execute(&package, "describe", json!({}), Duration::from_secs(1), &mut |_, _, _| {
        panic!("denied call reached the host")
    })
    .unwrap_err();
    assert_eq!(error.code, "permission_denied");
}

#[test]
fn native_error_receipts_survive_the_lua_boundary() {
    let (_dir, first_package) = package(
        &lua_manifest(),
        Some(
            br#"
        return {describe=function(ctx) return ctx.runtime.call('runtime.describe', {}) end}
    "#,
        ),
    );
    let mut expected = Error::new("action_failed", "native operation failed");
    expected.details = Some(json!({"partial":true, "cleanup_deferred":true}));
    let error =
        execute(&first_package, "describe", json!({}), Duration::from_secs(1), &mut |_, _, _| {
            Err(expected.clone())
        })
        .unwrap_err();
    assert_eq!(error.code, expected.code);
    assert_eq!(error.details, expected.details);

    let (_dir, package) = package(
        &lua_manifest(),
        Some(
            br#"
        return {describe=function(ctx)
            local ok = pcall(function() ctx.runtime.call('runtime.describe', {}) end)
            return {caught = not ok}
        end}
    "#,
        ),
    );
    let result =
        execute(&package, "describe", json!({}), Duration::from_secs(1), &mut |_, _, _| {
            Err(expected.clone())
        })
        .unwrap();
    assert_eq!(result.result, json!({"caught":true}));
}

#[test]
fn instruction_yields_stop_loops_even_inside_pcall() {
    let error = run_lua("return {describe=function() while true do pcall(function() while true do end end) end end}").unwrap_err();
    assert_eq!(error.code, "lua_instruction_limit");
}

#[test]
fn source_initialization_is_budgeted_too() {
    let error = run_lua("while true do end").unwrap_err();
    assert_eq!(error.code, "lua_instruction_limit");
}

#[test]
fn lua_memory_is_bounded() {
    let error =
        run_lua("return {describe=function() return string.rep('x', 32 * 1024 * 1024) end}")
            .unwrap_err();
    assert_eq!(error.code, "lua_memory_limit");
}

#[test]
fn payload_text_and_runtime_call_count_are_bounded() {
    assert_eq!(
        run_lua("return {describe=function() return string.rep('x', 65537) end}").unwrap_err().code,
        "payload_limit"
    );
    let (_dir, package) = package(
        &lua_manifest(),
        Some(
            br#"
        return {describe=function(ctx)
            for i=1,17 do ctx.runtime.call('runtime.describe', {}) end
        end}
    "#,
        ),
    );
    let mut calls = 0;
    let error = execute(&package, "describe", json!({}), Duration::from_secs(1), &mut |_, _, _| {
        calls += 1;
        Ok(Value::Null)
    })
    .unwrap_err();
    assert_eq!(error.code, "runtime_call_limit");
    assert_eq!(calls, MAX_RUNTIME_CALLS);
}

#[test]
fn filesystem_and_unmanaged_execution_are_not_exposed() {
    let execution = run_lua("return {describe=function() return {restricted=(io==nil and os==nil and package==nil and debug==nil and load==nil and loadfile==nil and dofile==nil and coroutine==nil)} end}").unwrap();
    assert_eq!(execution.result, json!({"restricted":true}));
}

#[test]
fn conversion_bounds_cycles_deep_data_and_shared_expansion() {
    for source in [
        "return {describe=function() local t={}; t.self=t; return t end}",
        "return {describe=function() local t={}; for i=1,40 do t={t} end; return t end}",
        "return {describe=function() local t={1}; for i=1,20 do t={t,t} end; return t end}",
    ] {
        assert_eq!(run_lua(source).unwrap_err().code, "payload_limit");
    }
}

#[test]
fn conversion_preserves_arrays_and_rejects_ambiguous_values() {
    assert_eq!(
        run_lua("return {describe=function() return {1,2,{name='ok'}} end}").unwrap().result,
        json!([1,2,{"name":"ok"}])
    );
    for source in [
        "return {describe=function() return {[1]=true, extra=true} end}",
        "return {describe=function() return {[2]=true} end}",
        "return {describe=function() return 0/0 end}",
        "return {describe=function() return function() end end}",
    ] {
        assert_eq!(run_lua(source).unwrap_err().code, "invalid_payload");
    }
    assert_eq!(
        run_lua("return {describe=function() return string.char(255) end}").unwrap_err().code,
        "invalid_utf8"
    );
}

#[test]
fn metadata_versions_unknown_fields_and_duplicate_commands_fail() {
    for manifest in [
        NATIVE.replace("api_version = 1", "api_version = 2"),
        NATIVE.replace("api_version = 1", "api_version = 1\nunknown = true"),
        format!(
            "{NATIVE}\n[[commands]]\nid='describe'\ntitle='duplicate'\nmethod='runtime.describe'"
        ),
        NATIVE.replace("method = \"runtime.describe\"", "method = \"pane.close\""),
    ] {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("plugin.toml"), manifest).unwrap();
        assert!(Package::open(dir.path()).is_err());
    }
}

#[test]
fn package_reads_are_bounded_and_utf8_only() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("plugin.toml"), vec![b' '; 32 * 1024 + 1]).unwrap();
    assert_eq!(Package::open(dir.path()).unwrap_err().code, "package_file_too_large");
    fs::write(dir.path().join("plugin.toml"), b"\xff").unwrap();
    assert_eq!(Package::open(dir.path()).unwrap_err().code, "invalid_utf8");
    fs::write(dir.path().join("plugin.toml"), lua_manifest()).unwrap();
    fs::write(dir.path().join("init.lua"), vec![b' '; 256 * 1024 + 1]).unwrap();
    assert_eq!(Package::open(dir.path()).unwrap_err().code, "source_too_large");
}

#[test]
fn entry_paths_are_portable_and_package_relative() {
    for entry in ["../init.lua", "/init.lua", "C:/init.lua", "a\\init.lua", "a//init.lua"] {
        let dir = tempfile::tempdir().unwrap();
        let manifest = lua_manifest().replace("entry = 'init.lua'", &format!("entry = '{entry}'"));
        fs::write(dir.path().join("plugin.toml"), manifest).unwrap();
        assert_eq!(Package::open(dir.path()).unwrap_err().code, "invalid_entry_path");
    }
}

#[cfg(unix)]
#[test]
fn entry_symlinks_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("plugin.toml"), lua_manifest()).unwrap();
    fs::write(outside.path().join("other.lua"), "return {}").unwrap();
    std::os::unix::fs::symlink(outside.path().join("other.lua"), dir.path().join("init.lua"))
        .unwrap();
    assert_eq!(Package::open(dir.path()).unwrap_err().code, "linked_package_file");
}
