# 译文运行展示（工具 0.33）

本页是 `runtime.localization.v1` 的可选展示协议。字符串身份、typed parts、修订、
只读准备及额度唯一真源为 [localization.md](localization.md)。不增加 DSL、翻译表达式、
自动翻译或公开可玩包；未指定 locale 的旧运行入口、运行指纹与 JSON 形状保持原样。

## 1. 准备与身份

调用方从当前 Project 已应用缓冲调用 `prepare_localization_presentation`，显式提交
`LocalizationPresentationRequest { schema_version: 1, target_locale, policy }`。
`Strict` 要求所有正文、say 正文和 choice 标签都有唯一稳定 ID 及匹配修订、合法 token
的译文；`SourceFallback` 仅在作者明确选择时允许逐项回退。准备不求值、不保存、不应用
写作缓冲。未知 locale、无法读取或不受支持的 sidecar、编译错误与预算错误始终拒绝。

core 产生不可任意构造或反序列化的 `LocalizationPresentationSnapshot`。它包含独立的
`presentation_digest`、请求、source baseline、程序指纹及各源单元的 ID、修订、位置、
源/译 parts、状态和译文条目位置。runtime 启动前由 core 校验快照与实际 Program/Analysis
的指纹、单元位置/kind/ID/修订；仅有相同运行指纹不足以证明展示快照相同。

`presentation_digest` 绑定 locale、回退策略、单元 ID/修订、派生状态及实际译文内容，
独立于运行 fingerprint，不把译文加入程序 fingerprint。位置不是稳定字符串身份，
source baseline 另用于回源的当前稿守卫。此内容摘要用于一致性检查，不是安全签名。

`RuntimeLocalizationIdentity { request, presentation_digest }` 是运行持有的展示身份；
源文运行没有该身份。切换语言必须显式重新开始运行；查看已物化的源译对照无需重开、
不求值、不消费随机流。禁止把新 locale 标签贴在旧 source-only 会话上。

## 2. 求值与组装

每次实际执行一条正文、say 或可见 choice 标签时，严格沿原 AST `TextPart` 次序遍历。
每个表达式只按原位置求值一次，缓存其实际显示值；每个链接只解析一次目标。
同一遍同时生成源文 content 与 UTF-8 链接范围，再用 core 提供的 `LocalizationPart`
顺序组装译文。`p0`/`p1` 与 `l0`/`l1` 继续采用原交换协议，runtime 不另造 token 格式。

译文重排 token 只能重排已物化值，不能重排 `rnd` 或规则求值、再次执行表达式、
改变错误次序。源/译链接范围均由 runtime 对各自最终 UTF-8 字符串重新计算，
链接目标仍来自原 AST。译文文字绝不作为 DSL、表达式或链接语法重新解析。

选择 `if`、`once`、`enable` 的执行次序、隐藏/禁用行为、稳定选择 ID、状态动作、
访问计数、call/return、局部值、glue 和暂停点均沿原 runtime。禁用原因、说话者身份
及显示名、direction、状态/动作说明等没有本地化单元的字段仍为源文。

没有合法译文的条目在 SourceFallback 下输出原文字，并逐条保留 MissingId、
MissingTranslation、StaleSource、InvalidTranslation 或 DuplicateId 状态；不得显示为
Translated。空译文与缺译不同：合法空译文显示为空并保留定位/对照元数据。
原 source-only 正文空字符串省略规则保持不变。

## 3. 实际输出与回源

`Output::Text`、`ChoiceView` 和 `ChoicePresentation` 增加可选 `localization`；source-only
运行不输出该字段。原 `content`/`label` 与 `links` 表示实际译文或明确回退后的显示内容。
`LocalizedPresentation` 记录稳定 ID（无 ID 时为空）、真实状态、当前 source revision、
core 源定位、sidecar 路径与条目 JSON Pointer，以及同一次求值的 `source_content` 和
`source_links`。仅定位完整语句或选择头时明确该精度，不伪造表达式子范围。

每次输出的 metadata 来自该运行绑定的不可变快照。UI 只消费这些身份和范围，不重新
扫描注记、解析译文或按文本猜对应项。回源前核对当前 Project/source baseline；快照
已过期、文档不存在或有未应用执行输入时说明原因，保留输入，不自动保存或套用旧位置。

