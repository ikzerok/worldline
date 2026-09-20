# worldline 创作手册

本手册解释从建立工作区到交付作品的完整用法。精确语法以 [语法规范](../spec/syntax.md) 为准；运行细节查 [语义规范](../spec/semantics.md)，机器字段查 [协议](../spec/agent-protocol.md)。语言当前为 v1.9，程序包版本为 0.2.0，这两个版本号不是同一概念。

## 阅读路线

第一次创作按本页顺序阅读，随后运行 `examples/harbor-world`。修复语法看 syntax 与 diagnostics；设计状态看 states；查询人物、分类、附件或锚点看 catalog；接入其他程序看 API 指南与 agent-protocol。完整规范索引位于 [spec/README.md](../spec/README.md)。

## 1. 建立工程

一个作品对应一个目录。根目录建立 UTF-8 的 `world.wl`，子目录名可使用中文。示例结构：

```text
我的世界/
  world.wl
  人物/主角.wl
  事件/第一章.wl
  事件/第二章.wl
  设定/分类.wl
  附件/地图.png
  参考/创作笔记.md
  .agent/skills/
```

目录模式递归分析全部 `.wl`，其余文件作为工程文件保留。`.wl` 不是纯笔记格式，未完成的语言草稿也会参与检查；纯创意笔记用 `.md`。子目录只是整理方式，不形成独立命名空间。`人物/主角.wl` 中定义的 `lin`，在其他子目录仍然直接写 `lin`。

入口文件可以显式 `include "事件/第一章.wl"`，决定先读哪个文件；同一文件重复 include 只加载一次，环路会报错。相对路径以写 include 的文件为基准。子目录内可写 `../人物/主角.wl`，只要最终仍在工作区中。禁止绝对路径、跨工作区引用及目录链接，见 [目录契约](../spec/workspace.md)。

```powershell
wl check "D:/作品/我的世界" --json
wl catalog "D:/作品/我的世界" --json
wl play "D:/作品/我的世界"
```

清单 `.world/project.json` 可将 `language_version` 设为 `"1.10"`，例如：

```json
{"schema_version":1,"language_version":"1.10","entry":"world.wl","required_features":["content.entities.v1"]}
```

没有清单时仍使用 1.9；一次性检查也可显式写 `--language-version=1.10`。
`wl check --json` 将故事编译诊断放在 `diagnostics`，将清单和工作区能力诊断放在
`workspace_diagnostics`；后者非空时 `read_only` 为 `true`，`check` 返回非零并提示
工作区只读。未知语言版本或 `required_features`（例如 `future.entities.v2`）会报告
`WS003`。此时 `wl catalog --json` 仍可读取目录，但不会伪装成可写工程。

给 CLI 传目录获得完整工作区分析；给 CLI 传单个文件则只分析该入口及 include，适合独立示例。编辑器总是把所选目录当作工程。

## 2. 最小故事

```wl
event start as "抵达"
  你在港口醒来。
  choice "寻找灯塔"
    你沿着海岸前进。
    -> lighthouse
  choice "留在码头"
    -> END

event lighthouse as "灯塔"
  灯塔的门开着。
  -> END
```

`event` 后面是稳定 ID，`as` 后面是给人看的名称。ID 使用英文字母、数字和下划线，以字母或下划线开始；场景全名允许点分段。名称、正文和字符串可用中文。重命名显示名称不会自动更改 ID。

块由空格缩进确定，建议统一两个空格，不混用 Tab。选择文案只显示为选项，不会自动回显进正文。希望玩家选后看到同一句话，就在选择块中明确写出。

入口优先是 `world.wl` 中的第一个事件；入口没有事件时按加载次序选择第一个事件。显示排序 `at` 不改变入口。

## 3. 世界、人物与静态资料

```wl
world harbor as "雾港"
  description "一座在长夜中等待灯塔亮起的城市。"
  property era = "潮汐纪元"

character lin as "林舟"
  property age = 28
  property motivation = "寻找失踪的同伴"
  property alive = true
  relation mei as "同伴"

character mei as "梅"
alias character lin as "阿舟"
```

1.10 工程可以声明通用实体。实体的 `id` 是稳定引用，`kind` 是可修改的资料
分类，显示名、description 和静态 property 不进入故事运行状态：

```wl
entity lighthouse kind place as "雾港灯塔"
  description "由作者编写的地点资料。"
  property height = 38
  property lit = true
```

