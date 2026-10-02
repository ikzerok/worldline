# worldline 0.15 验证事实摘要（开发候选）

核对时间：2026-10-02 16:59 UTC。本文记录 worldline 已经执行的检查及其适用范围，产品仍为未正式发行的开发候选；配对编辑器的验证与待验范围由其单独摘要记录。本页不构成 0.15 整体验收完成或正式发行结论。功能与限制见 [CHANGELOG](../CHANGELOG.md)，行为和数值门槛以 [矢量场景](../spec/vector-scene.md)、[静态世界站](../spec/reader-site.md)、[快照](../spec/workspace-snapshot.md)及[共同契约第 11 节](../spec/presentation.md)为准。

本文指向 worldedit 的跨仓相对链接仅用于 `worldline` / `worldedit` 同级检出；Windows CI 链接为对应 GitHub 运行。

## 已完成的自动检查

| 检查 | 已核实结果 | 范围 |
| --- | --- | --- |
| Linux workspace 测试 | 925 通过，0 失败，3 项显式忽略 | core、runtime、CLI、agent 与文档测试合计；不把 ignored 计为通过 |
| Linux 格式检查 | 通过 | `cargo fmt --all -- --check` |
| Linux 严格 Clippy | 通过 | workspace 全 targets，警告视为错误 |
| WASM 严格 Clippy | 通过 | core、runtime，`wasm32-unknown-unknown`；不是浏览器执行结果 |
| Linux workspace release 构建 | 通过 | `cargo build --workspace --release --locked`，含 `wl` 和 `wl-agent` |
| Windows exact CI | 925 通过，0 失败，3 ignored；源码行数、格式、严格 Clippy 均通过 | 下述精确提交和运行 |

