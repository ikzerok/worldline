# worldline（世界线）

面向世界设定与分支叙事创作的 Rust 语言工具链。语言 v1.10 包含世界、人物、故事线、事件/场景、时段偏序、状态标签、前后效果、独立叙事锚点、分类、附件、别名与正文链接。工具版本 0.7.0，新增资料查询的显式名称/对象类型排序与共享定义兼容保护；保留复合条件真实求值证据、Wiki、离线阅读发布、安全 SVG 地图、文字标签与平面校准能力。

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
python scripts/check-source-lines.py
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo doc --workspace --no-deps --locked
```

源码按职责拆分为模块；上述检查将每个纳入 Git 的 Rust 源文件（包括测试）限制在 600 个物理行以内，CI 同样执行。

本目录可作为独立 GitHub 仓库构建，无需父目录 Cargo.toml，也不依赖 worldedit。工具链版本固定在 rust-toolchain.toml，依赖锁定在 Cargo.lock。编辑器仓库 worldedit 可与本仓库同级放置。许可证见 [LICENSE](LICENSE)。

## 0.7.0 资料查询排序

资料组合查询支持名称或对象类型的单字段升降序，全量排序后分页，并列对象保持稳定身份顺序。显式排序使用 v2 查询；v1 默认查询与 page/cursor 格式保持不变。共享查询仅在定义文档声明 `catalog.query_sort.v1`，恢复默认时清除本功能字段；旧版拒绝执行未知查询版本并只读保留定义。名称采用 ASCII 不区分大小写的 Unicode 顺序，不提供拼音或数字自然排序。详见 [资料查询契约](spec/catalog.md#71-显式结果排序)。
