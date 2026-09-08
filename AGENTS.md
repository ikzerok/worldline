# worldline 仓库约束

语言真源为 spec/。改语言、诊断或协议先更新对应规范，再修改 core/runtime 并补充行为回归。文档中文，标识符与诊断 code 英文，诊断 message 中文。

分析、目录、关系图和时间线统一由 core 生成。runtime 只依赖 core；cli/agent 依赖 core/runtime，不依赖 worldedit。主要实现语言为 Rust。

每个作品是受限工作区，递归索引子目录源码，引用和附件不得越界；实现前读 spec/workspace.md。运行与指纹改动读 spec/semantics.md 和 spec/states.md；机器接口读 spec/agent-protocol.md。故事失败是 ok:false，协议违规才用 JSON-RPC error。

在本目录执行 cargo fmt --all -- --check、cargo test --workspace --locked、cargo clippy --workspace --all-targets --locked -- -D warnings。构建使用 cargo build --workspace --release --locked。完整使用路线见 README.md 与 docs/handbook.md。
