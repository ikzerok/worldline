# 诊断的可信原稿来源（工具 0.19）

本合同适用于现有语言 1.9–1.13，不新增语法、诊断事实或运行语义。
诊断仍由 core 产生；[diagnostics.md](diagnostics.md) 的八字段 JSON 不变。
有界摘录、旧报告读取及定位守卫见 [problem-source-context.md](problem-source-context.md) 与 [problems.md](problems.md)。

状态：2026-10-03 17:10 UTC 实施前联合冻结合同。

## 1. 坐标与角色

文件身份使用本次编译的真实来源文件，不能按 basename 或显示名合并。
`Span` 的行/列均 1 起，长度按 Unicode scalar；编译快照投影为原始 UTF-8
字节半开范围。CRLF 的 CR 不算可命中的行内容，EOF 可以是零长位置。
范围合法只是必要条件，不能证明它就是诊断所属语法来源。

内部 `DiagnosticSourceRole` 使用以下 snake_case 值：

| 角色 | 承诺 |
|---|---|
| `target` | 具体语法目标，例如变量名、跃迁目标、正文链接 |
| `expression` | 真正所属的完整表达式或子表达式，包括纯 literal 运算 |
| `statement` | 完整语句或真实所属子句；不是任意固定长度前缀 |
| `declaration` | 完整声明头；不是用声明名字冒充另一字段的错误 |
| `document` | 只有文件身份，不能据占位 Span 生成精确命中 |
| `unavailable` | 没有可供定位的可信来源 |

`Diagnostic::source_role()` 与 `related_source_role(index)` 返回可选角色。
无角色意味着生产者没有提供精确来源证明。主/related 独立，related 角色按
原元组顺序对应；缺项不继承主角色。`with_source_role` 与
`with_related_source_role` 是内部生产者构造接口，不增加旧 Diagnostic JSON 字段。
只有前四类角色和当前原稿范围共同有效时，才能投影为 `span` 精度。

## 2. 正式解析来源

物理行词法边界记录从原稿行首到实际输入片段的 scalar 前缀长度。
表达式/重构片段接口的 `base_col` 始终是 0 起前缀长度；`base_col=0` 表示
独立片段首字符最终是 column=1。物理缩进映射只在明确边界实施一次，不能
对所有 Loc 统一加 indent 或 1。去注释保持 scalar 对齐，不通过字节数猜列。

表达式解析在产生 token 与组合语法节点时同时记录非语义 `ExpressionSource`
树。每个节点有真实原稿两端及按语义 AST 顺序排列的子来源；literal、unary、
binary、call 均有来源。恢复占位节点没有精确承诺。带外层引号的规范化文本
使用 scalar 边界映射，start 与 end 都映回原文；原始转义双字符不能被截去
一端，也不能扩入相邻文本。

表达式来源存入 Program 的非语义侧表。结构键由正式 parser 的文件身份、
所属声明/语句行及表达式用途/序号产生，并由同一结构顺序消费：

- let/const/set/local：初始化或右值
- event：after；effect：条件
- rule：返回表达式；call：按参数顺序的表达式
- text/say：按正文 TextPart 顺序的插值，两个相同表达式仍是两处来源
- choice：标签插值、if、enable 分别独立；不能把条件指到标签文本
- if：每个 if/else-if 分支分别保留物理范围，分支顺序归属于同一 IfStmt
- dynamic become：state 与 tags 表达式分别独立

分析使用当前 AST 的结构 owner/slot 绑定来源树，现有 clone 必须同步按结构
绑定。不能按 Expr 格式化结果、相同 literal、标识符搜索或中文 message 建立
绑定。不存在可证明来源时使用真实所属上下文或明确降级，不能猜测固定宽度。

## 3. 生产者覆盖矩阵

矩阵覆盖实际生产者，不要求所有错误收窄到单 token。相同代码可由多个不同
角色的生产者产生，因此禁止按 code 统一决定角色。精确表达式/目标与完整
上下文都必须来自本次解析；下表的“上下文”不是 document 降级。

