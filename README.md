# worldline（世界线）

世界设定与分支叙事的语言、核心分析和运行时，语言源码为唯一真源。0.32 连接普通外改协调、未应用正文隔离试演、同范围作者审稿本和世界资料巡检；配对 worldedit 使用同一 core/runtime 的结果完成作者操作。

工作分支不表示正式发行。范围和验证边界见配对编辑器[0.32版本说明](https://github.com/ikzerok/worldedit/blob/main/docs/releases/v0.32.0.md)。本轮不新增 DSL，默认语言 1.9、最高既有显式版本 1.13 保持，不自动升级作品。

升级边界：0.31 及更早运行轨迹与运行检查点（ReplayTrace / ReplayCheckpoint）按既有 runtime_version 守卫拒绝在 0.32 重放、比较或生成已验证审阅，应重新录制。合法路径 JSON 只读导入成功不代表可执行；普通 Story Save 继续按格式、能力和指纹独立校验。升级前保留完整工程和原记录。

应用与源码发行包不附带样例工程或规范演示文件。新建作品从仅含起点与结束的空白骨架开始；必要测试、可选产品字段模板与既有作品能力保持。

## 0.32 同一当前稿的作者闭环

- [普通外改协调](spec/workspace-reconciliation.md)：基线、本地、磁盘三方完整身份，明确逐项决定，候选重验、撤销和另行保存
- [未应用正文隔离试演](spec/draft-rehearsal.md)：实际草稿编译和独立运行会话，真实状态与来源，返回原稿
- [筛选范围审稿本](spec/manuscript-delivery.md)：同一书稿查询的连续全分支审阅和作者私密 Markdown 交付
- [查询范围巡检](spec/catalog-scope.md)：相同不可变 typed 范围里的对象、地图绑定和正式关系，返回与过期保护

## 0.31 作者契约

- [模板设计与影响](spec/templates.md)：稳定字段身份、结构编辑、未知扩展保留、真实适用范围与完整性、原子应用
- [书稿查询](spec/manuscript-query.md)：同一不可变快照全范围检索、路径和完整数量、有界分页、草稿与外部变化保护
- [真实状态检查](spec/state-inspection.md)：当前值与确实记录的首次/前次观测，类型、不可比较状态、检索与分页
- [机器协议](spec/agent-protocol.md)：模板 draft/preview/apply、书稿 query、session.inspect 共用核心，不隐式保存或推进

## 0.30 作者契约

- [新章原子计划](spec/manuscript-authoring.md)：明确新/已有书稿与正式来源，预览影响后一次应用，保存另行执行
- [书稿与正文](spec/manuscript.md)：core 投影可信空槽，UI 与机器入口共用正式源缓冲，不另写 DSL
- [对象筛选分页](spec/object-search.md)：完整对象目录预算、类型约束、按需来源匹配与稳定分页
- [机器协议](spec/agent-protocol.md)：新章 preview/apply 与对象 search 使用同一 core，拒绝无效和过期计划

## 0.29 作者工作流

- [对象分页检索](https://github.com/ikzerok/worldline/blob/main/spec/object-search.md)：名称、ID、别名和类型使用同一个core结果
- [原始路径交换](https://github.com/ikzerok/worldline/blob/main/spec/replay-exchange.md)：实际字节预算、重复键拒绝、只读导入与运行验证区分
- [安全拥有型会话](https://github.com/ikzerok/worldline/blob/main/spec/owned-story.md)：程序快照随试玩会话释放
- [读者公开页目录](https://github.com/ikzerok/worldedit/blob/main/docs/reader-page-directory.md)：在已公开内容里直接查找与定位

## 0.28 作者工作流


- [静态可执行依赖](https://github.com/ikzerok/worldline/blob/main/spec/executable-context.md)：查看调用、读写和语境，回到真实源码
- [可读试玩审阅](https://github.com/ikzerok/worldedit/blob/main/docs/playthrough-report.md)：重新验证单条路径，预览后明确复制或导出作者报告
- [地图作者位置](https://github.com/ikzerok/worldedit/blob/main/docs/map-author-context.md)：资料、源码与地图间的有效身份返回
- [平行连接审阅](https://github.com/ikzerok/worldedit/blob/main/docs/parallel-edges.md)：准确数量、完整分支条件与逐条来源

## 使用入口

- [连续键盘试玩](https://github.com/ikzerok/worldedit/blob/main/docs/keyboard-play.md)：成功推进后的可选项与结束动作焦点，不替作者继续选择
- [资料约束影响完整性](https://github.com/ikzerok/worldline/blob/main/spec/schemas.md)：缺失源码、已知影响与实例违规分别说明

- [真实变量写入与回源](https://github.com/ikzerok/worldline/blob/main/docs/variable-write-evidence.md)：两条路线的前后值、实际动作与来源返回

- [当前源码行列定位](https://github.com/ikzerok/worldline/blob/main/docs/source-coordinates.md)：物理位置、当前稿预览、准确跳转与返回

- [当前源码结构与可信定位](https://github.com/ikzerok/worldline/blob/main/docs/source-outline.md)：当前文件声明、精确范围、键盘跳转与作者位置返回

- [单实体声明安全移源](spec/entity-source-move.md)：精确原文、两文件事务、引用与运行等价、无变化及失败边界

- [逐处审阅后安全改稿](docs/selective-replace.md)：精确命中选择、原文上下文、预览、事务应用与失败保护

- [可信全分支作者审稿](docs/manuscript-review.md)：条件/选择分组、人物身份、静态流程与真实来源

- [世界资料批量导入与修订](docs/catalog-import.md)：UTF-8 CSV → 显式映射 → 逐行差异 → 整批应用/撤销 → 保存与幂等重导入


- [真实路线对照](docs/route-comparison.md)：两个当前稿实际结果、分层覆盖、状态动作和来源

- [工程问题报告](docs/problems.md)：当前缓冲、静态资料覆盖、主/相关来源及 CLI/RPC 分页
- [世界对象焦点上下文](spec/world-context.md)：区分正式关系、属性引用、人物参与、显式链接与可选文字提及
- [时间偏序解释](spec/temporal-explanations.md)：先后证据、真正环成员与受阻下游

- [安全源码组织](docs/source-lifecycle.md)：显式活动文件新建、受控单源码移动、语义/资源/存档证明与失败边界

- [原生矢量与 SVG](spec/vector-scene.md)：保留曲线、文字、组与变换；预览、原子编辑、显式迁移及安全交换
- [静态世界站](spec/reader-site.md)：按对象、字段、地图图元、章节和附件分别授权，生成多页离线内容；可保存发布配置
- [场景机器接口](spec/scene-protocol.md)：`wl scene` 与 `scene.*` 使用同一 core 计划；读者站沿用 `reader-export` / `reader.export.*`
- [既有能力显式启用](docs/explicit-capabilities.md)：语言与资料能力继续先预览、后确认，取消不改稿

## 开始使用

```powershell
cargo build --workspace --release --locked
./target/release/wl check "D:/作品/我的世界" --json
./target/release/wl catalog "D:/作品/我的世界" --json
./target/release/wl play "D:/作品/我的世界"
```

先在 worldedit 新建并保存自己的作品，或按[语法规范](spec/syntax.md)创建根目录 `world.wl`。将整个作品目录作为参数，会递归分析全部 `.wl`，以根目录 world.wl 为入口。单文件参数适合独立示例，只读取入口及 include。所有引用都必须留在入口工作区中。

## 文档

完整入口见 [按任务阅读](docs/README.md)；版本变化见 [CHANGELOG](CHANGELOG.md)。

- [从资料到可信重放](docs/author-route.md)：串起来源资料、偏序与倒叙、双路线、共享片段和安全改稿。

- [语言1.11规范](spec/language-1.11.md)：新语法、返回帧、纯性、类型和兼容边界。
- [创作手册](docs/handbook.md)：从目录、语法、人物、事件到状态、锚点、协作与交付的完整教程。
- [语言规范索引](spec/README.md)：语法、语义、诊断、关系、状态、目录与协议的唯一真源。
- [API 与工具接入](docs/api.md)：CLI、JSON-RPC、Rust Project 与运行时入口。
- [工作区契约](spec/workspace.md)：递归索引、引用边界、刷新冲突与完整导出。
- [安全源码组织](docs/source-lifecycle.md)：源码新建、引用和单文件安全移动；core、CLI 与 agent 共用逐处预览和保存恢复边界。
- [发布说明](docs/release.md)：源码仓库、构建、打包与产物。

## 0.11.0 可信流程与修订

流程检查识别可证明没有选择暂停或出口的闭环，并利用有限片段摘要区分返回与转场。普通演练有明确步数/时间预算，超限保留输出与状态供继续，不冒充正常 END；CLI和JSON-RPC的扩展结果须显式协商。参阅[有界演练契约](spec/bounded-execution.md)。

批注修订使用独立只读投影，解决状态与锚定状态分开；原有TodoProjection v1意义保持不变，批注不进入运行指纹或读者包。正文关键字示例明确使用反斜杠转义，空格只表示缩进。

## 目录

| 目录 | 内容 |
|---|---|
| core | 词法、解析、分析、文档缓冲和结构创作 API |
| runtime | 演练状态机、选择、效果和存档 |
| cli | wl：check / play / graph / timeline / catalog / scene / reader-export |
| agent | wl-agent：stdio JSON-RPC 机器会话 |
| spec | 完整语言与机器契约 |
| docs | 教程与接入文档 |

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


## 0.8.0 完整创作语言与工作区

新能力仅在显式1.11中解释；旧call/return/say正文保持原文。规则与片段都拒绝静态递归，纯规则禁止随机副作用；片段暂停存档保存每层返回地址和参数/local。新语义参与指纹，未知存档能力拒绝载入。块内let/const遵守全局声明、实际路径初始化；完整跨事件scene目标与有符号rnd按既有规范纠偏。rnd合法非负范围保持原seed序列。

配对worldedit提供正文/结构/源码同一源缓冲、书稿重组、可恢复个人布局、双参考、统一命令、可选资料列与可读性设置。未选字段、未公开引用目标及say演出备注不自动进入读者包。旧排序与v1阅读选择行为保留。本版不提供并发线程、任意对象脚本、日历计算或在线权限系统。

## 0.10.0 静态时间与人物引用

显式选择1.13后，具有同一声明时间根的事件可保留各自直接 `during` 时段并用 `follows` 连接。祖先和声明顺序不生成时间边，独立根不可互比；旧 rank仍表示直接时段内层级，新增根scope/root_rank明确根内投影。错误快照标记不完整；偏序层级不表示精确日期、持续时间或同时发生。

人物属性可直接填 `ref("character", "regent")`；持续schema与模板可限制为人物引用。需同时声明 `content.object_refs.v1` 与 `content.character_refs.v1`，普通字符串不会自动转引用。静态引用属性和时间元数据不进入运行指纹，人物自身稳定ID的重命名继续遵从旧存档契约。

参阅[语言1.13](spec/language-1.13.md)、[静态人物引用](spec/character-refs.md)、[支持版本契约](spec/language-versions.md)。

## 0.9.0 持续资料约束与锁定选择

显式语言1.12支持[持续schema与bind](spec/schemas.md)及[读者可见禁用choice](spec/choices.md)。默认1.9和显式1.10/1.11不自动升级。资料违规通过统一compiler/CLI/RPC/发布检查；schema字段变更先查看实例影响，不填值、不强转、静态约束不改变运行指纹。锁定选择采用独立presentation能力协商，旧choices仍只含可选项；禁用拒选零推进，全锁落穿，片段/once/存档/重放闭环。

配对worldedit提供[当前稿安全查找替换](spec/search-replace.md)、标准编辑命令与当前章节只读预览。core投影未应用WritingBuffer，坏稿保留并明确标记预览过期。产品继续按0.x递进，语言版本与产品版本独立。

### 工程问题与可信来源（0.19）

`wl problems 工程目录 --json` 汇总当前已应用缓冲及已注册文档的静态问题，支持筛选、分页与主/关联来源；`wl check` 的既有范围和运行门禁保持不变。0.19 的 schema 1 加法 context 提供六种角色、有界原文及局部命中，0预算仍保留角色；旧报告可只读查看，导航须刷新。报告不会自动修复、保存或发布作品。用法与覆盖边界见 [工程问题指南](docs/problems.md)。
