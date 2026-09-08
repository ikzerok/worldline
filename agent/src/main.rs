//! wl-agent —— worldline 机器协议入口。实现见 lib.rs。

fn main() {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let code = worldline_agent::run(&mut stdin.lock(), &mut stdout.lock());
    std::process::exit(code);
}
