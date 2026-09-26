# worldline 语法规范

**版本:** 1.9（1.10 扩展见 §1.1、§7）
**真源:** 本文档 + `worldline/core` 参考实现
**扩展名:** `.wl`

worldline 是面向协作世界构建与叙事创作的文本优先语言。所有被引用的 `.wl` 文件共同定义一个世界,
由事件、人物、时段与资料构成。有序与无序事件可以并存;需要运行叙事时,可通过跃迁和选择表达阅读流程。
通用标签与图片/音频引用见 [catalog.md](catalog.md),均不要求世界事件组成固定游戏流程。
事件归属**故事线(storyline)**；“森林”描述分组，不要求执行图是树，分支汇合与回环均保留。
**漂流(`->>`)**负责跨线移动;**角色 / 状态 / 效果 / 独立锚点**
承载结构化的叙事语义(§10、§11)。

---

## 1. 文件与顶层结构

```text
story       := item*
item        := include | worldDecl | periodDecl | globalDecl | storylineDecl | characterDecl | entityDecl | relationTypeDecl | relationDefDecl | eventDecl | tagDecl | stateDecl | anchorDef | anchorLink | assetDecl | mark | attach | aliasDecl | 注释 | 空行
include     := 'include' STRING
globalDecl  := ('let' | 'const') IDENT '=' expr
storylineDecl := 'storyline' IDENT ('as' STRING)? block(event*)
characterDecl := 'character' IDENT ('as' STRING)? block(property* relation*)?
entityDecl   := 'entity' IDENT 'kind' IDENT ('as' STRING)? block(description? property*)?
relationTypeDecl := 'relation_type' IDENT ('as' STRING)? block(inverse? direction? endpointConstraint*)?
relationDefDecl := 'relation_def' IDENT 'type' IDENT 'from' targetKind IDENT 'to' targetKind IDENT block(description? sourceNote? scopeRef* property*)?
inverse       := 'inverse' STRING
direction     := 'direction' ('directed' | 'undirected')
endpointConstraint := ('from' | 'from_kind' | 'to' | 'to_kind') targetKind
sourceNote    := 'source_note' STRING
scopeRef      := ('scope' | 'scope_ref') targetKind IDENT
worldDecl   := 'world' IDENT ('as' STRING)? block(description? property*)?
periodDecl  := 'period' IDENT ('as' STRING)? ('within' IDENT)?
tagDecl     := 'tag' IDENT ('as' STRING)? block(description? property*)?
stateDecl   := 'state' IDENT 'on' targetKind (qualifiedName | STRING) 'with' tags ('as' STRING)?
tags        := '[]' | IDENT (',' IDENT)*
anchorDef   := 'anchor_def' IDENT 'as' STRING block(description)?
anchorLink  := 'anchor_link' IDENT ('character' | 'event' | 'state' | 'entity' | 'anchor') IDENT
assetDecl   := 'asset' IDENT ('image' | 'audio' | 'file') STRING ('as' STRING)?
mark        := 'mark' targetKind (qualifiedName | STRING) 'with' IDENT (',' IDENT)*
attach      := 'attach' targetKind (qualifiedName | STRING) 'with' IDENT (',' IDENT)*
targetKind  := 'anchor' | 'state' | 'event' | 'scene' | 'character' | 'entity' | 'relation' | 'world' | 'storyline' | 'period' | 'variable' | 'tag' | 'asset' | 'file'
aliasDecl   := 'alias' targetKind TARGET 'as' STRING
description := 'description' STRING
property    := 'property' IDENT '=' (STRING | NUMBER | BOOL | objectRef)
objectRef   := 'ref' '(' STRING ',' STRING ')'
relation    := 'relation' IDENT 'as' STRING
eventDecl   := 'event' qualifiedName eventClauses? block
eventClauses:= ('as' STRING)? ('with' IDENT (',' IDENT)*)? ('at' UINT)? ('during' IDENT)? ('follows' qualifiedName (',' qualifiedName)*)? ('perm' IDENT)? ('after' expr)?
qualifiedName := IDENT ('.' IDENT)*
```