core 的只读 `localization_source_hit(&CompileResult, root, &LocalizationSource, draft)`
复用已有 AST 来源检查，返回工作区内实际文件、完整语句/选择头的 UTF-8 字节范围、
物理行列与原文；相对 `source.file` 不能直接当编辑器文档 key。它只查询传入的
编译快照，不重新编译或执行；`draft` 只标记调用方已验证的稿件身份，不构成写入授权。
普通试玩须先确认来源属于本次已显示输出或当前 choice，且当前稿、运行来源集合、
外部保存基线和工作区边界仍一致，再用统一作者导航恢复精确选区与返回位置。
草稿试演的 `DraftRehearsalSnapshot::localization_source` 继续调用同一 helper，并保留
草稿代次、已显示来源及回源时的完整守卫。定位失败须显示原因，不能静默无动作；
定位、重复点击及返回均不重执行故事或替换实际 locale 会话。

正文、say、choice（含 scene/fragment 及 include 拼接）的来源由正式 parser provenance
确定，不用事件/片段声明文件代替。相对 file 链接按真实语句文件解析，源文与译文共用
同一实际目标；目录引用校验与链接诊断也采用该来源。缺失或同根同位置有歧义时明确
拒绝定位，涉及相对文件链接的运行在初始化之前拒绝，不先消费随机数或执行效果。
此为纠正 include 来源错误，普通同文件 source-only 路径不变，不变更指纹或版本守卫。
非 file 稳定对象链接不依赖目录；即使物理来源有歧义，仍校验目标并保持 source-only
执行。此时不提供可编辑文字位置，反向引用只保留目标事实（空 file、line 0），不漏掉
删除/重构影响。不存在的目标仍报错，诊断明确来源不可用，不猜任一 include 或根文件。

core `RuntimeOutputSourceIndex::new(&Program)` 一次遍历正式 AST，根 owner 贯穿场景、
条件与选择体；`get(&Stmt)` 只接受该不可变 Program 内的实际语句，返回借用的真实文件。
索引不序列化，地址键不是字符串身份、存档身份或编辑凭证；另一份/克隆 AST 必须重建。
来源字符串不逐条复制，渲染时不反复搜索整棵 AST。旧 `runtime_output_source_file` 保留。
Choice EvidenceSource 的 node 确认执行体，file/line 再由同一 parser 来源索引确认；
scene/fragment 的选择可位于其声明之外的 include 文件。单项及批量 resolve 复用同一
校验，每次调用至多建立一次来源索引；错误文件、错误 node 或歧义来源均明确拒绝。
审阅报告的实际选择来源采用同一索引，不因翻译或节点根声明文件改变回源目标。

`ChoiceIdentity.label`、选择覆盖和状态视图中的标签继续保存同次物化的源标签，使
源文/译文会话的语义状态可直接比较。显示标签位于 ChoiceView/ChoicePresentation。
locale 会话的 replay observation 总是包含 `choice_presentation`，包括未使用 enable
的作品，因而真实译文标签和启用状态仍被观察与重放校验。

## 4. 存档、检查点与重放

locale 存档增加可选 `presentation` 身份并在 required_features 中声明
`runtime.localization.v1`；locale trace 和 checkpoint 外壳同样增加可选 `presentation`。
旧 source-only 存档/trace/checkpoint 不增加字段、能力或新 schema，原兼容规则保持。
旧入口遇到含 locale 身份的持久物必须在执行前明确拒绝，不能忽略身份后当源文恢复。

恢复 locale 会话必须传入当前 core 验证的展示快照；请求及 presentation_digest 必须
与记录完全一致。缺失快照、不同 locale/策略、译文更改、修订或回退状态变化均拒绝，
即使程序 fingerprint 相同也不得默默恢复。修订后可显式重新开始体验；不静默迁移
旧译文观察。source-only 持久物不能通过新入口暗中转换为 locale 持久物。

locale 暂停存档另记录选择组开始前的 RNG，恢复时只重建该暂停呈现一次，并核对重建后
RNG 与保存的实际 RNG；不因重新呈现标签多推进随机流。locale checkpoint 使用同一规则。
该额外字段仅在 locale 暂停存档存在，不修改旧 source-only 普通 save 的历史行为。

