# worldline

分支叙事语言 .wl 的编译器、核心分析与工程生命周期；内容与关系的唯一真源。语言规范真源在本仓 `spec/`。

## Language

**内容身份**:
`TargetRef { kind, id }` 指向的对象身份；改显示名不断链，同名异对象不合并。
_Avoid_: 对象指针、名称引用

**展示文档**:
进入 Project dirty/refresh/restore/mark_saved 生命周期的 JSON 文档（地图、网络视图、模板、批注）；与 .wl 内容文档分域。
_Avoid_: 元数据、配置文件

**语义关系**:
作者显式声明的设定联系（relation_def），有稳定 ID 与方向；不做对称/传递推断。
_Avoid_: 关系（单用时）、关联（与 MentionReference 混淆）

**旧人物关系**:
1.9 的 CharacterRelation，无独立 ID；持久引用前须显式"提升"为语义关系。
_Avoid_: 兼容关系

**运行指纹**:
决定旧存档可用性的内容哈希口径；新 entity/relation 声明与展示文档不参与。
_Avoid_: 文件指纹、工程哈希

**纯内容工程**:
没有可运行 event 的合法工程；仅试玩入口检查可运行性，保存/打包不得拒绝。
_Avoid_: 设定集（口语化）

## 当前开发切片

WP-10 的 `graph_views` 支撑接口已加入共享布局读取、带基线事务、删除与显式引用保护。
契约见 `spec/presentation.md` 和 `spec/diagnostics.md`，14 项行为回归见 `core/tests/graph_views.rs`。
这不代表 worldedit 的网络 UI 或后续 WP-11 已完成；配对进度见同级 worldedit 的 `docs/wp10-progress.md`。