| 正式生产入口 | 既有 code | 来源角色及依据 | related |
|---|---|---|---|
| lexer::parse_quoted/decode_escapes、expression::text | P003 | 已知非法转义/括号为 target；整体未闭合为真实字符串/插值上下文 | 无 |
| lexer 缩进检查、parser 块/顶层限制 | P002 | 真实缩进或整条 statement/declaration；缺可靠语法对象时 document | 无 |
| lexer::classify 及 declarations/statements/catalog/choice_tail | P004/P007 | 正式分类产生的完整 statement/declaration；具体合法目标可 target | 无 |
| expression::lex_expr/ExprParser | P006 | 真实 token 或完整 expression；恢复缺失位置明确降级 | 无 |
| parser::language、metadata、text、schemas | P004/SCH001 | 真实 statement/declaration；不拿四字符前缀冒充来源 | 无 |
| parser 空块/EOF | P005 | 所属声明头可证明时 declaration；无来源时 document/unavailable | 无 |
| compiler include 加载/边界 | A105/A109/P002 | include 正式路径/完整声明；根文件加载失败 unavailable/document | 无 |
| analysis::builder::expressions 未知变量 | A102 | Var token，target | 无 |
| expressions 类型/操作数/调用签名/rnd | A103/A230 | 所属完整 ExpressionSource，包括纯 literal 和嵌套参数 | 无 |
| expressions visits/seen 目标 | A101 | 正式调用 expression，不能截去括号或定位同名正文 | 无 |
| builder::variables、collect_decl_symbols、collect_nodes | A104 | 对应声明的真实 target 或 declaration | 前一声明独立来源 |
| builder::walk set | A102/A106 | set 左值 target；类型不匹配使用右值 expression | 无 |
| builder::walk divert | A101/A209 | 真实跃迁目标 target | 无 |
| builder::walk choice 组 | A203/A207 | choice 完整 statement 或真实标签 | 无 |
| builder 角色 with、walk meet/part/to | A208/A210 | 真实所属声明/动作上下文；具体角色可 target | 无 |
| builder::language 调用/参数/局部/上下文/环 | A103/A104/A208/A216/A230 | expression、statement 或 callable declaration；call/say 不指半个关键字 | 重复 callable 的前一声明 |
| builder::flow 可达/结尾/once/执行环 | A201/A202/A205/A206 | 真实事件/选择/尾语句上下文 | 环成员各自真实上下文 |
| builder::flow 未读变量 | A107 | 已证明变量声明 target | 无 |
| analysis_metadata | A208/A211/A212 | relation/property 或对象 declaration | 多世界声明的前一来源 |
| catalog::collect 对象/素材/标签/附件/属性 ref | A104/A109/A214/A215 | 完整 declaration/statement；属性 ref 使用真实 property 表达式上下文 | 重复对象前一声明 |
| navigation::collect_parts / alias | A218 | 正文链接完整 target；alias 为真实 declaration | 无 |
| anchors::analyze | A217 | anchor_def/anchor_link 完整 declaration | 前一定义 |
| states 语法/重复/目标/标签/变更/has | P004/A103/A216 | 正式 state/change declaration/statement 或 has expression | 已有相关来源独立 |
| relations::catalog | A220/A221/A222/A223 | relation_type/relation_def/字段完整 declaration/statement | 前一声明/约束来源 |
| timeline::analyze | A104/A213/A219 | period/event 声明或真实 follows 子句 | 各闭环/受阻来源独立 |
| schemas::parse | SCH001 | schema/field/bind 完整 declaration | 无 |
| schemas::validate 结构/绑定 | SCH001/SCH002/SCH003 | 对应 schema/field/bind declaration | 前一声明或绑定 |
| schemas::validate required | SCH004 | 缺属性的对象 declaration；不能伪造不存在的 property | 真实 field declaration |
| schemas::validate 值/enum/ref/closed | SCH005/SCH006/SCH007/SCH008 | 违规 property 完整 statement/expression | 真实 field declaration |

P001、A108、A204 当前没有活动生产入口；保留原编号定义，不为覆盖而新增诊断。
`schemas::edit` 等只筛选既有 code 的消费者不算新生产者。

## 4. 不变量与验证

只修正来源元数据与旧 Span 值，不改变诊断事实集合、code/severity、接受语法、
类型规则、流判断或世界语义。来源侧表不进入 fingerprint、expression_signature、
choice signature、once 身份及 Story save required_features；不改 runtime_version
守卫。表达式 AST variant 及其语义遍历保持原样。

回归必须覆盖版本 1.9–1.13、0/2/8 缩进、LF/CRLF、中文/emoji/组合字、同词多处、
同 basename 跨文件/include、主/related、同一行多个插值、choice label/if/enable、
引号转义两个端点、长表达式及EOF/恢复。原有 escaped_text 与重复 tide 的正确
行为必须保留。对 slice 作人工确定的golden，不把“范围在界内”当作正确证据。

受影响消费者必须单独验证：refactor/text.rs 的 base0 与 column-1、正式身份
重构、search_replace、evidence_source、timeline 证据与运行解释回源，以及
fingerprint/choice signature/once。位置/投影不得改原稿字节、dirty、撤销或保存基线。
