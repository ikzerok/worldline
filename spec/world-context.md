# 世界对象焦点上下文（工具 0.17）

这是只读作者投影，不新增语言或第二份正典。`CompileResult::lookup_world_object` 按完整
`TargetRef` 返回单对象；`query_world_context` 在同一编译快照投影一或两跳上下文。
Project 同名包装在当前缓冲编译一次并附 `content_baseline`；不会刷新、保存或修改稿件。
既有 `query_relations` 仍只返回独立正式关系，`ReferenceInfo.kind` 显示文案不变。

## 身份、来源与分类

`WorldContextKind` 分为 `formal_relation`、`legacy_character_relation`、
`property_reference`、`event_participation`、`explicit_body_link`、`text_mention`。
前五类分别复用关系目录及邻接索引、legacy handle、正式 AST 属性、分析人物事件资料、
目录正文链接。`provenance` 是带 kind 的类型枚举：正式关系含 relation_id/relation_type/scope_refs（保留所有限定范围，空数组才无范围），
旧关系含 occurrence，属性含 property，参与含 event，链接含 label，提及含 preview。
不通过中文 `ReferenceInfo.kind` 判断语义；普通字符串与同名显示文字不产生强边。

每个 record 有 id、identity、kind、from_ref/to_ref、direction、role、provenance 和
source。正式关系使用持久 relation ID；其余 occurrence ID 是当前快照内稳定的结构身份，
不可保存为关系 ID。相同源的多次出现和平行关系都保留；反向浏览只展示原端点、不复制边。
旧人物关系依旧只读临时身份，不能伪装为持久关系。节点按完整 TargetRef 识别，缺失端点
保留 `exists:false`，不得猜测替代对象。

source 保留真实 file、1 起 line、可选 1 起 Unicode column、precision（line/column）。
属性 column 指属性声明位置；正文链接含选择外层转义时仅保证行级来源，因此统一按 line
提供正文链接导航。不会将行号或解码字符串列伪造成源码精确字节范围。
自动提及默认关闭，启用时复用 wiki KeywordIndex 原始源码 occurrence，独立标记为线索。
其中可能含显式链接与声明文字命中，不能计入事实、正式关系或参与；每次原始出现保留。

## 同快照、预算与完整性

结果 schema_version=1，含 target/object、snapshot、content_baseline、nodes/records、
total（未知时 null）、returned、complete、truncated、reasons、diagnostics。
snapshot 是排序源码路径/原文、编译选项及实际语义加载序列的内容摘要，不是运行指纹；
任何源字节或编译上下文变化都使旧结果过期。expected_snapshot 不匹配返回 STALE_SNAPSHOT。
Project 的 content_baseline 另标识展示文档等完整作者内容；不得当保存授权。

选项 depth 只能 1/2；direction=outgoing/incoming/both；kinds 空表示全部，text_mention
仍需 include_text_mentions=true。max_nodes 默认/最大250，max_records默认/最大500，
max_candidates默认10000、最大100000；不合法选项返回 INVALID_OPTIONS，不无限提高预算。
候选预算只计当前焦点遍历遇到的唯一相关记录，不因无关目录对象数量拒绝已知 ID。
精确 lookup 不经过广域 CatalogQuery 的候选预算，广域查询旧保护不变。

遍历按类型、来源、端点和 occurrence 的稳定顺序，广度优先最多两跳；方向筛选针对每次
展开的对象，undirected 在任一方向可见。投影不是传递关系推断。输出只含可闭合的节点和边，
达到节点/记录上限标 truncated 与 node_limit/record_limit；可收窄类型、方向、深度或换焦点。
候选预算先于结果投影，耗尽时保留有界已知部分、total=null、reason=candidate_budget；
未耗尽则 total 为所选范围准确记录数，即使显示上限截断。returned 始终等于 records.len()。
有编译错误仍返回可定位资料但 complete=false、reason=invalid_source，不能以空表伪装正常。
未知目标返回 UNKNOWN_TARGET，取消回调返回 CANCELLED，不返回伪造完整结果。

取消 API 每一轮及收集相关记录时检查回调；编译与可选既有 wiki 建索引阶段不是可抢占的。
本预算约束投影相关候选与输出，不宣称限制已有编译/索引构建的内存或墙钟时间。

### 缓冲完整性与未决源码冲突

CompileResult 的 complete 仅描述所选不可变缓冲快照，不证明磁盘仍为最新版，也不授权保存。
Project 包装不偷偷 refresh；只读检查既有交叉修改/恢复冲突，已知未决冲突时保留缓冲资料，
设置 complete=false、reason=source_conflict。冲突本身不属于范围截断，truncated 保持其
原有预算/显示含义，不改变 read_only。无法读取冲突状态时返回 SOURCE_UNAVAILABLE，
不得把不可检查当成无冲突。没有冲突也不承诺查询返回后磁盘不会再次改变。
调用方若已有 CompileResult 与已知冲突，可用结果的 mark_source_conflict 标注同样状态。

## 0.28 可执行使用处扩展

通过 `include_executable:true` 或显式新 kind 协商启用静态调用与全局读写；默认旧
请求的六类及计数不变。新增类型、typed context、来源与索引不完整原因见
[executable-context.md](executable-context.md)。静态写入不是运行时写入证据。
