# 工具接入与 API 导航

## 构建与输出

在 worldline 仓库根目录执行 `cargo build --workspace --release --locked`。可执行程序为 `target/release/wl` 与 `target/release/wl-agent`，Windows 带 `.exe`。包名 `worldline-agent` 与二进制名 `wl-agent` 不同。Rust 版本由根目录 rust-toolchain.toml 固定。

`cargo doc --workspace --no-deps --locked` 生成公开 Rust API 文档。源码入口：core/src/lib.rs、runtime/src/lib.rs 与 runtime/src/model.rs。依赖严格为 runtime → core，CLI/agent → core + runtime。

## 选用入口

| 任务 | 入口 | 说明 |
|---|---|---|
| 一次检查 | `wl check DIR --json` | 完整目录；退出码 0/1/2 |
| 执行关系 | `wl graph DIR --json` | 节点、边、条件上下文、目标准入 |
| 时间偏序 | `wl timeline DIR --json` | 时段、事件、follows 与图 |
| 作者资料查询 | `wl catalog DIR --json` | 人物、1.10 实体、标签、状态、锚点、附件、别名、链接 |
| 作者资料编辑 | `wl entity create\|update\|delete DIR ... --json` | 1.10 实体的创建、资料更新与安全删除 |
| 终端演练 | `wl play DIR` | 人类选择序号从 1 起 |
| 脚本演练 | `wl play DIR --json` | 输入使用输出给出的 index，从 0 起 |
| 多轮机器会话 | `wl-agent` | 每行一个 JSON-RPC 请求，保持进程存活 |
| Rust 结构编辑 | `Project::edit` 与各 draft 方法 | 全工程校验，失败回滚 |

JSON 消费者应按字段语义读取，不依赖映射键序。诊断失败不意味着输出无法解析。每个子命令的 JSON 形状不同，完整方法、字段、错误码和示例见 [agent-protocol.md](../spec/agent-protocol.md)。CLI 当前没有通用 `--help` 子命令；无参数会输出用法并返回 2。

## 编译与文档缓冲

`compile_source(file, text)`、`compile_path(path)` 和 `compile_sources(entry, ...)` 保持
1.9 默认；需要解析 entity 时使用对应的 `*_with_options` 入口并传入
`CompileOptions::v1_10()`。CLI 目录若有 `.world/project.json` 会读取清单版本，
也可传 `--language-version=1.10`。`CompileResult.options` 记录最终选择的版本。

`compile_source(file, text)` 处理单源，不读取 include；`compile_path(path)` 读取磁盘，目录路径递归载入工作区；`compile_sources(entry, &BTreeMap<PathBuf,String>)` 优先使用内存覆盖并加载工作区内 include，同时分析提供的其他内存源码。外部路径不进入编译结果。

返回 CompileResult 包含 program、analysis、diagnostics、sources。即使有错误也可能返回尽力解析的数据；执行前检查 has_errors，不能因为 program 非空就运行。
`CompileResult.diagnostics` 只属于故事编译域。打开工程时，清单版本、
`required_features` 和注册展示文档的作者诊断由 `Project::authoring_diagnostics()`
单独提供；CLI JSON 与 `wl-agent` 将它们序列化为同名的
`workspace_diagnostics`，并以 `read_only` 表示是否禁止作者写入。未知语言版本或
必需能力报告 `WS003`，不会污染 `CompileResult.diagnostics`。

`Project::open` 打开工作区、先恢复 `.world/.transactions/` 中的未完成保存，再将旧权限迁移到缓冲；`Project::new(root)` 建立未保存的雾港示例。`documents` 保存源码与保存基线，`sources` 返回当前文本映射。`refresh` 更新磁盘变化并返回冲突路径；`search` 搜索缓冲，每个命中行返回一次，列号按 Unicode 字符。`save` 以逐文件可恢复事务写入，跨文件不宣称原子性；打开时若目标同时不同于事务前后 hash，会保留第三方值与事务草稿，`recovery_conflicts` 返回冲突路径，普通保存、另存和导出等待人工处理；recovery_drafts 返回含事务身份、前后/当前 hash 与原始字节（或删除意图）的救援记录，export_recovery_drafts 显式写入工程外新目录并附 recovery.json，原工程及事务不变。`save_as` 建立新工作区，`export` 输出经校验的新目录，`export_files` 返回相对路径到字节的映射供 ZIP 使用。

调用结构编辑先准备草稿，再放进 `Project::edit` 事务；错误时全部缓冲回滚。直接 `set_text` 允许未完成源码，编辑器据此显示诊断。事务不等于多文件磁盘原子提交：保存逐个文件替换，IO 中断可能已经保存一部分，后续依保存基线恢复。

1.10 工程的 `EntityDraft` 由 `Project::write_entity` 创建或更新，
`Project::remove_entity` 删除前重新计算 `TargetRef { kind: "entity", id }`
的源码和地图引用影响；两者都应放入 `Project::edit`。显示名和分类可修改，ID
保持稳定。`wl-agent` 的 `project.open` 返回内容基线，后续 entity 编辑可回传
`baseline` 拒绝陈旧请求；CLI 的 `--baseline` 具有相同语义。两者都使用
`Project::content_baseline()`，覆盖源码、工程清单和已注册展示文档的相对路径、原始
字节与删除状态，不使用运行时 fingerprint；实体作者资料也不会改变运行 fingerprint。
声明实体的工程清单应同时声明能力，例如：

```json
{"schema_version":1,"language_version":"1.10","entry":"world.wl","required_features":["content.entities.v1"]}
```

