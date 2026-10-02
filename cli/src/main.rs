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
            eprintln!("      wl play <文件.wl> [--load=存档.json] [--save=存档.json] [--seed N] [--trace-output=文件.json] [--choice-presentation] [--json]");
            eprintln!("      wl replay <入口.wl> --trace-json '<DTO>' [--max-steps N] [--time-budget-ms N] [--json]");
            eprintln!("      wl graph <文件.wl> [--json]");
            eprintln!("      wl timeline <文件.wl> [--json]");
            eprintln!(
                "      wl catalog <目录或入口.wl> [--tag ID] [--recursive] [--kind 类型] [--json]"
            );
            eprintln!("      wl catalog-query <目录或入口> --query '<JSON DTO>' [--offset N] [--page-size N] [--max-candidates N] [--cursor '<JSON 游标>'] [--json]");
            eprintln!("      wl reader-export preview|apply <工程目录> --selection-json '<JSON DTO>' [apply: --plan-digest 摘要 --out 新目录] [--json]");
            eprintln!("      wl scene svg-preview --source '<SVG>' [--json]");
            eprintln!("      wl scene preview|apply <工程目录> --request-json '<SceneBatch>' [apply: --baseline 基线 --plan-digest 摘要] [--json]");
            eprintln!("      wl scene export <工程目录> --map-id ID [--json]");
            eprintln!("      wl localization export preview <工程目录> --selection-json '<JSON DTO>' --json");
            eprintln!("      wl localization export apply <工程目录> --selection-json '<JSON DTO>' --plan-digest 摘要 --out 新包.json --json");
            eprintln!("      wl localization import preview <工程目录> --selection-json '<JSON DTO>' --package 交换包.json --json");
            eprintln!("      wl localization import apply <工程目录> --selection-json '<JSON DTO>' --package 交换包.json --plan-digest 摘要 --json");
            eprintln!("      wl source-edit preview|apply <工程目录> --request-json '<DTO>' [apply: --plan-digest 摘要] [--json]");
            eprintln!("      wl source-lifecycle preview|apply <目录或入口> --request-json '<DTO>' [apply: --plan-digest 摘要] [--json]");
            eprintln!("      wl schema-index <工程目录> [--json]");
            eprintln!("      wl schema-preview <工程目录> --request-json '<DTO>' [--json]");
            eprintln!("      wl schema-apply <工程目录> --request-json '<DTO>' --plan-digest 摘要 [--json]");
            eprintln!("      check/play/replay/graph/timeline/catalog 可显式指定 --language-version=1.9|1.10|1.11|1.12|1.13");
            eprintln!("      wl authoring-intent preview|apply <目录或入口> --intent-json '<JSON DTO>' [--json]");
            eprintln!("      wl markdown import preview|apply <工程目录> --source <Markdown目录> --baseline <基线> [--id-map-json '<JSON 对象>'] [--namespace ID] [apply: --plan-digest 摘要 --accept-losses --allow-language-upgrade] [--json]");
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