实体工程的清单还应声明 `content.entities.v1`；机器脚本可用目录查询和结构编辑命令：

```powershell
wl catalog "D:/作品/我的世界" --kind entity --json
wl entity create "D:/作品/我的世界" --id lighthouse --kind place --display "雾港灯塔" --json
wl entity update "D:/作品/我的世界" --id lighthouse --display "新灯塔" --baseline <上次结果中的值> --json
wl entity delete "D:/作品/我的世界" --id lighthouse --baseline <上次结果中的值> --json
```

编辑命令要求清单明确启用 1.10，并在写盘前验证当前源码和引用；过期
`--baseline` 返回 `ok:false`，不会覆盖外部修改。实体只允许在顶层声明，不能
作为事件正文中的可执行语句。

世界可省略，但一个工程最多声明一次。人物、事件、变量等分别按各自类型的命名空间检查重复。属性是字符串、有限数值或布尔字面量，不执行表达式；同一对象的属性名不能重复。人物关系有方向，`lin relation mei` 不自动写出反向关系。相同目标和关系名称的重复定义会报错。

静态属性适合外貌、习惯、经历、动机、说话方式。随叙事变化的事实用状态，数值计算用变量。写下 `property alive = true` 不会建立可执行的“存活状态”。

人物反查包含事件头部 `with`、正文 `meet/part` 和效果中的引用。这些是源码关联，不证明人物在每条分支都出现。别名用于查找，同名人物不会合并，引用仍使用稳定 ID。

## 4. 故事线、场景与两种顺序

```wl
period night as "暴风雨夜 · 21:00—23:00"
storyline harbor as "港口"
  event arrival with lin at 1 during night
    抵达记录。
  event inquiry with mei at 2 during night follows arrival
    调查记录。
storyline city as "城区"
  event blackout during night
    全城停电。
```

`storyline` 是事件分组，可跨文件用同名块续写。没有明确分组的事件归 `main`。不能把 storyline 嵌在 storyline 内。事件中的 `scene dock` 建立子场景，完整身份如 `arrival.dock`；同事件内跳转可用短名，跨事件用全名。

`at` 只控制展示和正文概览排序。`during` 表示所属时段，`follows` 表示同一时段内必须晚于哪些事件；没有约束的事件默认无序。前驱必须存在且在同一时段，时间图不能有环，不根据卡片位置推断先后，不计算历法。

`->`、`choice` 控制实际演练路径，允许汇合和回环。`after` 检查能否进入目标；它既不是时间约束，也不会自动连出执行边。编辑器的关系图与时间线使用两种不同分析数据。

## 5. 正文、转义与链接

```wl
event note
  第一行。
  连在一起~
  的第二部分。 #提示
  你认识 [[character:lin|阿舟]]。
  字面量：\{变量\} \#标签 \~波浪线 \\
  -> END
```

普通正文逐行输出。行尾 `~` 表示与下一段文本无换行、无额外空格地连接；`#标签` 是输出元数据，不属于可见正文。正文标签兼容未预声明的隐式标签；顶层 `mark` 引用的标签必须预声明。

`{表达式}` 在执行时内插，选择文案同样支持。双引号字符串的换行可用 `\n`，引号用 `\"`，反斜杠用 `\\`；正文转义与字符串转义所在层不同，在选择字符串中写字面反斜杠需先经过字符串层。准确转义集合见语法规范。

`[[类型:ID|显示文字]]` 建立正文对象链接，播放输出显示文字，不跳转、不改变状态。选择文案也可含链接；属性和 description 不自动解析对象链接。显示文字有保留符号限制，详见 catalog §5；显示名改动不会自动改写链接文字。`\[\[` 可输出普通双左方括号。

`//` 为行注释，`/* ... */` 为跨行注释。字符串中的注释符保持原文。注释不参与运行，但编辑器结构修改尽量保留作者注释。

## 6. 选择、分支与汇聚

连续的 choice 构成一组。默认选择可重选，`choice once "文案"` 在整个故事中只选一次；`if 表达式` 控制是否显示。一次选择增加一次 `turns()`。

选择块没有显式跃迁时，执行完毕落到整个选择组后面。不会执行同组未选择的其他选项。所有选项均被过滤时也落到组后；可在那里写兜底内容。嵌套选择按同样规则返回上层。

```wl
let courage = 0
event decision
  choice "鼓起勇气"
    set courage = courage + 1
  choice once "等待"
    风声渐强。
  无论选择什么，都来到这里。
  if courage > 0
    你推开了门。
  else
    你仍在犹豫。
  -> END
```

