# 状态与事件效果（v1.9）

状态是有独立稳定 ID 的作者对象。每个状态引用一个完整对象（包括 tag），内容为一组 tag ID；同一对象可以拥有多个彼此独立的状态。正文与属性仍从属于对象，不单独寻址。指向 tag 时指向标签本体，不会自动改写该标签指向的全部对象。

```worldline
tag calm as "平静"
tag alert as "警觉"
character lin as "林舟"
state mood on character lin with calm as "心境"

event arrival with lin after has(mood, calm)
  effect on enter
    become mood with alert as "听见警报"
  effect on exit
    become mood with calm
  林舟走进港口。
  -> END
```

- 声明：`state ID on KIND TARGET with TAG_ID[, TAG_ID…] [as "名称"]`。空集合写 `with []`。文件目标使用引号路径，相对声明文件解析。状态 ID 在整个工程的状态命名空间唯一；`state` 本身也是可标记和附加文件的完整对象。
- 动作：`become STATE_ID (with | add | remove) TAG_ID[, TAG_ID…] [as "变更说明"]`。`with` 完整替换内容，`add` 增加标签，`remove` 移除标签；后两种操作保留其他标签。`with []` 清空，`add []` / `remove []` 不改变内容。三种操作均可写在事件正文、选择/条件/场景内或效果块中，标签按集合去重。
- 前置要求沿用事件 `after` 的布尔表达式，可使用 `has(STATE_ID, TAG_ID)` 与 `seen` 等组合。`has` 两个参数为静态标识符或字符串，直接查询状态当前内容，不递归解引用标签。旧 `perm` 写法只作为迁移输入，见下文。
- 前置效果 `effect on enter [if 条件]`：准入要求通过后、正文之前执行。
- 后置效果 `effect on exit [if 条件]`：事件自然完成、跃迁到另一个事件（包括同事件重新进入）或 END 时执行一次；同一事件内场景跳转不触发。先执行源事件后置效果，再检查目标事件前置要求；准入失败保留已发生的源事件效果。
- 原有 `effect on done` 保持仅自然完成的含义；自然完成时先 done 后 exit。新后置效果不静默改变旧作品。
- 同一时机多个效果块按声明顺序执行，各条件在执行该块时求值。无序事件不会自动运行，时段与先后关系不决定运行路径。

编译分析提供 `catalog.states`：声明、初始标签与按状态 ID 聚合的 `StateChangeSite`（操作 `kind`、事件 ID、节点、时机、标签、说明、文件、行号、条件上下文）。`kind` 对应 `Become` / `AddTags` / `RemoveTags`；`tags` 是本次操作的参数，不能把 add/remove 的参数当作完整结果。它是作者的变更索引，不把条件分支和无序事件伪造为已经发生的唯一历史。展示顺序是来源顺序，不表示时间顺序。

可选演练提供当前状态集合及按实际发生顺序追加的 `state_history`，包含操作 `kind`、状态 ID、变更前后标签、事件/节点、说明、轮次；同值替换、重复增加及移除不存在的标签均记录明确动作。存档保存当前值和历史；旧历史记录缺少 `kind` 时按 `Become` 读取。状态声明、初始内容以及变更动作参与程序指纹；旧权限迁移的指纹兼容规则见下文，其余未使用状态的旧作品沿用原策略。导出的完整工程包含本规范。

调试重放可附带每一步的状态观察与只读条件解释，检查点严格绑定 runtime/schema 与程序 fingerprint；这些是单次执行的证据，不替代 `state_history`，也不写入作者工程。重放覆盖只列出实际访问的节点与实际选择，不能据此声称其他分支不可达。完整 DTO 见 [replay.md](replay.md)。

只校验状态/标签/对象引用、ID 唯一性与语法，不判断世界设定是否合理或“吃书”。新增诊断 A216 用于状态声明或变更的结构错误。

## 身份权限统一迁移