Windows 证据为 [run 37030107038 / check 110914504739](https://github.com/ikzerok/worldline/actions/runs/37030107038/job/110914504739)，最终状态 success。PR head 为 `90000754940c9b8630d294732912c9a9daea79d3`，实际测试的合并提交为 `75bf64483aaebbea65a45a947c31928c6d2824da`，合入基线 `3074863a00811b1131c2f3d78c1a2e2e968527bd`。运行环境是 Windows Server 2025，镜像 `windows-2025-vs2026` / `20260925.250.1`，Rust `1.98.0-x86_64-pc-windows-msvc`。此结果包含 profile 等价索引修改，reader 定向普通回归为 14/14；不沿用较早 head 的成功结论。此 CI 不启动编辑器，不证明 Windows 原生交互、文件选择器或浏览器体验。

关键回归已经在完整测试中执行：

- 矢量场景：原生曲线、组、文字、仿射、根 viewport 裁剪和安全 SVG 往返；未知字段保留、显式 legacy 迁移、只读保护、批次陈旧/错误/取消零变更，以及节点、层级和导入图元额度
- 场景协议：CLI 4 项、agent 5 项，覆盖 core 预览透传、一次 apply、导出、会话修订、能力发现、错误参数和 SVG profile 拒绝；失败不会获得任意候选字节写入权限
- 世界站协议：CLI 和 agent 各 2 项，比较预览与 core DTO、实际输出的完整文件集合与每个文件字节，检查 typed 页面、别名索引、私有 CANARY 隔离、旧摘要和已存在目标拒绝
- 世界站核心：v3 类型投影、精确地图白名单、稳定路径、资源闭包、逐字段授权、profile 保存/迁移及 v1/v2 兼容；引用不扩大公开范围，未公开数据和作者备注不能因反链进入包
- 快照：Windows `project_snapshot` 6 项通过，包含原生反斜线相对路径、嵌套路径/墓碑重建及盘符、UNC、父路径等拒绝；Linux 完整门另覆盖字面反斜线拒绝。坏 UTF-8、只读文档、草稿和删除态按精确基线重建

## Release 性能测量

以下为本轮 0.15.0 候选的最新 core release 复测。世界站构建和 SVG 门槛通过；profile 等价索引优化后的 core 与完整 UI 提交复测均未超限，对应完整回归及上述新提交 Windows 检查已通过。

机器为 Linux `6.18.44` / x86_64，CPU 型号 AMD EPYC 9V74 80-Core Processor，进程可见及 affinity 为 9 个逻辑 CPU；共享主机，未确认独占物理机器或 cgroup CPU 配额。Rust `1.98.0 (88d9e12ae 2026-08-18)`，LLVM `22.1.8`。构建使用 jobs=2、incremental=0，dev/test debug=0；下表计时均来自 optimized release 测试，构建耗时不在业务计时内。

| 固定负载及计时范围 | 已测数值 | 解释 |
| --- | --- | --- |
| 1000 个 SVG 矩形，parse + plan + apply，5 份新 Project | 60.715 / 45.102 / 44.884 / 45.552 / 46.755 ms | 每轮均低于 3 s 门槛 |
| 同 SVG 负载，最大进度回调间隔 | 18.046–31.013 ms | 每轮低于 250 ms；不是操作系统窗口响应测量 |
| 同 SVG 负载，收到取消请求至返回 | 0.198–0.275 ms | 测试同时断言基线不变，低于 500 ms |
| v3 世界站，2000 对象，preview + build，5 份新工程 | 394 / 345 / 344 / 350 / 355 ms；中位 350 ms | 每轮低于 5 s；每包 2018 文件、4,667,362 原始字节 |
| 同世界站，导出取消响应 | 31–33 ms | 新目标未出现，暂存清理，原工程不变 |
| core profile apply | 216 / 200 / 199 / 200 / 197 ms | core 阶段测量；完整前台另测 |
| D5 目录关系查询：1000 对象、3000 关系、16000 字符正文 | 首次查询 46.747 ms；20 次重复查询 P95 48.647 ms | 每次总结果 1000；这是采样记录，测试未断言耗时门槛 |

“新工程”指重新构造 fixture / Project；进程和操作系统缓存没有清空，因此不能称作磁盘冷启动。SVG 第一次测量与后四次分开保留，但没有独立冷缓存控制。D5 日志中的 cold 只表示该 Project 首次查询，不是清空系统缓存后的启动。世界站负载含 1600 地点及逐字段授权、200 事件、100 关系、99 时期、1 变量，共 2000 公开对象和别名，另有未公开 CANARY；这个性能 fixture 没有地图或媒体，相关行为由独立功能测试覆盖。

profile 创建加规划为 333–342 ms，不能放在需要 250 ms 响应的前台同步路径；配对编辑器将规划放到后台。本测试对世界站构建耗时执行断言，对 profile 耗时只记录超限标志，所以测试进程成功本身不能证明前台通过。优化前一轮曾测到 core profile apply 269 ms；保留这次超限事实，优化后增加 v1/v2/v3 与原线性算法的逐字节对照，重新执行五轮 core 和十次完整 UI 提交。完整前台最新为 192–239 ms，计时范围与余量见[编辑器验证摘要](../../worldedit/docs/verification-0.15.md)。

## 可复现入口与 fixture

在对应源码检出中运行。普通完整测试的三个 ignored 项表示默认跳过的性能采样；本轮已分别按下列 release 命令执行，结果见上表，不与普通回归总数混为同一轮统计。

```sh
rustc --version --verbose
python3 scripts/check-source-lines.py
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy -p worldline-core -p worldline-runtime --target wasm32-unknown-unknown --locked -- -D warnings
cargo build --workspace --release --locked

cargo test -p worldline-core --test project_snapshot --locked
cargo test -p wl --test scene_protocol --test reader_world_site_protocol --locked
cargo test -p worldline-agent --test scene_protocol --test reader_world_site_protocol --locked

cargo test -p worldline-core --release --locked --test vector_scene scene_import_release_budget -- --ignored --nocapture
cargo test -p worldline-core --release --locked --test reader_world_site release_reader_2000_preview_build_five_fresh_fixtures -- --ignored --nocapture
cargo test -p worldline-core --release --locked --test catalog_queries d5_profile_1000_objects_3000_relations_and_long_text -- --ignored --nocapture
```

性能 fixture 的生成逻辑随正式测试保存：[1000 图元 SVG](../core/tests/vector_scene/performance.rs)、[2000 对象世界站](../core/tests/reader_world_site/performance.rs)和[D5 查询](../core/tests/catalog_queries/filters.rs)。它们固定数量和内容，无需另取外部测试数据；普通测试目录还含 CANARY、地图、profile 和机器协议 fixture。

如需保留世界站供独立浏览器验收，在执行上述世界站 release 命令前将 `WORLDLINE_READER_QA_OUTPUT` 设为仓库外尚不存在的绝对目录；测试为第 5 轮保留 `workspace/` 和 `site/`。目标不得处于任何 `.git` 祖先之下。作者工程含合成私有 CANARY，公开站点不得含 CANARY。保留输出只证明生成结果可供检查，不能直接记为浏览器通过。

## 未验证及适用边界

- 本页 Windows 结论只适用于明确列出的 head / merge / run；完整配对检查与发行资产状态见编辑器单独摘要，不能由 worldline 单仓成功推导为通过
- 实际 `file://` 打开此前被工具安全策略拒绝；本轮正常 HTTP 访问重试返回 `ERR_BLOCKED_BY_CLIENT`。离线页面视觉、真实搜索/链接/地图交互、浏览器 Worker 仍未验；没有绕过这些限制
- 世界站静态字节、资源与 CANARY 检查，不替代浏览器 JavaScript 执行或真实网页布局验收
- 配对编辑器本轮 Linux 原生功能会话已完成 21 分钟，修补构建另完成局部悬停及 5000 节点原生 CPU 采样，详情见[编辑器验证摘要](../../worldedit/docs/verification-0.15.md)；Windows/macOS GUI 和物理 IME 等未验，不能由本仓测试总数推导为通过
