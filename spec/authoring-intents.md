# 就地建档与引用组合意图（CAP-01A）

组合意图复用 Project 实体写入、正文显式链接与地图 CreatePlacement，不定义新的语言语法。公开入口为 `Project::preview_authoring_intent` 和 `Project::apply_authoring_intent`；CLI 与 agent RPC 接受同一份可序列化 `AuthoringIntent` DTO，不维护第二套写入语义。

命令包含 `expected_baseline`（Project 内容基线）、目标（已有 `TargetRef` 或新实体及其已载入源文件）、可选正文选区、可选地图入口。至少指定正文或地图入口之一。新实体 ID 与地图入口 ID 分开传入，不从显示名、坐标或图层名称推导身份或语义关系。

正文选区使用已载入活动源文件的绝对路径和 UTF-8 字节范围 `[start,end)`；必须携带 `expected_text`，非空且与原选区完全一致。范围须位于字符边界，不跨行，替换为 `[[kind:id|expected_text]]`。显示文字遵守 catalog.md 的既有链接约束；无法无损表示时拒绝，不清洗、截断或吞掉转义。只允许产生目录可识别的正文/选项链接，声明、注释、属性中的同形文字不得伪装成功。

基线不符、未启用实体所需的 1.10、未知必需能力、只读、非活动或越界源码、缺失目标、失效选区、锁层或非法地图几何全部失败。预览和提交均先在候选 Project 上检查完整操作，任一步失败不改变原缓冲。提交返回受影响目标、去重的全部变更文件和新基线；取消通过丢弃预览实现。

结果的 `reference_impact` 复用 core `DeletionImpact` DTO，表示候选工程中该目标的引用全貌（正文来源、地图入口等），不是删除命令或增量；`complete` 与诊断防止把不完整分析当作没有引用。

编辑器在一次提交前后保存 Project 快照，用既有 `restore` 实现一次撤销/重做；外部刷新后仍遵守恢复代次限制。预览不刷新、不写磁盘、不推进基线，但读取已跟踪文件核对保存基线以拒绝尚未刷新的外部改动。磁盘保存继续使用 workspace.md 的可恢复协议，不承诺跨文件磁盘原子性。实体资料和地图入口不改变运行指纹；正文保持显示文字时沿用现有链接指纹规则。

## CLI 与 RPC

`wl authoring-intent preview|apply <目录或入口> --intent-json '<JSON DTO>' --json` 与
`authoring.intent.preview` / `authoring.intent.apply` RPC 使用同一个 DTO。RPC 参数为
`{path, intent}` 或 `{project_id, intent}`；`intent.expected_baseline` 必须提供。路径
字段使用绝对工作区路径，与 Rust `PathBuf` 一致。CLI/RPC 的 `apply` 在 core 候选
Project 验证完整组合后调用 `Project::save()`；保存错误沿用可恢复工作区事务，接口不
承诺多个磁盘文件同时原子替换。RPC 使用 `project_id` 时，成功后该会话 Project 也保留
应用后的状态。

`target` 使用相邻标记形式：`{"kind":"existing","value":{"kind":"character","id":"lin"}}`
或 `{"kind":"create_entity","value":{"path":"绝对源码路径","draft":{"id":"tower","entity_type":"place","display":"灯塔","description":"","properties":[]}}}`。
`selection` 对应 `{path,start,end,expected_text}`，范围为 UTF-8 字节；`placement` 对应
`PlacementRequest`，地图 geometry 使用 `MapGeometry` 的 `{kind:"point",position:[x,y]}`、
`{kind:"polyline",points:[...]}` 或 `{kind:"polygon",points:[...]}` 形状。未使用的
`selection` / `placement` 可省略或置为 null；至少一个必须存在。`properties` 是
`EntityDraft` 的 `[name,value]` 二元组数组，value 为字符串、有限数值或布尔值。

preview 成功返回 `operation:"preview"`、`target`、`reference_impact`、`changed_files`、
候选 `new_baseline` 与当前基线；它不保存候选内容。apply 返回相同影响与变更文件信息，
保存成功后基线推进。任一目标、选区或地图操作失败都返回 `ok:false` 且不写部分源码或
展示文档；RPC 协议违规（缺字段、DTO 类型错误或未知 `project_id`）仍使用 `-32602`，
合法 DTO 但组合失败使用结果中的稳定 `error.code` 与中文 message。
