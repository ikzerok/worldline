# 语言 1.12：持续资料约束

本规范先于实现定义语义。默认编译仍为 1.9，显式 1.10/1.11 不升级；新声明只在显式 `language_version: "1.12"` 或 `CompileOptions::v1_12()` 中识别。未知语言版本的工作区由既有清单保护按只读保留。强引用仍另需既有 `content.object_refs.v1` 能力；schema 不扩大 ref kind。

## 声明与身份

```worldline
schema city for entity entity_type place closed
  field population_id population number required
  field founding_id founding_year number required
  field mayor_id mayor ref entity entity_type organization
  field category_id category enum "城市" "城镇"
  field public_id public boolean
  field note_id note text
bind entity harbor to city

entity harbor kind place as "港城"
  property population = 0
  property founding_year = 1200
  property public = false
  property note = ""
```

- `schema ID for KIND [entity_type SUBTYPE] [closed]` 是顶层声明。KIND 仅为 world、character、entity、relation；entity_type 仅可用于 entity。ID 在工程 schema 命名空间唯一。
- 块内仅 `field FIELD_ID KEY TYPE [required]`，TYPE 为 text、number、boolean、enum 或 ref。FIELD_ID 是 schema 内稳定身份，KEY 是实例已有 property 键，二者分别唯一。改 KEY 不更换 FIELD_ID，也绝不自动改已有 property。
- enum 后跟一个或多个带引号字符串，不接受表达式、数值或布尔枚举。重复枚举值是声明错误。required 位于类型完整子句后。
- ref 后必须写 entity 或 relation；entity 可再写 `entity_type SUBTYPE`。目标存在性由原强引用校验器处理，schema 额外核对完整 kind 和可选实体子类。文本不会被提升为引用。
- `bind KIND ID to SCHEMA_ID` 是顶层显式绑定。每个对象只能绑定一次（即使绑定相同 schema 也报错）；对象和 schema 均须存在且 kind/subtype 适配。不提供 kind 全局覆盖、自动绑定、继承或合并。
- schema 与 field ID 是静态约束身份，不成为可执行状态，也不新增 TargetRef kind。schema 列表及字段、绑定投影保留 file/line 源位置；对象绑定同时进入 catalog.references，参与既有对象重命名和删除影响。删除绑定对象须先显式去除绑定；schema/field 的改名、移除使用源码编辑影响计划，未同步的绑定诊断报错，不猜测重命名意图。

## 唯一校验与诊断

所有编译入口复用 core `schemas::validate`：内存、磁盘、工作区、当前稿、CLI/RPC 与发布前检查使用同一结果。声明/绑定错误与实例违规均为 error；Program 和原文仍保留，源稿可应用、保存和撤销。执行和公开读者包沿用现有 error 阻断。工程备份/另存仍可保留错误草稿，不冒充发布成功。

required 只判断该 key 是否实际存在；0、false、空字符串是已填写。number 只接受有限数字；text 不禁止空字符串。没有 null、默认填值、强转、数值区间、任意嵌套、计算字段或动态脚本。closed 默认关闭；显式开启才对不属于字段集合的 property 键报错。旧 property 和模板语义不变。

诊断代码：SCH001 声明语法；SCH002 重复 schema/字段身份或键；SCH003 绑定不存在/重复或类型不适配；SCH004 必填缺失；SCH005 标量类型；SCH006 枚举；SCH007 ref kind/subtype；SCH008 closed 未知键。实例诊断指向 property 行，missing 指向实例声明，related 指向约束声明。重复绑定关联首个绑定的文件/行。重复实例属性仍使用既有 A212（relation 为 A220），不会通过 schema 选择首项或末项使重复声明合法；含重复对象/属性的影响预览标记不完整。

## 编辑与影响事务

`Project::schema_index()` 返回声明、绑定、实例状态及诊断。
`preview_schema_edit(&SourceEditRequest)` 使用已有 DTO 的 schema_version=1、相对 .wl 路径、expected_baseline 和完整 source，返回可序列化 `SchemaEditPreview`：expected_baseline、plan_digest、before_diagnostics、after_diagnostics、field_changes、instance_impacts、changed。

预览在 Project 副本应用候选，不改原稿；字段变化按 schema ID + FIELD_ID 对齐，列新增/移除/键名/类型或约束改变；受影响对象包含绑定前后 schema 和前后诊断。无效候选照样展示，不把不完整解析推断为无影响。

`apply_schema_edit(request, plan_digest)` 重新构造计划，校验当前内容基线、磁盘冲突、只读/能力边界和摘要，整笔仅修改内存；拒绝/取消零修改。旧 schema 或实例值不会自动补写、删改或强转。调用方保存 Project 前后快照，统一撤销/重做；错误草稿允许应用和保存，发布仍阻断。完整源码包含多处 schema/绑定时作为同一文件的原子事务；跨文件修改继续使用工作区既有事务而不伪称本 API 支持隐式跨文件重写。

schema 声明、字段约束、绑定及其位置不进入 runtime fingerprint。1.12 本身不额外混入版本盐；未使用运行新增能力时，仅增加、编辑或删除静态约束不得使存档失效。约束不授权公开任何属性、schema 文本或引用目标，读者包仍严格使用既有白名单。
