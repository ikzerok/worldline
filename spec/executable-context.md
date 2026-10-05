# 可执行依赖上下文（工具 0.28）

这是同一编译快照的静态使用处投影，不执行表达式、不展开被调用体、不预测可达路径，
也不代替运行时的实际变量写入证据。语言、存档、运行指纹、Catalog.references、重命名
及删除影响的既有计数不变；不从 ReferenceInfo 的中文展示文案反解语义。

## 显式查询与兼容

能力名为 `authoring.executable_context.v1`。既有 world-context schema1 和一/二跳、
方向、节点/记录/候选预算保持不变。WorldContextOptions 新增 `include_executable`，
默认 false；设为 true，或 kinds 显式包含下述任一新类型，才启用此投影。
旧请求（包括 kinds 空数组）继续返回原有六类；include_text_mentions 的独立开关不变。
启用后 kinds 空数组包含全部原类型及四种可执行类型，非空仍只返回所选类型。
CLI/RPC 与编辑器消费相同 core 投影，编辑器通用对象资料主动启用，不另建图真源。

## 类型化 occurrence 与来源

新增 `rule_call`、`fragment_call`、`global_read`、`global_write`。from_ref 为真实
所属 rule / fragment / event / scene / variable，to_ref 为 rule / fragment / variable。
方向始终为调用者或使用者向被依赖对象。场景使用完整场景身份；片段内部仍属片段。
每个源使用处单独保留，包括同一表达式/同一行多次使用；同一快照重复查询身份稳定。
二跳只沿已存在使用处行走，不递归复制被调函数的使用处，不推导传递事实。

provenance.kind 为 `executable`，包含 typed `context` 与 occurrence 序号。
context 可为 rule_body、call_statement、fragment_argument、global_initializer、local_initializer、
assignment_value、assignment_target、text_interpolation、choice_label、choice_condition、
choice_enable、branch_condition、event_requirement、effect_condition、dynamic_state、
dynamic_tags。role 只是中文阅读标签，机器必须使用 typed kind/context。

全局读写依据正式符号与 AST：rule 参数、fragment 参数和整片段作用域 local 不当成
全局；不把 tag/state/has/seen/visits/perm 的静态身份参数当全局读。声明和 set 的
静态写入与 RHS 读取分开，const/let 初始化也属于静态写入，不声称已实际执行。
普通字符串、注释、链接文字与局部赋值不产生这些全局边。

源文件归属来自 parser 的 statement owner 与 expression slot 侧表，不继承错误的
include 宿主文件。表达式位置来自正式词法来源；写入使用真实声明/语句头位置。
source.column 为 1 起 Unicode 标量列，precision=column 指表达式或语句位置，
不承诺标识符范围。无法确认来源时不伪造文件/行，不返回可点击旧位置。

## 有界索引、失效与完整性

Analysis 为每个编译快照建立一次只读邻接索引，不在 UI 每帧或查询每跳重扫 AST。
索引至多访问 200000 个声明/语句/表达式节点、保存 100000 个 occurrence；达到限额
且尚有内容时，启用此投影的查询标 complete=false、truncated=true、total=null，
reason=`executable_index_budget`。这些限额约束新增投影，不宣称限制既有编译工作。

无法绑定某个可执行来源，或其文件/行/列在当前源码快照不可用时，查询保留已知
可用记录但 complete=false、total=null、reason=`source_unavailable`；这本身不属于
显示截断。编译错误仍按 invalid_source 披露，expected_snapshot 过期仍拒绝。
所有静态关系只描述当前缓冲快照，source_conflict 不被清除。UI 不把错误稿的空结果
解释成“没有使用处”，来源动作须重新核对当前快照、文件与既有草稿守卫。

节点访问额度包含参数绑定与片段整作用域局部名绑定预扫；预扫中止则不把尚未绑定的局部名称
猜成全局变量。调用图没有展开，因此层叠无递归片段不会成倍增加索引使用处。
