# 工具 0.35：同一 schema 的资料作者闭环

本契约先于实现。语言维持既有显式 1.12/1.13；不新增语法、不升级旧工程、不把模板转成约束。

## 单对象共享投影

`Project::schema_authoring(&TargetRef)` 返回 `SchemaAuthoringProjection`；目标必须是完整
world/character/entity/relation 身份，不能用显示名或省略 kind 消歧。
`schema_authoring_from_compile(&CompileResult, &TargetRef)` 核验当前活动源码和编译能力与
给定编译完全一致后，从其 Program、catalog 与正式 diagnostics 构造同一结果，不再次
compile/validate。`SchemaIndex::from_compile` 同样复用既有编译；界面可按内容快照缓存，
空闲帧、滚动、折叠、主题和对象选择不得触发完整重新编译。

投影提供 schema_version=1、baseline、target、object、当前 binding/schema、相容
compatible_schemas、fields、对象 diagnostics、complete/incomplete_reasons、writable
和 read_only_reasons。来源为 file/line/column，行列一基。schema 和字段保持声明顺序；
候选采用 catalog 稳定顺序与完整 TargetRef，仅按正式 SchemaType.ref kind/subtype 过滤。

字段提供稳定 id、key、value_type、required、真实 present、value、property_source、
constraint_source、diagnostics、reference_candidates。缺失为 present=false/value=null；
0、false 和空字符串为 present=true，绝不填入默认值。存在但类型错误的值保持原值。
字段诊断通过正式 property/约束来源关联，不解析中文消息。对象诊断包含未知属性、
绑定和对象来源问题；related 继续指向约束来源。

完整性沿用 schemas.md 的四类原因；普通 SCH004–SCH008 和不存在的强引用 A214
不自动使对象集合未知。目标不存在或身份不唯一时直接失败；其它不完整来源可只读
查看已知字段，但不可把不完整结果用于结构化编辑。未知能力、只读和来源保护同时
显示原因，不因为仍有可见字段就宣称可写。

## 显式请求与原子预览/应用

`SchemaAuthoringRequest` 为 schema_version=1、expected_baseline、target、binding、
fields。binding 使用 op 标签：keep、bind{schema_id}、change{schema_id}、unbind。
bind 仅用于未绑定对象，change/unbind 仅用于恰有一个绑定对象；keep 保留现状。
fields 为 op=set{field_id,key,value} 或 remove{field_id,key}，按操作后 schema 的稳定
字段 ID 与当前 key 双重核验。一个字段只能选择一次；未绑定或解绑时不允许字段操作。
value 采用既有 PropertyValue JSON：字符串、有限数字、布尔或严格 {kind,id} 对象；
不支持 null、自动强转或隐式默认值。所有对象字段拒绝未知键，JSON 任意层重复键拒绝。

`preview_schema_authoring(&request)` 在 Project 副本中完成全部修改、编译与来源核验，
返回 `SchemaAuthoringPreview`：schema_version、expected_baseline、plan_digest、changed、
before、after、changed_files、diagnostics、has_errors，以及 before_fingerprint、after_fingerprint
（十进制字符串，避免 JavaScript 整数精度损失）和 runtime_changed。预览零修改。新绑定写在目标声明
所在文件，已有绑定在自身文件原位更换或移除，因此绑定与属性可跨文件且仍整笔提交。
只替换选中语句的正式来源范围，未选字段、未知属性、注释及其它字节保持；不重建对象。

`apply_schema_authoring(&request, plan_digest)` 重新完整预览，复用 Project 内容基线、
磁盘保存基线、活动源码、能力、恢复冲突与候选整笔替换检查。摘要覆盖请求、前后
内容基线和外部刷新代次。任何失败都不修改原 Project；不能先改属性再补绑定。
来源缺失、歧义、重复属性/声明/绑定、过期计划、字段改名、外改、未知能力或有界预算
失败均拒绝，保留输入。合法或仍含普通实例约束错误的草稿均可显式应用；has_errors
必须清楚呈现，应用/保存成功不表示语义合格，执行/读者发布维持既有 error 门禁。

应用仅改内存，不保存；调用方保存 Project 前后快照实现同笔撤销/重做，保存沿用
可恢复事务。schema/bind 本身不改变运行指纹。属性沿用既有 hash 契约：world/character
非 Ref 属性进入指纹，entity/relation 属性和显式 Ref 不进入；不为新表单悄改旧存档语义。
preview 由 core 实际计算并返回前后指纹与 runtime_changed，UI/机器消费者不能根据
kind 或字段名称猜测。runtime_changed 时必须提示既有试演/记录需要重新核验；该事实
不单独禁止显式作者稿应用。不新增存档兼容迁移，也不授权公开任何资料。

## 有界协议

最多 256 个字段操作、单 schema 256 个字段、256 个相容 schema、累计 10000 个引用
候选；请求 JSON 上限 256 KiB，单值 64 KiB，ID/key 512 字节，活动源码总计 32 MiB。
事务检查的目标声明源码以及每个被修改源码均最多 4 MiB；即使仅在其他文件改绑或解绑，
也不绕过目标声明源码的该额度。原文件和候选文件分别检查。
投影与预览序列化上限均为 8 MiB。预算计量包含字段、诊断、来源、候选和原值；
构建投影时，在克隆可变长度内容前以借用值流式计量，保守预留结构开销，因此可在最终 JSON
达到 8 MiB 前拒绝。工作区库存/保存基线另复用既有 4096 文件、256 MiB 普通文件验证上限。
超限明确失败，不截断后授权应用。CLI/RPC 还必须在应用前预检包含外层包装的最终响应。
公共 `parse_schema_authoring_target`、`parse_schema_authoring_request` 使用严格唯一 JSON
解析，且 core 的直接 Rust 入口执行相同身份、操作和预算验证。以上是有限规模契约，
不声称任意规模性能保证；浏览器仍是授权导入快照，不声称实时磁盘监视。