身份是状态内容的一种用途，不维护第二套权限集合。新作品可自行声明世界状态，并通过 `become identity add witness`、`become identity remove witness` 和 `after has(identity, witness)` 表达身份变动与要求。

旧作品中的全部权限（不仅角色相关权限）统一映射到世界下显示名为“叙事身份”的状态。已有唯一世界时复用；缺少世界时补建。生成的世界、状态和标签 ID 会避开工程已有标识符，调用方应读取迁移结果，不能硬编码内部 ID。

| 旧输入 | 归一后的含义 |
|---|---|
| `grant P` | 向叙事身份状态增加 P 对应标签 |
| `revoke P` | 从叙事身份状态移除 P 对应标签 |
| `perm(P)` | 查询叙事身份状态是否含对应标签 |
| `event E perm P after EXPR` | `after has(身份状态, 对应标签) and (EXPR)`，保留原表达式优先级与权限闸门先行的兼容行为 |

编译器先归一 AST，再分析和运行。`Project::open` 自动迁移工程缓冲；`Project::migrate_permissions() -> Result<usize, String>` 返回本次修改文件数，重复调用返回 0。迁移覆盖全部引用文件、条件、效果和文本/选择的表达式内插，保留注释、普通文字、字符串字面量及转义内插；无旧权限输入的工程不改写。迁移失败不提交修改，原文件在保存前保持原样。导出在工程副本上完成相同归一，不改动原缓冲或磁盘。

运行时 `perms` 兼容字段由身份状态映射派生。迁移记录保存旧指纹与归一后指纹的对应关系；只有这对指纹匹配才接受旧存档并把旧权限映射进身份状态。不放宽其他指纹不匹配，也不以初始值填补缺失状态。未知权限、含糊的双重来源或非空兼容权限与状态冲突会拒绝载入。迁移生成的兼容注释用于重开工程与旧档识别，不应手动修改。

## 独立锚点如何引用变化

锚点关联状态 ID 与事件 ID 后，`Catalog::anchor_changes` 从既有状态出处中选取两类关联的交集，返回借用；不复制状态内容、不保存易随编辑漂移的行号身份。未同时关联状态和事件时没有变化出处。详见 [catalog.md](catalog.md) §4。

## 语言1.11片段与动态集合变更出处

片段中的旧静态become仍检查所有状态/标签身份，按定义位置进入同一状态变更索引。StateChangeSite的可选source是完整fragment身份；event为空表示调用前无法确定实际执行事件，不把片段当事件。运行后state_history仍记录真实调用者事件和片段节点。

动态动作保留state_expression与tags_expression，Catalog.dynamic_state_changes列出所有定义位置和选择/条件上下文。仅state(id)等直接可确定目标同时挂入该状态的changes；参数或运行表达式目标不猜实例。静态tags构造可列出已知标签，表达式字段始终保留，空tags不意味着清空结果。索引不求值、不改变状态、不自动展开调用。1.11锚点可明确关联rule/fragment，状态与fragment关联的交集查询使用source身份。

## 所属对象身份与安全重构

state 声明的 `target.kind` 与 `target.id` 参与运行指纹。entity 稳定 ID 的全引用重命名若改变这些字段，core 仍拒绝提交；错误给出受影响 state 与声明位置、旧/候选指纹及旧 Story 存档/检查点不匹配原因。入口 replay trace 仍按既有协议允许在新指纹上受控重放，必须重新验证，不能保证沿用。可保留 entity 稳定 ID，只改 `as "显示名"`；显示名不进入实体运行身份。本轮不迁移旧 save/replay，也不移除或放宽任何指纹检查。

当前状态分析不接受 relation 作为有效所属对象，仍报告 A216；本轮不扩展支持范围。运行身份拒绝的实体用例与语法/引用无效的候选保持区分。

## 工具 0.20 实际动作回源

[route-comparison.md](route-comparison.md) 的瞬态证据按真实执行顺序记录Become/AddTags/RemoveTags及正式动作owner；不同于catalog静态候选，也不向旧state_history回填来源。所属世界对象仍只复用state.target。
