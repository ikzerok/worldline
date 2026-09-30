# 显式语言 1.11：规则、片段、集合与台词

本规范为产品 0.8.0 的语言真源。默认语言仍为 1.9；显式 1.10 作品不自动升级。只有工程 `language_version: "1.11"` 或 `CompileOptions::v1_11()` 识别以下语法。1.9/1.10 的 call、return、say 正文保持原文。语言版本不等于产品版本。

## 1. 统一类型及纯表达式

类型名为 `num`、`str`、`bool`、`tag`、`tagset`、`state`。后三者分别为静态验证过的标签身份、去重且按 ID 排序的标签集合、状态身份；不接受字符串隐式转换，也不引入数组、对象或动态身份构造。

```wl
rule fare(people: num) -> num = people * 2
rule ready() -> bool = when(count(members(state(evidence))) >= 2, true, false)
rule caption(place: str) -> str = "抵达" + place
```

规则是顶层稳定 ID，参数名在本签名内唯一、不可赋值，返回类型必须匹配。规则每次读取当前全局值、状态与只读谓词，不缓存结果。规则可调用其他规则，但直接/间接递归静态拒绝；规则表达式及传递调用中禁止 rnd。调用实参按左到右在调用方环境求值，允许调用方 rnd（此整个表达式不宣称纯/稳定）。规则不能包含执行语句或调用片段。

`when(condition, yes, no)` 是显式惰性内建：条件必须 bool，两个分支同类型，只执行命中分支；诊断仍检查两分支。旧 and/or 保持急切求值。

集合纯函数：`tag(map)`、`state(evidence)` 的唯一参数为静态 ID（裸标识符或字符串）；`tags(tag(map), tag(log))` 构造集合，`tags()` 为空集；`members(state(evidence))` 读取当前集合；`count(set)`、`contains(set, tag)`、`union(a,b)`、`intersect(a,b)`、`difference(a,b)`。集合显示为按 ID 排序的 `[a, b]`，身份显示为 ID；显示文本不能再解释成身份。旧 has 与静态 become 的裸参数意义不变。

## 2. 可暂停片段与局部绑定

```wl
fragment explain(place: str, selected: tag)
  local fee: num = fare(2)
  say doctor "{place}需要{fee}罐油。" direction "低声"
  choice once "交出证物"
    become state(evidence) remove from tags(selected)
  return

event start
  call explain("医院", tag(map))
  返回医院后文。
```

片段只能顶层声明，签名采用同一类型系统，不产生事件访问、enter/done/exit 或故事线变化。call 求值并验证全部实参后压入一层片段帧，参数按值绑定。片段允许正文、台词、if、choice、局部、全局声明/赋值、状态动作、call、return、跃迁；不允许 scene/effect。直接/间接片段递归静态拒绝，避免无界返回链。规则求值和片段调用最多128层；载入帧栈最多1024层，超过时明确失败。

`local name: type = expr` 仅在片段内，按实际执行路径初始化，绑定不可变。局部名在整个片段内唯一（含不同分支），与参数不可重名；可遮蔽全局。同一 local 再执行时重新求值。读取当前调用尚未初始化的局部明确失败，不回落到同名全局；callee 无法读取 caller 局部。set 只写全局，若名字被本片段参数/局部遮蔽则拒绝。return 仅在片段内，退到最近 call 后文；自然末尾同 return。嵌套 if/choice 不改变这条返回边界。跃迁/END 清空片段返回链并沿既有事件准入/效果规则执行。片段内的场景跃迁必须使用完整场景名，不从调用点猜测短名。

片段内 choice once 的身份由片段定义中的结构位置决定，所有调用共享一次性状态；不同定义中的选择不碰撞。调用位置和片段内当前位置同时可查询。暂停存档完整保存每层定义 ID、调用点、返回位置、参数及已初始化局部，类型和身份严格验证，缺失已必需参数不得补默认值。

## 3. 动态集合状态操作

`become state_expr with from set_expr` 替换；`become state_expr add from set_expr` 增加；`become state_expr remove from set_expr` 移除。state_expr 必须 state，set_expr 必须 tagset。`from` 显式区分旧静态形式，旧 `become evidence remove selected` 仍引用字面标签 selected。

执行时先按当前局部/全局求值并验证两参数和全部标签，再提交一次已有 ChangeKind 状态操作。错误不追加历史、不改变集合；已发生的其他事件效果不回滚。同值与空集操作保留现行历史行为。选择标签缓存不决定后续动作参数。

## 4. 强角色台词

