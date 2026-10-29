use super::*;

/// 隔离真值表：只有「进程**确认**提升而桌面 shell 非提升」才隔离。
///
/// Administrator 桌面会话（UAC Admin Approval Mode 关闭，explorer 与一切
/// 进程同为提升）必须放行——否则会话保存整体静默失效，恢复功能在那种
/// 机器上永久不可用。UAC runas 的提权窗口（进程 High、桌面 Medium）保持
/// 隔离，这是该判定的原始意图；查询不到桌面 shell 时按非提升回退，
/// 等价于旧的仅看进程语义。
#[test]
fn only_a_privilege_mismatch_with_the_desktop_isolates() {
    assert!(!isolate(Ok(true), Some(true)), "管理员桌面会话：两边都提升，不隔离");
    assert!(isolate(Ok(true), Some(false)), "UAC runas：进程提升、桌面非提升，隔离");
    assert!(!isolate(Ok(false), Some(false)), "普通启动永不隔离");
    assert!(!isolate(Ok(false), None), "无桌面会话里的普通进程不隔离");
    assert!(isolate(Ok(true), None), "无桌面会话时保守回退到旧语义");
}

/// 进程 token 查询失败：未知即按提升处理（fail-closed），且**不得**进入
/// 与桌面 shell 的合成判定——桌面提升不能洗白状态未知的进程（失败同样
/// 可能发生在真正的 runas 提权进程上）。错误路径与上面的成功查询真值表
/// 分开锁定（维护者 2026-10-28 review 裁定）。
#[test]
fn a_failed_process_token_query_isolates_regardless_of_the_desktop() {
    assert!(isolate(Err(()), Some(true)), "查询失败 + 管理员桌面：仍隔离");
    assert!(isolate(Err(()), Some(false)), "查询失败 + 普通桌面：仍隔离");
    assert!(isolate(Err(()), None), "查询失败 + 无桌面会话：仍隔离");
}

#[test]
fn launch_arguments_target_pebrel_cli_and_preserve_selected_shell() {
    let shell_args = vec!["--literal".into(), "$(value); `echo nope`".into()];
    let args = arguments("shell path.exe", &shell_args, Some(Path::new("C:\\work area\\")));
    assert_eq!(
        args,
        [
            "--gpui",
            "--working-directory",
            "C:\\work area\\",
            "-e",
            "shell path.exe",
            "--literal",
            "$(value); `echo nope`"
        ]
        .map(OsString::from)
    );
    assert_eq!(arguments("pwsh.exe", &[], None), ["--gpui", "-e", "pwsh.exe"].map(OsString::from));
}

#[cfg(windows)]
#[test]
fn native_windows_argument_parser_round_trips_quotes_paths_and_metacharacters() {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::UI::Shell::CommandLineToArgvW;

    let inputs = [
        "",
        "simple",
        "C:\\Program Files\\shell.exe",
        "C:\\工作区\\",
        "a\"b",
        "x\\\"y",
        "$(echo hidden); `literal` & | < > %PATH%",
        "--option=two words",
    ];
    let mut command: Vec<u16> = "pebrel.exe".encode_utf16().collect();
    for input in inputs {
        command.push(b' ' as u16);
        command.extend(quoted_argument(OsStr::new(input)).unwrap());
    }
    command.push(0);
    // SAFETY: the parser returns `count` NUL-terminated strings; LocalFree owns cleanup.
    unsafe {
        let mut count = 0;
        let argv = CommandLineToArgvW(command.as_ptr(), &mut count);
        assert!(!argv.is_null());
        let parsed: Vec<_> = std::slice::from_raw_parts(argv, count as usize)
            .iter()
            .map(|value| {
                let mut len = 0;
                while *value.add(len) != 0 {
                    len += 1;
                }
                OsString::from_wide(std::slice::from_raw_parts(*value, len))
            })
            .collect();
        LocalFree(argv.cast());
        assert_eq!(&parsed[1..], inputs.map(OsString::from));
    }
    assert!(quoted_argument(OsStr::new("nul\0suffix")).is_err());
}