- `include "chapter2.wl"`:引入同目录其他故事文件,路径相对当前文件。
  语义为源文本级合并(扁平命名空间),重复符号按诊断规则报告。
  include 环路 = 错误。目录模式递归载入全部 `.wl`；include 仅控制显式合并顺序。引用必须是工作区内相对路径，见 [workspace.md](workspace.md)。
- `let` 声明全局变量,`const` 声明常量(运行期不可再赋值)。
  所有变量都是故事全局的——状态模型从第一天就全局化、可存档。
- `event` 定义事件。`event market.entry` 在事件 `market` 下定义场景
  `entry`(等价于嵌套声明,见 §3)。

### 1.1 语言 1.10 的实体资料

工程清单的 `language_version` 为 `"1.10"`，或调用方显式使用
`CompileOptions { language_version: V1_10 }` 时，才启用 `entity`。默认
`compile_source`、`compile_sources` 和没有清单的旧工程始终按 1.9 编译；
1.9 中行首的 `entity` 继续按旧文本/顶层结构处理，不会被隐式升级。

```wl
entity lighthouse kind place as "雾港灯塔"
  description "由作者编写的地点资料。"
  property height = 38
  property open = true
```

`entity` 的第一个标识符是稳定 ID，`kind` 后的标识符是可修改的
`entity_type`（例如 `place`、`organization` 或 `culture`），显示名与 ID
分离。实体的 `description` 最多出现一次，`property` 值是字符串、有限数值、布尔
字面量或显式对象引用；属性名在一个实体中不能重复。实体是作者资料，不是事件、
选择、效果或运行状态，也不进入运行指纹。实体 ID 与 `character`、`tag` 等
不同类型的 ID 可以相同；同名资料不会自动合并。

实体可作为 `TargetRef { kind: "entity", id }` 出现在正文链接、目录查询、展示
标记和语言 1.10 的独立锚点关联中。修改显示名或 `entity_type` 保留 ID 和所有展示
坐标；删除前必须通过引用影响查询处理正文、别名、分类、锚点及地图引用。

语言 1.10 属性可使用 `ref("kind", "id")` 声明显式对象引用，例如
`property home = ref("entity", "harbor")`。两项参数必须是非空字符串字面量，不求值；
kind 必须是当前语言版本允许的完整 `TargetRef` 类型。项目清单必须声明
`content.object_refs.v1`，以使不支持该能力的旧客户端按只读处理。缺失目标报告 A214；
只有此显式值建立强引用，普通字符串、模板字段同名或显示文字不自动转成引用。
该属性供作者资料、目录和引用影响使用，不参与事件执行或运行指纹。重命名会更新
显式引用，删除前会报告引用；语言 1.9 不支持此值。

## 2. 块与缩进

- 结构块(event / scene / choice / if)以**缩进**界定归属。
- 一行属于某块,当且仅当其缩进严格大于该块声明行。
- 缩进单位为空格,禁止混用 Tab 与空格(诊断 P002)。
- 子句(`else`、`else if`)缩进与所属 `if` 对齐。

## 3. 事件与场景

```
event arrival                      // 事件
  雾很大,港口的灯一盏盏亮起。       // 文本行
  scene dock                        // 场景(嵌套于事件)
    你走近码头。                    // 场景内的文本行
```

- 场景是事件的子节点,`scene` 只能出现在事件(或场景)块内。
- 全名引用:`事件名.场景名`;事件本身也可直接跃迁(进入其第一个场景,
  无场景则进入事件体文本)。

## 4. 语句

事件/场景/选择/条件块内可出现:

```text
stmt := 文本行 | choiceStmt | divertStmt | ifStmt | letStmt | setStmt | 注释
```

### 4.1 文本行

