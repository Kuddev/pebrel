//! Windows 包中的 pebrel.com：保留管道、等待和退出码，不把 GUI 子系统当控制台。
mod native_result_model;
mod native_view_cli;

use clap::Parser as _;

#[derive(clap::Parser)]
#[command(bin_name = "pebrel view")]
struct View {
    #[command(flatten)]
    options: native_view_cli::Options,
}

fn main() {
    let mut arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments.first().is_some_and(|argument| argument == "view") {
        arguments[0] = "pebrel view".into();
        std::process::exit(native_view_cli::run(View::parse_from(arguments).options));
    }
    let result = std::env::current_exe().and_then(|path| {
        let executable = path.with_file_name(format!("pebrel{}", std::env::consts::EXE_SUFFIX));
        std::process::Command::new(executable).args(arguments).status()
    });
    match result {
        Ok(status) => std::process::exit(status.code().unwrap_or(1)),
        Err(error) => {
            eprintln!("pebrel: {error}");
            std::process::exit(1);
        },
    }
}