长生命周期的 `project_id` 请求在 `project.analyze` 和 `entity.*` 前先刷新磁盘源码及
清单注册的展示文档，并返回刷新后的 `baseline`。分析遇到刷新冲突仍提供目录和
`conflicts` 供恢复；刷新读取失败返回 `ok:false` 的 `IO_ERROR`，实体写入遇到刷新
冲突返回 `ok:false` 的 `CONFLICT`，调用方应处理后使用新基线重试。
`project.open` 和 `project.analyze` 即使遇到工作区只读诊断，仍返回可读的
`project_id`/`catalog`；这时 `diagnostics` 仍只放故事编译诊断，新增的
`workspace_diagnostics` 会列出 `WS003` 等原因，`read_only` 为 `true`，后续
`entity.*` 以故事层失败返回而不写盘。`wl check --json` 对同一状态返回
`ok:false` 和退出码 1，并在人类模式打印中文只读提示；`wl catalog --json`
可继续返回目录并保留 `ok:true` 与 `read_only:true`。

## 分析与创作模块

`workspace_snapshot::snapshot_files(&Project)` 返回当前工作区的相对路径到原始字节映射，用于保存草稿包；它不编译或迁移源码，保留注册展示 JSON、普通文件和未知字节，排除删除墓碑，不读取事务日志作为作者文件。未解决保存事务会阻止普通组包；需要救援时使用上述显式原稿接口。严格发布导出仍使用 `Project::export_files()`，要求源码编译通过且声明的附件可读。`workspace_snapshot::project_entry` 负责读取新清单中的可选入口，复用 core 的无重复键 JSON 解析；旧宿主入口与新入口的冲突由包导入方明确拒绝。

| 模块 | 职责 |
|---|---|
| analysis | 符号、诊断、图、时间线、目录的统一快照 |
| authoring | 世界、人物、事件草稿、结构改写与引用更新 |
| catalog / catalog_edit | 完整对象、标签、素材、标记、附件查询与修改 |
| states | 状态定义、集合操作及源码变化出处 |
| anchors | 独立叙事锚点、关联、变化交集 |
| navigation | 别名、正文链接、对象搜索与资料导航 |
| relation_context | 显式分支条件与目标准入上下文 |
| timeline | 时段与 follows 偏序 |
| migration | 旧权限输入归一与指纹兼容 |
| file_access | 桌面磁盘 / 浏览器导入文件、目录遍历与边界检查 |

Analysis 属于编译快照，改稿后重新编译再取 ID 与源位置。行号仅用于定位，不作为持久身份。图节点数组下标只能在同一快照中使用。状态源码变化与运行历史是不同对象，不互相替代。

## 运行时

以 program 和 analysis 建立 Story，`continue_story()` 推进到选择或结束，`choices()` 查询可选项，`choose(index)` 选择，`state_view()` 取机器状态，`save()` 生成存档字符串。运行时借用编译产物，调用方须让产物活得比会话更久。保存与恢复边界见 semantics；外部进程需要持久会话时使用 wl-agent，而不是重复启动后复用失效的 session_id。

运行失败可能包括准入拒绝、动态表达式问题、执行步数限制及无效存档。按实际 RunError 报告位置和信息，避免将所有错误都解释成语法错误。

## 编辑器边界

这些工具能创建或修改 `.wl` 并检查、演练作品，但没有运行中编辑器的窗口控制协议。切换视图、布局拖动、撤销重做和文件对话框仍是 UI 操作。磁盘改稿由桌面自动刷新接入；需要确认画面效果时，应实际查看编辑器，不能拿 CLI 成功替代 UI 验证。


## 时段与选择草稿

`Project::write_period_with_parent(id, display, parent)` 设置时段直接上级，放入 Project::edit 以验证层级；旧 write_period 保留现有上级。`Timeline::period_order()` 返回稳定的父先子后顺序及深度。

`EventDraft::choices()` 提取包含嵌套结构的 ChoiceDraft；write_choice 按当前草稿行局部替换，None 新增，remove_choice 删除选择块。ChoiceDraft 包含 line/depth/label/once/condition/body/target/drift；line 只用于当前草稿定位。末尾直接出口单独建模，内部条件跃迁保留在 body。草稿允许未完成输入，最终用 Project::edit 与 write_event 校验提交。

## 冲突快照

`Project::conflict_snapshots()` 只读比较脏缓冲、保存基线和当前磁盘，返回 `ConflictSnapshot { path, baseline, local, disk }`。后三者为原始字节的可选值，`None` 表示该方文件缺失；无效 UTF-8 不转换、不丢弃。查询不刷新工程或推进保存基线，读取失败返回错误。界面按用户请求捕获一次，重新打开时再读取。

## 删除影响计划

`Project::deletion_impact(&TargetRef)` 只查询当前缓冲，返回目标是否存在、语言引用来源、地图目标标记（map_placements）、作用域标记（map_scopes）、底图素材引用（map_rasters）、诊断及检查是否完整。`DeletionImpact::can_delete()` 仅在目标存在、检查完整且没有引用时为真。界面可对同一已缓存快照调用 `reference_impact::deletion_impact(content, maps, target)`，不必在绘制时重新编译。

语言层的删除反查由 `deletion_content_references::content_deletion_references(content, target)` 提供；它读取同一 `CompileResult`，覆盖目录引用、别名、顶层标记/附件以及 `seen`/`visits`、`has` 和变量条件，并排除随事件块一同删除的内部来源。调用方应把返回结果与展示层地图引用合并后再作删除判断；当前结构删除命令是事件删除，其他对象类型仍需由对应编辑命令声明删除范围。

`remove_event` 在修改前重新生成计划；旧查询结果不授权后续写入。引用未解除或损坏内容/地图使检查不完整时拒绝删除，不隐式删除标记或其他资料。调用方可取消操作，或先显式修复、重新绑定/解除引用，再发起新的删除。