```
你站在岔路口。                     // 输出一行文本
风停了~                            // 行尾 ~ :与下一行粘接(不换行不加空格)
推开门 #音效:creak #重要            // 行尾 #标签:元数据,不输出
你有 {coins} 枚硬币。               // {表达式} 内插
他说:"\{别怕\}。"                   // \{ \} \# \~ \\ 为转义
```

规则:
- 文本行**不得以关键字开头**(`choice`/`if`/`let`/`set`/`scene`/`->`);
  需要以这些词开头的正文,前置转义 `\choice` 或空格。
- `{}` 内的表达式求值后转为文本:数字去掉多余的 `.0`;
  字符串原样;布尔输出 `true`/`false`。
- 粘接 `~` 只作用于紧随的下一行,链式粘接合法。

### 4.2 选择

```
choice "敲门" if courage >= 5
  门开了一条缝。
  -> hall

choice once "翻墙"
  你摔进了院子里。
```

```text
choiceStmt := 'choice' ('once')? STRING ('if' expr)? block
```

- 标签 `STRING` 是必填的按钮文案,支持 `{}` 内插;
  标签**不会**被自动输出进正文——要回显请写进块内(Ink 的差异点,
  见 §9)。
- 默认选择为**粘性**(sticky):事件重访时仍可再选;
  `once` 修饰后仅可选一次(全故事范围)。
- `if` 条件不满足时该选择不出现。
- 选择块执行完毕且未跃迁 → 流程落到**整个选择组之后**的语句
  (即"隐式汇聚",Ink gather 的结构化替代,见语义规范 §4)。
- 一组选择全部不可用时,流程同样落到组后——这就是 fallback,
  无需专门语法。
- 选择可任意嵌套;内层选择组耗尽后落到内层组之后。

### 4.3 跃迁

```
-> hall                    // 跳到事件 hall
-> market.entry            // 跳到事件 market 的场景 entry
-> entry                   // 同事件内场景可短名引用
-> END                     // 结束故事
->> dream.entry            // 漂流:跨故事线跃迁(语义规范 §8.1)
```

```text
divertStmt := ('->' | '->>') target
target     := qualifiedName | 'END'
```

- `->>` 在普通跃迁语义之上追加故事线切换与漂流锚点记录;
  `->> END` 为语法错误(END 无故事线,漂流无意义)。
- 引用解析顺序:当前事件内场景 → 全局事件 → 报错(unknown-symbol)。

### 4.4 条件

```
if coins >= 10
  你买下了地图。
else if coins >= 5
  你只买得起半张。~
else
  你转身离开。
```

```text
ifStmt := 'if' expr block ('else' 'if' expr block)* ('else' block)?
```

### 4.5 声明与赋值

```
let courage = 0            // 顶层:全局变量声明(也可在块内声明,仍为全局)
set courage = courage + 1  // 赋值,变量必须已 let 声明
```

## 5. 表达式

```text
expr := or
or   := and ('or' and)*
and  := not ('and' not)*
not  := 'not' not | cmp
cmp  := add (('=='|'!='|'<'|'<='|'>'|'>=') add)?
add  := mul (('+'|'-') mul)*
mul  := unary (('*'|'/'|'%') unary)*
unary:= '-' unary | primary
primary := NUMBER | STRING | 'true' | 'false' | IDENT
        | IDENT '(' args ')'          // 函数调用
        | '(' expr ')'
```

- 数值统一为 64 位浮点;输出时 `3.0` 显示为 `3`。
- 字符串字面量 `"..."`,支持 `\n \" \\ \{ \}` 转义;`+` 可拼接字符串。
- 布尔: `true` / `false`;数值与字符串无隐式真值,条件必须为布尔
  (否则诊断 A103)。