`-> END` 结束故事；事件自然执行到末尾也会结束，非时段事件可能产生缺少显式结束的提醒。跳到不存在的对象是编译错误。`->>` 跨故事线漂流，会切换当前故事线并记录漂流；不能写 `->> END`。普通跨线跳转与当前故事线字段的差异见语义规范，创作跨线移动应明确使用漂流。

## 7. 变量、表达式与内建函数

`let` 声明全局变量，`const` 声明不可重新赋值的常量，`set` 给已有变量赋值。块内 let 仍是故事全局，不是局部变量。数值使用 f64，不把数值或字符串隐式当作真假。

优先级从低到高为 `or`、`and`、`not`、比较、加减、乘除取余、一元负号、字面量/函数/括号。比较支持 `== != < <= > >=`，字符串可用 `+` 拼接。条件必须为布尔；变量未声明、常量写入和类型错误分别诊断。

| 函数 | 参数与结果 | 创作用途 |
|---|---|---|
| `visits(node)` | 节点访问次数 | 重访文字、回环限制 |
| `seen(node)` | 是否已经访问 | 事件准入前置 |
| `turns()` | 已完成选择回合数 | 回合条件 |
| `has(state, tag)` | 状态是否包含标签 | 身份、心境或事实条件 |
| `rnd(a, b)` | 含两端的随机整数 | 明确的随机内容 |
| `perm(id)` | 旧权限查询，自动迁移 | 兼容旧作品 |

节点和状态参数用静态标识符或字符串，不把它们当可调用的用户函数。语言没有用户函数、局部调用栈、并发叙事线程或外部脚本执行。

进入节点时访问计数即增加，所以节点自身正文的 visits 至少为 1。随机状态随档保存，但暂停选择组读档后会重新求值，含 rnd 的选项可能变化。稳定分支可先把随机结果存入变量再判断。

## 8. 标签与状态

```wl
tag calm as "平静"
tag alert as "警觉"
tag identity_group as "身份分类"
character lin as "林舟"
state mood on character lin with calm as "心境"
mark state mood with identity_group
event warning with lin
  become mood add alert as "听到警报"
  if has(mood, alert)
    他握紧了手中的信。
  become mood remove calm
  -> END
```

state 是有稳定 ID 的完整对象，绑定世界、人物或其他可引用对象，内容是去重后的标签集合。`with` 替换全部内容，`add` 增加而保留其他标签，`remove` 删除指定标签；`[]` 表示空集合。重复添加、删除不存在的标签仍可记录明确动作。

状态的内容标签和 `mark state ...` 分类标签不同。修改分类不改变运行初值；修改状态初值会影响运行。分析目录列出全部源码中的条件变化，运行 `state_history` 只记录真正执行的变化，并保存 before/after 与动作来源。不能把不同分支中的动作拼成唯一“当前事实”。

旧 `grant/revoke/perm` 会归一到世界“叙事身份”状态。编辑器在缓冲中迁移，保存前不改磁盘；新作品直接用标签与状态。迁移的防冲突 ID、兼容注释和旧档限制见 [states.md](../spec/states.md)。

## 9. 准入与效果

`event gate after seen(start) and has(identity, ticket)` 表示进入 gate 前条件必须为真。准入失败是运行错误，不自动跳过；上游选择或跃迁可用相同条件守护。同事件场景切换不重复准入，跨事件进入场景仍检查目标事件。

效果块放在事件体顶层：

```wl
effect on enter if has(mood, calm)
  become mood add alert as "开始警觉"
effect on done
  become mood remove calm
effect on exit
  part lin as "离场"
```

这段是事件体片段，使用前需声明 mood、calm、alert 与 lin。`enter` 在准入通过后、正文前触发；`done` 仅自然完成触发；`exit` 在离开事件时触发，包括 END 和同事件重入。自然完成顺序为 done 后 exit；显式跃迁不触发 done。同事件内场景跳转不触发 exit。

效果条件假时整块跳过。动作有状态操作、人物 `meet/part`、当前故事线 `to`，旧权限动作仅作兼容。`to` 只改当前归属，不改执行位置；改变路径用 `->` 或 `->>`。源事件 exit 在目标准入之前执行，目标失败不回滚已经发生的效果。

## 10. 分类、附件与资料导航

