//! wl —— worldline 命令行工具入口。实现见 lib.rs。

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if matches!(
        args.first().map(String::as_str),
        Some("--version") | Some("-V")
    ) {
        println!("wl {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    let stdin = std::io::stdin();
    let mut lock = stdin.lock();
    match wl::run(&args, &mut std::io::stdout(), &mut lock) {
        Ok(code) => ExitCode::from(code as u8),
        Err(msg) => {
            eprintln!("wl: {msg}");
            eprintln!("用法: wl check <文件.wl> [--json]");
            eprintln!("      wl play <文件.wl> [--load=存档.json] [--save=存档.json] [--json]");
            eprintln!("      wl graph <文件.wl> [--json]");
            eprintln!("      wl timeline <文件.wl> [--json]");
            eprintln!(
                "      wl catalog <目录或入口.wl> [--tag ID] [--recursive] [--kind 类型] [--json]"
            );
            eprintln!(
                "      wl entity create|update|delete <目录或入口> --id ID [--kind 类型] [--display 名称] [--json]"
            );
            ExitCode::from(2)
        }
    }
}
