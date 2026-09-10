# worldline（世界线）

面向世界设定与分支叙事创作的 Rust 语言工具链。语言 v1.9 包含世界、人物、故事线、事件/场景、时段偏序、状态标签、前后效果、独立叙事锚点、分类、附件、别名与正文链接。工具版本 0.2.0，新增 Wiki 关键词索引、出现位置反查和运行时链接范围。

## 开始使用

```powershell
cargo build --workspace --release --locked
./target/release/wl check examples/harbor-world --json
./target/release/wl catalog examples/harbor-world --json
./target/release/wl play examples/harbor-world
```

将整个作品目录作为参数，会递归分析全部 `.wl`，以根目录 world.wl 为入口。单文件参数适合独立示例，只读取入口及 include。所有引用都必须留在入口工作区中。

## 文档

- [创作手册](docs/handbook.md)：从目录、语法、人物、事件到状态、锚点、协作与交付的完整教程。
- [语言规范索引](spec/README.md)：语法、语义、诊断、关系、状态、目录与协议的唯一真源。
- [API 与工具接入](docs/api.md)：CLI、JSON-RPC、Rust Project 与运行时入口。
- [工作区契约](spec/workspace.md)：递归索引、引用边界、刷新冲突与完整导出。
- [发布说明](docs/release.md)：源码仓库、构建、打包与产物。

## 目录

| 目录 | 内容 |
|---|---|
| core | 词法、解析、分析、文档缓冲和结构创作 API |
| runtime | 演练状态机、选择、效果和存档 |
| cli | wl：check / play / graph / timeline / catalog |
| agent | wl-agent：stdio JSON-RPC 机器会话 |
| spec | 完整语言与机器契约 |
| docs | 教程与接入文档 |
| examples | 独立故事与多文件示例；harbor-world 是完整世界工程 |

## 开发

```powershell
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo doc --workspace --no-deps --locked
```

本目录可作为独立 GitHub 仓库构建，无需父目录 Cargo.toml，也不依赖 worldedit。工具链版本固定在 rust-toolchain.toml，依赖锁定在 Cargo.lock。编辑器仓库 worldedit 可与本仓库同级放置。许可证见 [LICENSE](LICENSE)。