`tag` 可带 description 和 property；`mark KIND ID with 标签列表` 分类完整对象，`attach KIND ID with 素材列表` 关联附件。标签也能被标签分类，递归查询使用访问集合去重，即使有分类环也能终止。递归命中不自动写回祖先标签。

```wl
tag place as "地点"
tag harbor_place as "雾港码头"
mark tag harbor_place with place
asset map image "附件/地图.png" as "地图"
mark asset map with harbor_place
```

这段需要工作区中确有相应地图。支持 image/audio/file；缺文件可继续编辑并报告 A215，但完整导出会失败。`file` 对象目标用于工程源码，不等同于任意附件；任意格式资料通过 asset file 声明。

```powershell
wl catalog ./我的世界 --tag place --recursive --json
wl catalog ./我的世界 --tag place --recursive --kind character --json
```

资料阅读聚合属性、关系、别名、正文链接、状态、锚点和附件，全部来自源码快照。AI 或作者应按文件与行号回看上下文，不能把搜索命中本身当成无条件成立的设定。

## 11. 独立锚点与演练记录

`anchor_def turning as "转折"` 定义作者意义对象，可写 description。`anchor_link turning character lin`、`anchor_link turning event warning`、`anchor_link turning state mood` 关联对象；还可关联另一个 anchor。锚点关联状态与事件后，只显示二者交集中的状态变化出处，不递归复制其他锚点的内容。

`anchor "发现真相" as "他重新理解了那封信"` 则是正文执行到此才写入的手动演练记录，没有独立对象 ID。漂流、人物变化等也能产生运行记录。两个概念不自动转换，独立锚点不触发剧情、不建立时间先后关系。

## 12. 保存、协作与交付

编辑器保存全部 `.wl` 缓冲，写前检查外部修改。未修改文件由桌面约每秒自动刷新；本地与磁盘同时修改同一文件会保留缓冲并提示。协作可按子目录分工，合并后对整个目录运行 check，再检查 ID、关系、状态和时段。

导出要求无编译错误、至少一个可运行事件且声明附件可读。目标为工作区外的新目录；保留全部工作区文件与相对目录，包括 `.agent`、未引用资料和原 README。源码采用当前缓冲，旧权限在导出副本归一。另存允许未完成源码，仍遵守路径和新目录限制。

运行存档保存演练状态，不是作者工程备份。工程备份应复制完整目录或使用版本控制。改稿会改变部分运行指纹，旧存档可能拒绝载入；不要手工删除指纹验证。时段布局、分类、附件与独立锚点等纯作者元数据一般不影响运行指纹，正文、状态与其他字段的精确边界见 semantics §7。

## 13. 检查失败时

先处理 error，再审阅 warning 与 hint。诊断含英文 code、中文 message、真实源文件、从 1 开始的行列及关联位置。A104 看重复定义，A105 看缺失 include/环路，A109 看目录越界，A213 看时间约束，A214 看资料引用，A215 看附件，A216 看状态，A217 看锚点，A218 看别名与正文链接。完整编号见 [diagnostics.md](../spec/diagnostics.md)。

程序能验证结构与引用，不能验证自然语言真假、人物动机是否合理或情节是否矛盾。交付前分别审阅结构、可选演练路径和叙事一致性，并标明未覆盖的分支。


## 14. 时段包含与分支表单

`period year as "全年"` 与 `period september as "九月" within year` 建立父子关系；继续用 within 可建立月份包含日期等层级。每个时段只有一个直接上级，未知上级和包含环为 A219。事件保持原有直接归属，父时段视图容纳子时段而不复制事件。follows 仍约束同一直接时段，不把父子关系当作时间先后或历法计算。

编辑器时间线的“＋ 子时段”默认设置上级，双击时段标题可修改名称和上级。事件详情顶部有分支决策卡片，可以添加、删除和编辑选择文案、显示条件、once、选后正文与末尾去向。目标为其他故事线时默认使用漂流；结束故事使用 END。继续选择组后内容时，应在分支内提供有效正文或动作。条件仍使用语言表达式，复杂嵌套正文可继续在高级编辑中修改。

卡片修改先进入事件草稿，点击“应用更改”进行全工程验证并更新视图；Ctrl+S 保存磁盘。嵌套选择也单独列出，修改外层正文可能改变内层结构。卡片的去向只编辑该分支末尾的直接出口，不覆盖正文内部的条件跃迁。删除分支包括其正文与嵌套内容，编译失败不会提交工程，可撤销已提交操作。
