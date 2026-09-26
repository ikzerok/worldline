# worldline 发布说明

worldline 是独立 Cargo workspace，成员为 core/runtime/cli/agent；不依赖父目录文件与编辑器。保留 Cargo.lock 与 rust-toolchain.toml，执行 `cargo build --workspace --release --locked` 生成 wl 和 wl-agent。

源码包含 spec、docs、examples、源码、测试、许可证和 CI。examples/harbor-world 是 Project::new 的编译期模板，必须保留。target、日志、临时文件和发行包不上传。运行 `cargo doc --workspace --no-deps --locked` 可生成完整公开 Rust API 文档。

与同级 worldedit 联合发布时使用其 scripts/package.ps1，Windows 包会附两项语言工具、完整规范和示例，另输出独立 worldline-source.zip 及 SHA256 校验和。工具包版本与语言规范版本分开记录；0.3.0 同时支持显式 1.10 作者资料能力。当前不包含签名安装器。

## 0.3.0 更新

完成 M2–M4 作者工作台核心：通用 entity、独立语义关系与受限查询、跨视图重命名/删除保护、16 类可选内容模板、长文保真与性能门、作者范围与显式源码集、展示预设、持久批注和带基线的修改提案。地图、网络、模板和协作文档继续与故事运行指纹分域；旧 1.9 工程不自动升级。

最终 WP-17 验收为 65/65 项 PASS；配对 worldedit 提供 Windows/Web 发布包、实际启动/加载烟测和完整 SHA 记录。详见 worldedit `docs/wp17-acceptance.md` 与发行目录 `release-pair.json`。

## 0.2.0 更新

新增核心 Wiki 索引，按名称和别名匹配每次出现，支持 Unicode、最长词优先、同名候选与源文件位置反查。词条释义和别名通过 Project 事务保存，失败回滚，索引不改变运行指纹。

运行时正文和选项可返回可选 links 字段，保留插值及文本粘接后的显式链接目标和 UTF-8 字节范围；CLI 与 JSON-RPC 同步透传。Rust 调用方需适配 Output::Text 与 ChoiceView 的新增 links 字段；无链接时 JSON 省略该字段。worldline 156 项测试、格式检查和严格 Clippy 通过。
