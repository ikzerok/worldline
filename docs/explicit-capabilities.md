# 显式启用既有语言与资料能力

产品 0.14 与语言版本互相独立。无清单的旧工程和新工程默认仍为 1.9；最高支持 1.13。打开作品不会升级。编辑器的“工程 → 语言与资料能力…”以及任务命令提供同一入口，新建工程后也可选择保留当前设置。

## 怎样选择

- 1.9：人物、事件、分支、变量、标签、彼此独立的 state 与静态时期
- 1.10：entity 可创建地点、组织、物品等资料；relation_type/relation_def 描述独立语义关系。清单显式声明 content.entities.v1、content.relations.v1
- 1.11：纯 rule、可返回 fragment/call、local、tagset、动态状态动作与 say 台词
- 1.12：schema/field/bind 持续资料约束，choice enable/disabled 可见锁定选项；声明 content.choice_presentation.v1
- 1.13：同一顶层时期下的跨后代时段 follows；静态人物强引用需要同时选择 content.object_refs.v1 与 content.character_refs.v1

属性中的实体/关系强引用需要 content.object_refs.v1。本地化文本身份由 content.localization.v1 单独显式启用。界面显示每个能力的最低版本和依赖；升级不会猜测或自动迁入作品资料。

先处理未应用正文或表单，再选版本/能力并“预览全稿兼容影响”。已应用但未保存的源码参与预览，无需先保存磁盘。预览显示具体 required_features、全文诊断、关键字行的解释变化和实际前后运行指纹。旧稿中看似普通正文的新增关键字可能变为语法，必须逐条核对。候选编译失败不能应用；原稿保留。

确认后只提交一次内存事务，可撤销；“保存全部”才写入作品目录。取消完全不改清单、语言、源码或指纹。预览后内容/外部文件变化会拒绝旧计划，需重新预览。未知版本、未知 required_features 或未知格式按原字节只读保留；不会删除未知字段以强行升级。

## 存档和轨迹

升级后的运行指纹可能变化。普通 Story 存档仍检查 required_features 和运行指纹（保留既有权限迁移兼容）；运行检查点/轨迹还受 runtime/schema 版本约束，不能保证旧档可续玩。入口轨迹可按既有规则在候选稿重新严格验证，文字、选择或状态首处差异仍停止；不会忽略输出差异，也不更新旧期望。预览不运行故事、不消耗随机数。

## 一份命题，分别表达事实、相信、知道和携带

不要为每个角色复制同一段“真相”。下面组合沿用已有语言：tag 保存命题身份；不同 state 保存不同用途的集合；物件 entity 是静态资料，携带由独立标签表示。

```worldline
character lin as "林舟"
entity telegram kind item as "手抄电报"
tag signal_real as "信号是真的"
tag telegram_token as "携带电报"
mark entity telegram with telegram_token
state canon on tag signal_real with signal_real as "作者确定的事实"
state lin_knows on character lin with [] as "林舟所知"
state lin_believes on character lin with [] as "林舟相信"
state lin_holds on character lin with [] as "林舟携带"

rule may_verify() -> bool = has(lin_knows, signal_real) and has(lin_holds, telegram_token)

fragment interview()
  choice once "请说明信号来源"
    become lin_knows add signal_real
  choice "暂时不问"
    return
  return

event station
  choice "问清来源，再拿走电报"
    call interview()
    become lin_holds add telegram_token
    -> decision
  choice "只拿电报"
    become lin_holds add telegram_token
    -> decision
  choice "先离开"
    -> END

event decision
  choice "核验并通知" enable may_verify() disabled "尚缺来源知识或电报"
    已核验来源并发出通知。
    -> END
  choice "保留疑问离开"
    -> END
```

此示例需显式 1.12 或 1.13 及相应能力。第一路线再选择“请说明信号来源”后可核验；若选“暂时不问”仍会锁定。第二路线拥有电报却不知道信号真伪；第三路线直接离开。按可选 choices 的零起索引，三条演练为 A=[0,0,0]、B=[1,0]、C=[2]；可见禁用项的 presentation 索引与可选 choices 索引不同。canon、lin_believes 不因拿到电报自动改变。once 在共享片段身份上消费，第二次调用不会重置。标签集合并不自动保证世界逻辑一致，系统也不会自动传播知识或判定谎言。

period/during/follows 描述静态时期与偏序；显示“某年某日”可写资料文本，但不是历法运算或运行调度。正文选择和状态决定本次路径，编排章节顺序不改变运行。三个成功路线是三个实际证据，不能代替所有路径可达性证明。

更多语义边界见 [状态](../spec/states.md)、[1.11](../spec/language-1.11.md)、[锁定选择](../spec/choices.md) 与 [版本事务](../spec/language-versions.md)。
