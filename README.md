# worldline（世界线）

面向世界设定与分支叙事创作的 Rust 语言工具链。0.15.0 开发候选增加原生矢量地图、受限 SVG 交换、完整静态世界站与对应 CLI / JSON-RPC 接口。默认语言仍为 1.9，最高 1.13；旧作品不自动升级，故事运行与存档契约不因地图或发布功能改变。

本轮 Linux 与精确提交的 Windows 自动检查、固定负载 release 性能测量已通过；真实离线网页与浏览器 Worker 仍未验。具体提交、测量范围和配对编辑器结果见 [0.15 验证摘要](docs/verification-0.15.md)。当前仍为未正式发行的开发候选，完整变更见 [CHANGELOG](CHANGELOG.md)，使用入口见 [文档索引](docs/README.md)。

## 0.15 使用入口

- [原生矢量与 SVG](spec/vector-scene.md)：保留曲线、文字、组与变换；预览、原子编辑、显式迁移及安全交换
- [静态世界站](spec/reader-site.md)：按对象、字段、地图图元、章节和附件分别授权，生成多页离线内容；可保存发布配置
- [场景机器接口](spec/scene-protocol.md)：`wl scene` 与 `scene.*` 使用同一 core 计划；读者站沿用 `reader-export` / `reader.export.*`
- [既有能力显式启用](docs/explicit-capabilities.md)：语言与资料能力继续先预览、后确认，取消不改稿

## 开始使用

```powershell
cargo build --workspace --release --locked
./target/release/wl check examples/harbor-world --json
./target/release/wl catalog examples/harbor-world --json
./target/release/wl play examples/harbor-world
```

将整个作品目录作为参数，会递归分析全部 `.wl`，以根目录 world.wl 为入口。单文件参数适合独立示例，只读取入口及 include。所有引用都必须留在入口工作区中。

## 文档

完整入口见 [按任务阅读](docs/README.md)；版本变化见 [CHANGELOG](CHANGELOG.md)。

- [从资料到可信重放](docs/author-route.md)：用[栖雪山站最小示例](examples/snowline-seeds/README.md)串起来源资料、偏序与倒叙、双路线、共享片段和安全改稿。

- [语言1.11规范](spec/language-1.11.md)：新语法、返回帧、纯性、类型和兼容边界。
- [组合示例](examples/tide-rescue/README.md)：两次调用、嵌套暂停、动态证物和角色台词。
- [创作手册](docs/handbook.md)：从目录、语法、人物、事件到状态、锚点、协作与交付的完整教程。
- [语言规范索引](spec/README.md)：语法、语义、诊断、关系、状态、目录与协议的唯一真源。
- [API 与工具接入](docs/api.md)：CLI、JSON-RPC、Rust Project 与运行时入口。
- [工作区契约](spec/workspace.md)：递归索引、引用边界、刷新冲突与完整导出。
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


## 0.8.0 完整创作语言与工作区

新能力仅在显式1.11中解释；旧call/return/say正文保持原文。规则与片段都拒绝静态递归，纯规则禁止随机副作用；片段暂停存档保存每层返回地址和参数/local。新语义参与指纹，未知存档能力拒绝载入。块内let/const遵守全局声明、实际路径初始化；完整跨事件scene目标与有符号rnd按既有规范纠偏。rnd合法非负范围保持原seed序列。

配对worldedit提供正文/结构/源码同一源缓冲、书稿重组、可恢复个人布局、双参考、统一命令、可选资料列与可读性设置。未选字段、未公开引用目标及say演出备注不自动进入读者包。旧排序与v1阅读选择行为保留。本版不提供并发线程、任意对象脚本、日历计算或在线权限系统。

## 0.10.0 静态时间与人物引用

显式选择1.13后，具有同一声明时间根的事件可保留各自直接 `during` 时段并用 `follows` 连接。祖先和声明顺序不生成时间边，独立根不可互比；旧 rank仍表示直接时段内层级，新增根scope/root_rank明确根内投影。错误快照标记不完整；偏序层级不表示精确日期、持续时间或同时发生。

人物属性可直接填 `ref("character", "regent")`；持续schema与模板可限制为人物引用。需同时声明 `content.object_refs.v1` 与 `content.character_refs.v1`，普通字符串不会自动转引用。静态引用属性和时间元数据不进入运行指纹，人物自身稳定ID的重命名继续遵从旧存档契约。

参阅[语言1.13](spec/language-1.13.md)、[静态人物引用](spec/character-refs.md)、[支持版本契约](spec/language-versions.md)。

## 0.9.0 持续资料约束与锁定选择

可运行的[镜海孤岛示例](examples/lantern_archive_112/README.md)串起资料、锁定分支、书稿与静态公开边界。

显式语言1.12支持[持续schema与bind](spec/schemas.md)及[读者可见禁用choice](spec/choices.md)。默认1.9和显式1.10/1.11不自动升级。资料违规通过统一compiler/CLI/RPC/发布检查；schema字段变更先查看实例影响，不填值、不强转、静态约束不改变运行指纹。锁定选择采用独立presentation能力协商，旧choices仍只含可选项；禁用拒选零推进，全锁落穿，片段/once/存档/重放闭环。

配对worldedit提供[当前稿安全查找替换](spec/search-replace.md)、标准编辑命令与当前章节只读预览。core投影未应用WritingBuffer，坏稿保留并明确标记预览过期。产品继续按0.x递进，语言版本与产品版本独立。
