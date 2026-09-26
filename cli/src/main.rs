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
            eprintln!("      wl play <文件.wl> [--load=存档.json] [--save=存档.json] [--seed N] [--trace-output=文件.json] [--json]");
            eprintln!("      wl replay <入口.wl> --trace-json '<DTO>' [--max-steps N] [--time-budget-ms N] [--json]");
            eprintln!("      wl graph <文件.wl> [--json]");
            eprintln!("      wl timeline <文件.wl> [--json]");
            eprintln!(
                "      wl catalog <目录或入口.wl> [--tag ID] [--recursive] [--kind 类型] [--json]"
            );
            eprintln!("      wl catalog-query <目录或入口> --query '<JSON DTO>' [--offset N] [--page-size N] [--max-candidates N] [--cursor '<JSON 游标>'] [--json]");
            eprintln!("      wl authoring-intent preview|apply <目录或入口> --intent-json '<JSON DTO>' [--json]");
            eprintln!("      wl workspace check <目录> [--json]");
            eprintln!("      wl maps list <目录> [--json]");
            eprintln!(
                "      wl relations <目录> --target KIND:ID [--offset N] [--depth 1|2] [--scope KIND:ID] [--include-unscoped] [--include-period-children] [--json]"
            );
            eprintln!("      wl relation-type create|update|delete <目录> --id ID [--display 名称] [--inverse-display 反向名] [update: --clear-inverse-display|--clear-from-kind|--clear-to-kind] [--json]");
            eprintln!("      wl relation create|update|delete <目录> --id ID [--type TYPE] [--from KIND:ID] [--to KIND:ID] [--description 文案] [--source-note 来源] [--scope KIND:ID] [--property name=value] [update: --clear-source-note|--clear-scope|--clear-properties] [--json]");
            eprintln!("      wl relations promote preview|commit <目录> --source KIND:ID --target KIND:ID --label 标签 --id ID --type TYPE [--scope KIND:ID] [--property name=value] [--json]");
            eprintln!(
                "      wl entity create|update|delete <目录或入口> --id ID [--kind 类型] [--display 名称] [--json]"
            );
            ExitCode::from(2)
        }
    }
}