- 内建函数:
  - `visits(name)` — 目标事件/场景的访问次数(本回合跃迁前计数)
  - `turns()` — 已完成的选择回合数
  - `seen(name)` — 主角是否曾到访该节点(布尔;`after` 前置条件的谓词)
  - `has(state, tag)` — 状态当前是否包含标签；两个参数为静态标识符或字符串
  - `perm(id)` — 旧权限查询输入，编译时归一为世界“叙事身份”状态的 `has` 查询（见 [states.md](states.md)）
  - `rnd(a, b)` — 含两端的整数随机数(**不可**用于影响存档一致性的
    判定,语义规范 §6)

## 6. 注释

```
// 整行注释
文本行 // 行尾注释(不输出)
/* 块注释
   可跨行 */
```

- 注释剥离是**字符串感知**的:成对双引号内的 `//`、`/*` 不算注释。
- 注释在词法阶段剔除,不进 AST,不参与粘接。

## 7. 关键字与保留字

`event scene choice once if else let const set include and or not true false END storyline character entity world period tag asset mark attach state become anchor_def anchor_link description property relation effect grant revoke meet part anchor`

其中 `entity`、`relation_type` 与 `relation_def` 仅是语言 1.10 的关键字；1.9 的词法和解析入口不为它们保留关键字。

### 1.2 语言 1.10 的独立关系

```wl
relation_type maintains as "维护"
  inverse "由其维护"
  direction directed
  from entity
  to entity

relation_def rel_keepers_lighthouse type maintains from entity keepers to entity lighthouse
  description "守灯会负责灯塔的日常维护。"
  source_note "共同设定记录第3项"
  scope period modern
  property confidence = "作者明确"
```

`relation_type` 的 `display` 是正向显示名；`inverse` 只在从 `to` 端读取时
作为显示投影，不会另存一条反向关系。省略 `direction` 默认为 `directed`；
`undirected` 允许从两端读取同一条边，但仍只保存一个 `relation_def`。
`from`/`to` 约束是可选的单个 `TargetRef.kind`，用于检查关系定义两端，
不是新的对象身份。

`relation_def` 的 ID、类型 ID、from/to 端点均为稳定引用。端点使用完整的
`targetKind IDENT`，允许 `character`、`entity`、`relation`、`tag`、`world`、
`event`、`scene`、`storyline`、`period`、`state`、`anchor`、`asset`、`variable`、
`file` 等已存在对象类型；关系实例可引用另一个已声明或同批声明的关系对象。
文件端点及 scope 写作 `file "相对路径.wl"`，相对当前声明所在源文件解析，
必须命中工程已索引的源码文件；不会自动读取、引入或允许工作区外文件。
Catalog/Project API 使用文件的规范路径身份，结构写入转换为相对路径并加引号。
`description`、`source_note`、`scope` 和字面量
`property` 都是作者资料，不执行事件，也不参与运行指纹；scope 在当前版本
只保存明确的对象引用，不展开时期或推导历史范围。未知类型、端点、scope、
重复 ID 和不满足端点约束分别产生关系诊断，编译产物仍保留可查询的尽力结果。

`END` 仅在 `->` 之后有意义。文本行不得以上述关键字开头(需转义)。

上下文关键字(不保留,仅在对应结构内部有意义):
`as with perm after on enter exit done to at during follows add remove`。其中 `perm`、`grant`、`revoke` 保留为旧源迁移入口，新创作用状态与标签表达身份。

## 8. 示例

```
let courage = 0
const DOOR_LOCKED = true

event start
  你站在旧宅门前,雨越下越大。
  choice "敲门"
    set courage = courage + 1
    门后传来脚步声。
    -> hall
  choice "绕到后院"
    你翻过湿滑的矮墙。
    -> garden

event hall
  女仆举着烛台看着你。
  if courage > 0
    "真有胆量。"她说。
  -> END

event garden
  后院荒草齐腰,只有一间亮灯的小屋。
  choice "进小屋"
    -> shed
  choice "回门前" if visits(garden) >= 1
    -> start

event shed
  小屋里空无一人,桌上摆着一把钥匙。 #关键道具
  你收起钥匙。~
  雨声小了。
  -> END
```

