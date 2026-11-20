use super::*;

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
