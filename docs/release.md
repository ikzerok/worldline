# worldline 发布说明

worldline 是独立 Cargo workspace，成员为 core/runtime/cli/agent；不依赖父目录文件与编辑器。保留 Cargo.lock 与 rust-toolchain.toml，执行 `cargo build --workspace --release --locked` 生成 wl 和 wl-agent。

源码包含 spec、docs、examples、源码、测试、许可证和 CI。examples/harbor-world 是 Project::new 的编译期模板，必须保留。target、日志、临时文件和发行包不上传。运行 `cargo doc --workspace --no-deps --locked` 可生成完整公开 Rust API 文档。

与同级 worldedit 联合发布时使用其 scripts/package.ps1，Windows 包会附两项语言工具、完整规范和示例，另输出独立 worldline-source.zip 及 SHA256 校验和。工具包版本 0.1.0 与语言规范 v1.9 分开记录。当前不包含签名安装器或自动 GitHub 上传。