## 9. 与 Ink 的刻意差异

| Ink | worldline | 理由 |
|---|---|---|
| `*`/`+` 前缀区分一次性/粘性选择 | `choice` / `choice once` | 前缀符号不可读,关键词可读 |
| `-` gather 汇聚线 | 块结构隐式汇聚 | gather 是 Ink 最易错特性;块边界即汇聚点 |
| 选择文本默认回显进正文,`[]` 抑制 | 标签永不回显 | 显式优于隐式,正文由块内文本行表达 |
| `->` 后无 fallback 显式语法 | 组耗尽即落穿 | 统一规则,少一套特例 |
| `{knot}` 内插访问计数 | `visits(knot)` | 计数不再与文本内插共享语法 |
| `~ temp` 局部临时变量 | 无局部变量 | 状态全局可存档,分析可静态解析 |
| knot/stitch 两级结构 | event/scene,同语义 | 术语随"世界线"隐喻统一 |
| weave 无嵌套块边界 | 缩进块,任意嵌套 | 结构可静态分析,关系图可精确生成 |

## 10. 故事线与森林结构

完整项目由多条并行**故事线(storyline)**组织事件。单条故事线内的执行路径
可以分支、汇合或回环；跨线执行移动使用:
**漂流跃迁 `->>`**。

```
storyline awake as "清醒世界"
  event start
    你醒来。
    -> hall

storyline dream as "梦境"
  event dream.entry
    这里是梦境。
    -> END
```

- `storyline <名> [as "显示名"]` 声明故事线并开启一个块;
  块内的 event 归属该故事线,嵌套 storyline 为语法错误。
- 不在任何 storyline 块内的 event 归属隐式故事线 `main`
  (可与显式 `storyline main` 块合并;同名故事线的多个块自动合并)。
- 多文件(include)下,各文件声明自己的故事线块,同名合并。
- 每个事件可用 `at` 显式编排；否则按故事线内声明顺序获得**序号**(1 起)。
  序号供布局与正文概览排序，不承担身份，也不生成 `follows`。

### 10.1 事件准入:前置条件与身份状态

```
event shrine after has(identity, blessing) and seen(village) and seen(forest)
  神龛在你面前亮起。
```

- `after <布尔表达式>` 在进入事件前必须成立；可组合 `seen(节点)`、`has(状态, 标签)`、变量与 and / or / not。上例需声明 identity 状态、blessing 标签和所引用事件。
- 旧 `perm P` 输入被转换成世界叙事身份状态的查询；与已有 `after` 用 and 合并，保留优先级。旧权限无需作者预声明对应标签，由迁移补建防冲突 ID；新创作直接声明并引用状态与标签。
- 准入失败在运行时报错，不自动跳过事件(语义规范 §8.2)。

### 10.2 事件头部子句

```
event <名> [as "事件简述"] [with 角色, ...] [at 序号] [during 时段ID] [follows 前置事件, ...] [perm 权限] [after 表达式]
```

- `as "…"`:事件简述(快速理解层,不替代正文);
- `with <角色…>`:人物字段,引用 character 声明(§11.1);
- `at <正整数>`:显式时间线序号,可跨文件排序;省略时沿用故事线内声明序。
  序号相同时按事件 ID 排序;序号只影响展示,不改变入口和执行流。
- 子句顺序固定如上,全部可选。

## 11. 角色、效果与锚点

### 11.1 角色声明

```
character servant as "女仆"
```

角色是正式结构对象:唯一标识(ASCII 标识符)+ 显示名(可选)。
事件的 `with` 子句、`meet`/`part` 语句均引用此处声明的对象;
引用未声明角色为错误 A208。

v1.6 允许角色缩进块内声明静态属性和有向关系:

```wl
character lin as "林舟"
  property age = 28
  property occupation = "调查员"
  property alive = true
  relation mei as "同伴"
```

