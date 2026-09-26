# ADR-0002：结构化历法与不确定日期范围暂缓

- 状态：已决定（2026-09-26；延期该能力，维持现行时间语义）
- 关联：CAP-08C / [worldline#23](https://github.com/ikzerok/worldline/issues/23)
- 范围：作者记录历法日期、范围与争议年代；不涉及世界时钟或自动历史演化

## 背景

worldline 当前将事件时间分成两种信息：`period` / `during` 把事件归组，`follows` 只记录作者明确给出的先后约束。时段可有 `within` 父子层级，但层级不表示日期包含、重叠或换算；事件不必有时段。`follows` 必须位于同一直接时段，构成无环偏序；同 rank 不表示同时。文件顺序、`at` 布局序号和执行跃迁都不推导时间事实。上述资料不改变运行状态或存档指纹，见 [syntax.md §13](../../spec/syntax.md)、[semantics.md §7](../../spec/semantics.md) 与 [terms.md](../../spec/terms.md)。

CAP-08C 希望评估无日期事件、相对先后、不确定日期范围、多个历法和来源冲突。它们不是同一种值：`约214年`、一个缺少终点的范围、两个史料各自提出的候选日期，分别可能表示模糊精度、未知边界和互相竞争的主张。若将这些都压进单一的起止日期字段，系统就会替作者选定尚未说明的语义。

## 决定

**延期引入结构化日期、历法转换和不确定范围语义。** 本决定不批准新语法、日期字段、历法注册表、日期分析/排序、协议能力或迁移。维持当前 `period` 层级与 `follows` 偏序，继续允许事件没有 `during`。

当前可用做法如下：

- 无日期记录省略 `during`；它仍可被执行图引用或进入运行。
- 只有相对先后的记录继续使用 `follows`。不把 `within` 或布局序号解释成时间先后。
- 作者可在时段显示名中写人类可读的日期说明；它只是文本，不校验日期、不比较、不换算。
- 需要保留争议日期的多方主张时，可在语言 1.10 中用多个作者定义的 `relation_def` 连接事件与各自的候选 `period`，为每条关系写 `source_note`，并把“可能／约略／史料甲解释”等限定写明。这只保存作者的可读关系和来源说明；不生成 `follows`、不裁定真伪、不保证区间计算。语言 1.9 项目不能使用该 1.10 关系语法；定义见 [syntax.md](../../spec/syntax.md) 与 [relations.md](../../spec/relations.md)。

例如，在声明 `language_version: "1.10"` 且启用 `content.relations.v1` 的作品目录中，两条来源主张可以各自关联不同的候选时期，而不复制同一事件为两个运行分支。单独把下段 `.wl` 当作默认 1.9 文件检查会被拒绝；其工程清单 `.world/project.json` 至少包含 `{"schema_version":1,"language_version":"1.10","required_features":["content.relations.v1"]}`。

```wl
period julian_window as "约214—217年〔儒略历；史料甲推读〕"
period local_window as "本地历第18—20年〔史料乙异说〕"

storyline history
  event battle
    战役的叙事内容。
    -> END

relation_type dated_as as "被主张发生于"
  direction directed
  from event
  to period

relation_def battle_date_julian type dated_as from event battle to period julian_window
  source_note "史料甲第4章；作者按儒略历解释"
relation_def battle_date_local type dated_as from event battle to period local_window
  source_note "史料乙卷二；本地历换算规则未存"
```

这两个关系不是经过证明的日期，也不是当前 Timeline 的事件归属或排序输入；展示和筛选消费者必须按普通作者关系处理。
上述片段置于该 1.10 作品目录的 `world.wl` 后，已用 `cargo run --manifest-path .\cli\Cargo.toml -- check <作品目录> --json` 验证为 `ok:true`、无诊断；这只验证现有关系语法，不表示日期解释通过机器验证。

## 考虑过的方案

| 方案 | 覆盖范围 | 成本与边界 |
|---|---|---|
| 继续使用文本时段与 `follows` | 无日期、人工标记的历法/约略范围、明确相对顺序；1.10 关系可保存多个带来源说明的候选主张 | 当前已支持，零迁移；不能校验或比较日期，也不能自动筛出重叠区间 |
| 仅增加公历（RFC 3339）日期 | 可交换精确的公历时间点 | 仍不能表达自定义或非公历历法；不覆盖争议来源与模糊年代。RFC 3339 定义的是格里高利历日期时间格式，不是多历法模型 |
| 采用 ISO 8601-2 扩展 | 可表达不确定/近似、部分未指定日期、扩展区间和日期集合 | 该标准扩展仍以格里高利历为基础，明确排除非格里高利历日期元素；它不能单独定义作者自创历法或来源冲突治理 |
| 允许作者定义并转换任意历法 | 理论上可覆盖虚构日历和跨历法推算 | 需要稳定的历法身份、纪元、闰置规则、规则版本、有效范围、转换依据及歧义策略，还要决定近似/缺界/多主张的数据模型；目前没有用户选定的真实场景或转换规则，范围超过本研究票 |
| 引入完整时间本体与区间关系 | 能把时间参考系、点/区间和相对拓扑分开表达 | OWL-Time 展示了可用的概念分类，但本身不提供 worldline 的源语法、虚构历法算法或史料争议裁定；采用完整本体会扩大协议与编辑器模型 |

## 理由与边界

一手标准说明了选型缺口。ISO 8601-2:2019（官方目录列有 2025 年修订）扩展了格里高利历日期的近似/不确定值、未指定部分、区间与日期集合，但明确排除非格里高利历日期元素；因此它适合作为未来某个明确的公历互操作方案，不能作为所有 worldline 历法的底层真相。[ISO 8601-2 官方摘要](https://www.iso.org/standard/70908.html)。[RFC 3339](https://www.rfc-editor.org/rfc/rfc3339.html) 同样把自身限定为格里高利历时间戳格式；[RFC 5545 §3.3.9](https://www.rfc-editor.org/rfc/rfc5545.html#section-3.3.9) 的 `PERIOD` 则要求精确的开始和结束或开始加正时长，开始必须早于结束。这些适合精确排程，不覆盖开放边界、多个历史主张或任意作者历法。

Unicode CLDR 维护 `gregorian`、`buddhist`、`islamic`、`japanese` 等按类型区分的日历格式与补充数据；不同日历有不同历元/年编号与格式继承。这证明“选择日历类型”已经影响解析和显示，但 CLDR 的地区数据不是用户自定义日历规则的通用注册服务。[Unicode LDML 日期规范](https://www.unicode.org/reports/tr35/tr35-dates.html)。

W3C OWL-Time 将时间参考系分成日历/时钟、坐标和序数系统，并提供点、区间、时间位置和区间拓扑关系；其说明也指出历法运算存在明确与含糊的情况。它是表达概念和关系的本体，不是可直接用于任意历法换算的规则引擎。[W3C OWL-Time](https://www.w3.org/TR/owl-time/)、[OGC Abstract Specification Topic 2 §D](https://docs.ogc.org/as/18-005r4/18-005r4.html)。

所以本轮收益/成本明确：延期保住旧作品语义和零迁移，继续支持相对偏序与无日期事件；代价是作者不能要求机器校验、排序或比较这些日期文本。当前被接受的是“记录时间关系”，不是“推断绝对时间”。本 ADR 不批准世界时钟、角色自主行动、历史自动演化，也不重新打开 [ADR-0001](0001-no-engine-adapter.md) 已排除的游戏引擎适配器。

## 兼容、迁移与失败边界

- 本 ADR 只新增决策文档，不改变语言版本、`.wl` 语法、workspace 能力、CLI/RPC 协议结构、运行行为或指纹；不需要迁移。
- `period` 的显示名不是日期字段；任何能通过字符串语法的“日期”都按普通文字保留。错误的日期、日历名称或换算注释不会触发日期诊断，因为当前没有日期解析器。
- 当前结构错误仍由既有诊断负责：事件引用不存在的时段或 `follows` 指向跨直接时段事件会报 A213；`within` 上级缺失或父链循环会报 A219；重复时段 ID 报 A104。父时段不会放宽 `follows` 的同直接时段限制。
- 对不支持的历法、缺少换算规则、互相冲突的来源以及不完整范围，当前没有运行时拒绝或自动修复行为。若作者在显示文本中记录它们，分析不会确认其真实性或计算范围。
- 若未来批准结构化历法，必须另有范围决策和实施票；旧 `period`/`during`/`follows` 含义保持不变，新字段必须可选。换算只在作者明确指定两边的历法规则与转换依据时进行；规则缺失/歧义时返回“不可比较”，不得静默插补。近似值、缺失边界和相互冲突的来源主张必须有不同、可读的表示，不能都压成一个确定区间。

## 可复现的现有运行/诊断证据

以下命令从 worldline 仓库根目录执行，使用现有 CLI，不运行测试。成功样例同时具有含说明文本的父子时段、一个无日期事件、两个并列事件和一条 `follows`。CLI `timeline --json` 输出中，父子关系仍作为 `periods[].parent`，`claim_a` 与 `claim_b` rank 为 0，`later` rank 为 1；`undated` 出现在执行图节点中，但不在 timeline 事件列表中。

```powershell
$evidence = Join-Path $env:TEMP ('worldline-cap08c-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $evidence | Out-Null
$valid = Join-Path $evidence 'valid.wl'
$validSource = @'
period era as "后燧石纪（起点约214—217，历法换算有争议）"
period season as "雨季" within era
storyline archive
  event start
    choice "无日期记录"
      -> undated
    choice "按甲说"
      -> claim_a
    choice "按乙说"
      -> claim_b
    choice "直接查看后续记录"
      -> later
  event undated
    文献没有给出日期。
    -> END
  event claim_a during season
    甲说的记录。
    -> END
  event claim_b during season
    乙说的记录。
    -> END
  event later during season follows claim_a
    后续记录。
    -> END
'@
[IO.File]::WriteAllText($valid, $validSource, [Text.UTF8Encoding]::new($false))
cargo run --manifest-path .\cli\Cargo.toml -- timeline $valid --json
@('3') | cargo run --manifest-path .\cli\Cargo.toml -- play $valid --json

$invalid = Join-Path $evidence 'invalid.wl'
$invalidSource = @'
period julian as "旧历"
period imperial as "帝国历"
event first during julian follows second
  第一条。
event second during imperial
  第二条。
'@
[IO.File]::WriteAllText($invalid, $invalidSource, [Text.UTF8Encoding]::new($false))
cargo run --manifest-path .\cli\Cargo.toml -- check $invalid --json
```

在 2026-09-26 运行时，`play --json` 选择 0 起下标 `3` 后，直接输出“后续记录。”并结束；存档状态访问了 `start` 与 `later`，没有访问 `claim_a`。这复现了 `follows` 只约束分析时间线、不门控 runtime 的边界。无效样例的 `check --json` 返回 `ok:false`、`A213`、消息“时间约束 `second` → `first` 必须位于同一时段”。这些结果证明的是当前实现边界，不是任何历法兼容或转换能力。

## 重新评审门槛

只有在用户选择一个具体作者场景后，才新开格式/实现决策。需先回答：

1. 目标是史料年代记录、虚构世界历法，还是带时区的现代事件？首批必须支持哪些具体历法/版本？
2. 是否需要机器排序、范围查询、冲突提示或跨历法换算；如果需要，哪些操作允许“不可比较”？
3. “约”“某年到某年”“起点未知”“两种史料各有候选值”分别代表什么？来源和作者解释要保存到什么粒度？
4. 历法更名或规则修订后，旧日期按原规则版本解释、迁移到新版本，还是保持不可比较？

后续验收必须包含无日期、纯相对偏序、不确定/不完整区间、独立来源的冲突主张、同历法与跨历法值，以及显式/缺失/歧义转换规则；并先定下 `worldline/spec` 格式、能力协商与 core 诊断边界，再分开决定 UI 和实现。无论后续选择什么，都不得强制绝对日期、隐式推断历史事实或更改执行时间。