trace 的 runtime/schema 版本守卫不放宽。checkpoint 仍要求精确程序 fingerprint；
入口 trace 可在新 fingerprint 上受控重放，但必须仍满足同一展示身份并通过全部真实
观察比较。输出、译标签、源对照、链接或状态差异在首处停止。只有 metadata 中已定义
的源码/条目位置字段不参与语义比较；ID、修订、状态、原/译文字和链接仍参与比较。

语言切换不重用旧 trace 的已验证结论。路线比较和审阅记录提供同一快照的显式入口；
两条比较路线必须匹配同一展示身份，不能用一个快照比较不同 locale 或源文/译文混合
记录。结果增加可选 presentation 身份；审阅记录的正文与选择标签为本次实际显示值，并以
可选 localization_status 保留逐条状态，Markdown 明示回退原因。报告不因此增加源文
对照、链接目标或表达式内容。
这些消费者的旧入口遇到 locale trace 明确拒绝，不暗中回退源文。交换导入仍可只读
查看 locale trace。草稿试演的展示准备从隔离稿实际 compiled.sources 与原工程 sidecar
校验修订；源文草稿过期时按显式策略拒绝或标注回退，始终不应用或保存作者缓冲。
机器草稿试演带 presentation 请求时，在隔离编译或克隆 Project 之前先借用检查整个
本地化工作区预算；超限沿既有作品失败域返回 ok:false、零执行和空输出。不带该请求
的 source-only 草稿沿原额度与规则，不自动增加本地化预算限制。

## 5. API 与机器协商

- `Story::new_localized(program, analysis, &snapshot)` 使用既有自动 seed；OwnedStory 同名入口
- `Story::new_with_presentation(program, analysis, seed, &snapshot)`；OwnedStory 同名入口
- `Story::presentation_identity()`；OwnedStory 同名只读入口
- `Story::load_with_presentation(program, analysis, json, &snapshot)`
- `Story::from_checkpoint_with_presentation(program, analysis, checkpoint, &snapshot)`；
  OwnedStory 同名入口
- `ReplayTrace::replay_with_presentation(program, analysis, trace, budget, cancellation, &snapshot)`
- `ReplaySession::new_with_presentation(trace, budget, cancellation, &snapshot)`
- `RouteComparisonSession::new_with_presentation`、`compare_routes_with_presentation` 及
  `PlaythroughReportSession::new_with_presentation`、`generate_playthrough_report_with_presentation`：
  沿原参数，末尾增加 `&snapshot`
- `DraftRehearsal::new_with_presentation(draft_snapshot, seed, &snapshot)`；机器试演请求的
  可选 `presentation` 是 core 准备请求，响应同名字段是实际绑定的运行展示身份

旧 API 保留。CLI 的 locale 选项与 RPC 必须先经过 core 准备，再调用上述真实 runtime；
RPC 通过 `runtime.localization.v1` 协商新增输出字段和持久身份。参数格式错误和作品
准备/运行失败沿既有不同错误域。浏览器与 native 使用同一 core/runtime，不复制协议。

## 6. 必测回归

1. 同 seed 两个 `rnd(1,1000000)` 占位的译文反序；源 p0/p1 与译 p1/p0 对应，暂停前后
   RNG/变量/状态/访问/稳定 choice ID 完全一致；选同 ID 到 END 后再次比较
2. 正文、say、choice、规则参数/local、中文/emoji 链接、重复实际值、重排链接、glue、
   空译文、隐藏/禁用/once、片段暂停/返回；源译对照读取不得消费 RNG
3. 严格准备失败零执行；逐项缺 ID/缺译/过期/无效 token 的明确回退；不存在 locale 拒绝
4. 同快照 save/checkpoint/replay 和合作式分片恢复；错 locale、策略、内容或来源快照拒绝；
   选择标签含随机数时恢复不产生额外消费
5. source-only 原输出/JSON、旧存档/trace、指纹及权限迁移回归保持；译文不能改变选择身份
6. locale 合作式重放在快照校验/恢复后开始当前片时钟，保持累计时间和步数预算，避免
   大稿反复恢复后零进展让出；取消/预算及浏览器真实路径分别验证，编译通过不能替代
   GUI 或 WASM 行为证据