属性名为 ASCII 标识符。旧 1.9 属性值为字符串、有限数值(含负数)或布尔字面量；
1.10 可额外使用上文定义的显式 `ref("kind", "id")`，并要求清单能力
`content.object_refs.v1`。同一对象内属性名不得重复(A212),不作为运行时变量。
关系目标必须为已声明角色(A208),标签必填;相同目标与标签不得重复(A212)。
反向事件索引由 analysis 统一产生,包含 `with`、`meet`、`part` 和效果块内的角色引用,
按事件声明序去重;表示源码关联,不表示运行时必然在场。

### 11.2 效果:声明式效果块 + 内联变动

**效果块**(节点级声明,只写在事件体顶层):

```
event cellar
  effect on enter
    become identity add witness as "目击者身份"
  effect on done if courage >= 3
    to dream as "沉入梦境"
```

- `effect on (enter | exit | done) [if <表达式>]`;
  `enter` = 进入节点时(准入通过后、正文之前),
  `exit` = 离开事件时（包括 END、重新进入同一事件，详见 [states.md](states.md)），
  `done` = 节点体自然执行完毕时(经跃迁离开不算完成);
- 动作,`as "…"` 为可选叙事文本记录:
  - `become <状态> (with | add | remove) <标签列表或 []>` — 替换、增加或移除状态内容；状态与标签须声明;
  - `grant <权限>` / `revoke <权限>` — 仅旧输入兼容，归一为叙事身份状态的 add/remove;
  - `meet <角色>` / `part <角色>` — 人物变动(登场 / 离场);
  - `to <故事线>` — 主故事线变动(只改归属,不改执行流)。
- 生效时机显式标注,不依赖隐式默认。

**内联变动语句**(执行式,可出现在任意语句位置):

```
choice "下定决心"
  become identity add brave as "你握紧了拳头"
  -> stair
```

`become` 与 `meet / part <对象> [as "记录"]` 立即生效；上例的 identity 与 brave 需先声明。
改变执行流的跨线移动用 `->>`,不用 `to`。

### 11.3 手动演练记录与独立叙事锚点

```
anchor "听闻密室" as "主角得知三楼密室"
```

- `anchor "<名>" [as "说明"]`:执行到该处即产生一条手动锚点记录
  (名称、说明、节点、故事线和回合；不附带完整权限或人物集合快照)。
- 锚点记录的来源:手动 `anchor` 语句、漂流 `->>`、主线变动 `to`、
  权限增删、人物进出。
- 手动记录不改变控制流。独立作者对象使用另一条顶层语法：

```wl
anchor_def turning_point as "听闻密室"
  description "主角开始重新理解这座宅邸。"
anchor_link turning_point character servant
anchor_link turning_point event cellar
anchor_link turning_point state identity
```

关联目标须存在；1.9 的 `anchor_link` 支持 character/event/state/anchor，1.10
另外支持 entity。独立锚点有稳定 ID，可被 mark/attach；通过关联状态与关联事件的
交集读取变化出处，不复制状态，不使用行号作身份。独立锚点不生成演练记录、不改变
指纹或时间顺序。完整目录和编辑 API 见 [catalog.md](catalog.md) §4。

## 12. 多文件世界工程与交付目录

`world.wl` 是推荐的工程总入口,`include` 只能在文件顶层使用,相对各自文件解析。
CLI、agent 的路径参数和编辑器均可传入目录，递归载入全部 `.wl` 并以根目录 `world.wl` 为入口。
目录中的源码合为一个程序;共享文件只载入一次,环路和缺失文件报 A105。
入口为总入口文件的首个事件;总入口仅包含声明和引用时,按文件载入序取首个含事件文件的首事件。

工程可声明唯一世界观;兼容旧单文件故事时允许不声明,第二次 world 声明无论 ID 是否相同均报 A211。

```wl
world fog_harbor as "雾港纪事"
  description "长夜之后,人们沿两条时间线寻找失落的灯塔。"
  property era = "潮汐纪元"
include "characters.wl"
include "events/harbor.wl"
include "events/lighthouse.wl"
```