`say character_id "正文" [direction "作者演出备注"] [#wl-localization:ID]`。speaker 必须静态 character 身份，正文使用既有插值/链接规则和稳定本地化身份。direction 是作者备注，不执行、不作为对白输出、不默认进入翻译/读者导出。普通冒号前缀、speaker opaque tags 不变。

运行输出沿用 Text 并在新台词上增加可选 `speaker: {kind:"character", id}`；content 仅为求值后的 spoken text，不自动拼显示名；纯文本消费端因此有确定降级。作者备注不进入运行输出。说话者身份进入语义指纹，direction 与本地化 ID 不进入。

## 5. B1/B2/B3纠偏与兼容

块内 let/const 仍为全局：全源码先收唯一声明后检查引用，不提升初始化。顶层按原顺序启动初始化；块内按路径初始化，未初始化读取/set明确失败。不同声明位置同名 A104，含互斥分支。let 重入重新赋值；const 首次初始化后重入无操作，初始化式不重算。存档保留已初始化与未初始化区别，旧顶层变量缺失仍拒绝。

完整 scene ID 先精确解析，支持跨事件且不受本事件同路径嵌套场景遮蔽；未命中时，按 event.target 精确解析本事件相对路径，最后解析全局事件。单段短名的本事件场景仍优先于同名全局事件，不按前缀选择后代，也不猜嵌套或跨事件叶名；跨事件仍执行 exit/准入/enter。旧短名偶发跳过父场景的无序解析错误被纠正，原指纹保持；旧存档保留已保存位置与实际状态，不重算历史或追补被跳过的正文/效果。

rnd 原数值须有限，先检查 a<=b，再取 ceil(a)、floor(b)，空整数区间拒绝。取整后的边界限定在 IEEE754 安全整数 [-9007199254740991,9007199254740991] 内；跨度使用宽整数防溢出。有效非负范围保留旧抽样及单点 RNG 消费；负数和跨零区间使用有符号偏移，不饱和到0。旧负数错误结果不保证重放等价。

仅实际采用1.11规则、片段、新集合表达式/动作、台词的作品追加新语义指纹域和存档 `required_features:["runtime.language_1_11.v1"]`；旧合法未用新增能力作品指纹和存档形状保持。未知必需能力拒载，不跳过 fingerprint。1.11 纯作者备注变动不改运行指纹。新作品改规则/签名/调用/局部/集合/说话者身份均影响指纹。

## 6. 诊断与下游接口

AST 公开 RuleDecl、FragmentDecl、Parameter、LocalStmt、CallStmt、SayStmt、DynamicChangeStmt；Program.rules/fragments 与 ValueKind/运行 Value 共用六种类型。Stmt 增加 Local/Call/Return/Say/DynamicChange。表达式保持 Expr::Call，新增内建由统一分析/runtime识别，when 求值特别惰性。

A103 类型/签名错误；A104 重复声明；A102 未知变量；A208 未知speaker；新诊断 A230 为规则/片段循环与非法执行上下文，A216 未知标签/状态身份。所有失败提供定义或调用源码位置；规则条件证据旁录定义文件/行与实参求值、未命中分支保持未求值，不为解释重执行。运行规则错误同时标明定义与调用行。目录/引用/重命名/删除、结构编辑、CLI/agent、本地化、书稿/阅读消费同一 AST，不能把 call 降成 jump。

### 作者引用与安全重构

目录新增 rule/fragment TargetRef，调用、强 speaker、tag/state 身份、正文强链接均提供定义到使用位置的反查。
参数和 local 不登记为全局 variable 引用；同名普通正文、字符串、direction 不随身份重命名。
统一 `plan_rename_target` / `apply_rename_plan` 扩展到规则、片段、角色、标签和状态，
预览绑定源码及展示基线，候选完整编译，无法安全完整改写时拒绝并保留原稿。
运行身份重命名可以改变语义指纹；原 entity/relation 重命名保持原指纹保证。
已有 event/choice 结构编辑将未知复杂正文作为完整源码保留，不把 call 当作跳转或展开片段。

### 台词转义层次

say的外层引号使用普通字符串边界，但正文只由插值解析器解码一次：`\{名字\}`输出字面花括号、`\\`输出一个反斜杠、`\[[tag:x|文字]]`输出字面链接拼写而不建立引用。正文中的引号写`\"`。表达式内部的字符串需要保留外层引号层，例如：

```wl
say doctor "她说：\"稍候\"。字面\{花括号\}；{when(true, \"出发\", \"等候\")}"
```

表达式字符串中的反斜杠须经过其自身字符串层，因此如`\"a\\nb\"`表示表达式字符串a、换行、b。作者演出说明仍按普通字符串解码，不当作表达式或链接解析。源码诊断和重构位置映射回原始字符列，不能用解码后的长度覆盖源文。