协作时各作者在独立文件中定义事件,可复用同名 storyline 块,通过全局事件和角色 ID 引用。
事件、角色、变量分别在各自命名空间中唯一;重复定义报 A104,不执行最后写入覆盖。
共享角色只定义一次;不同文件引用同一角色文件不会重复载入。
编辑器中所有文件的未保存内容共同参与编译,切换文件不丢失修改。

导出先验证整个工程,错误阻止导出。交付目录包含全部工作区文件，包含未引用源码、素材、说明和隐藏文件；保持相对路径，拒绝目录外引用和绝对 include。README 与 spec 保留用户原文。
原入口不是 world.wl 时导出补充 world.wl 包装入口。导出使用新目录,不覆盖既有目录。
该目录可直接 `wl check <目录>` / `wl play <目录>`,也可重新打开继续编辑。
目录合并采用普通文件/Git 工作流,导入文件后统一校验 ID;不隐式重命名或覆盖冲突定义。

## 13. 时段与事件的部分顺序

时段可以包含子时段：`period month as "九月" within year`。`within` 引用全局已声明的上级时段，允许跨文件与多层包含，子时段只有一个上级。未知上级、自包含、循环包含报 A219；包含关系不推算日历边界，也不代表两个无父子关系的时段时间重叠。事件仍直接归属一个时段，查看父时段时可递归查看子时段事件，原事件不复制。`follows` 仍限于同一直接时段，包含不会隐式放宽顺序约束。时段包含只影响作者视图，不改变运行指纹。

worldline 可用于描述世界,不要求世界事件构成固定的文字冒险流程。
事件的时间关系与 `->` / `choice` 执行关系分开建模。

```wl
period storm_night as "暴风雨夜 · 21:00—23:00"

storyline harbor
  event investigate as "调查码头" during storm_night
    调查记录。
  event testimony as "取得证词" during storm_night follows investigate
    证词记录。
  event blackout as "全城停电" during storm_night
    停电记录。
```

- `period` 定义全局唯一时段 ID 与显示名称。具体起止时间可写在名称中;
  当前不推断日期、时间单位或跨历法换算。重复 ID 报 A104。
- `during` 将事件归入时段。同一时段默认**无固定先后**,不根据文件顺序、卡片位置或 `at` 推导因果。
- `follows A, B` 声明本事件须晚于 A 与 B;只允许引用同一时段内的事件(可跨文件、跨故事线)。
  未定义时段/前驱、跨时段约束、自依赖或有环约束报 A213。
- 无约束事件与有序事件链可在同一时段并存。analysis 计算约束层级,相同层级只表示可并列,
  并不要求同时发生,也不自动抽取随机排列。
- `at` 始终只用于布局。对时段内卡片拖动不会写入先后约束;先后关系通过明确的 `follows` 操作编辑。
- 此模型不改变入口、选择、跳转或运行时状态;新增时段与先后约束不参与运行存档指纹。
  `after` 仍是执行准入表达式,不能替代时间关系。

## 14. 跨文件正文阅读

`Project::event_drafts()` 从当前工程源缓冲提取事件草稿，按故事线顺序、编排序号与事件 ID 排列。编辑器正文概览据此集中阅读正文与效果、筛选故事线并定位源事件；不求值条件、不执行分支，也不生成第二份可独立编辑的正文真源。阅读排列不写入 `follows`。

## 15. 状态与前后效果（v1.9）

完整语法与生命周期见 [states.md](states.md)。事件名就是稳定事件 ID，不另建一套身份。

## 16. 别名、正文对象链接与人物资料

顶层新增 `alias KIND TARGET as "别名"`；正文和选择文案新增 `[[KIND:TARGET|显示文字]]`。详细语法限制、转义、诊断、静态资料与运行边界见 [catalog.md](catalog.md) §5。`alias` 是声明关键字，普通叙述请避免以该关键字开头。
