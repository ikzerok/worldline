# 真实路线的变量写入证据（工具 0.27）

本契约扩展[路线对照](https://github.com/ikzerok/worldline/blob/main/spec/route-comparison.md)，
只解释同一当前稿中已经执行的全局变量写入。语言默认1.9、最高既有1.13不变，
不新增DSL，不修改求值、随机数、Save、ReplayTrace、ReplayCheckpoint或运行指纹语义。
它不是完整因果图、数据流证明、局部变量调试器或永久历史。

## 1. 实际写入与起点

- 只记录本次路线验证区间内，现有`let`、`const`、`set`真正成功写入全局变量的动作
- 事件、场景、条件/选择嵌套和共享片段中的全局语句遵从同一规则；片段参数和`local`
  不进入本接口。同名局部参数不被当成全局变量，按现行语言的真实写入目标记录
- 可执行`let`首次或再次写入都记录；可执行`const`只在现行语义真正初始化时记录，
  重访时被跳过的const不制造动作。`set`之前必须已有值
- `before: null`表示本次写入前全局变量尚未初始化，不等于空字符串、false或0；
  其他before与after使用既有runtime Value类型，显示和非有限数保护不另立规则
- 同值写入仍是一条动作；顺序按本次真实发生先后，不能按变量名、源码行或值排序
- 只在求值及写入成功后记录已求出的结果。失败语句不产生成功动作；此前已成功的
  动作可保留。未执行条件分支、未选选项和静态可能写入点均不得补入
- 顶层全局初始化发生在故事起点建立，不是本次推进区间的动作。第一次实际写入的
  before可反映初始化值，但不伪造一条初始化历史。检查点恢复的值同样属于起点，
  不把检查点之前的历史回填为本段新写入
- 收集仅用于独立路线对照的验证实例；不改写作者工程或当前活跃试玩会话，不求值
  额外表达式、不额外消耗RNG、不把证据存入普通存档、trace或checkpoint

## 2. 类型与兼容

core公开`VariableWriteOperation::{Let, Const, Set}`，序列化为`let/const/set`。
`EvidenceSourceOwner`加法增加：

```text
VariableWrite { node: String, variable: String, operation: VariableWriteOperation }
```

runtime公开以下只读DTO，并为`RouteSideResult`增加`variable_writes`字段：

```text
VariableWriteRecord {
    sequence: u64,
    operation: VariableWriteOperation,
    variable: String,
    before: Option<Value>,
    after: Value,
    event: Option<String>,
    node: Option<String>,
    turn: u32,
    source: Option<EvidenceSource>
}
VariableWriteEvidence {
    captured: bool,
    records: Vec<VariableWriteRecord>,
    total_writes: u64,
    omitted: bool
}
```

sequence从1开始，按该侧本次实际变量写入计数；记录被省略后仍保留真实总数。
它不与state动作sequence混用，不建立跨类别永久身份。旧对照结果缺少整个新增字段时，
按default得到captured=false、空records、total_writes=0、omitted=false。UI必须显示
“旧结果未提供变量写入证据”，不能把缺字段称为没有发生写入。新验证实例明确开启
捕获后captured=true；尚未建立验证实例即取消或失败时仍为false。schema_version保持1
的加法兼容，旧state_actions、vars、variable_differences及其含义不变。

## 3. 有界捕获、分片与部分结果

状态动作与变量写入共享每侧现有max_evidence_records/max_evidence_bytes上限：默认
及最大合计256条/64KiB。不能为新增类别隐式增加额度。每类保留自身真实动作总数和
omitted；任一类别省略使该侧及总结果omitted=true。零额度仍可报告总数和省略事实。

先按借用值计算现有序列化预算和字段限制，再复制保留记录；巨大字符串不能先复制
到证据再发现超限。单字段与来源沿用现有2048字节保护，整体比较仍受1MiB输出守卫。
收集顺序决定共享预算的保留顺序，不能在报告阶段选择性丢弃早期状态动作来隐藏额度。

跨执行片恢复只续接本次瞬态捕获，不将恢复值重录。取消、步数/时间/输出预算停止、
故事失败与trace分歧只显示实际已执行的已知记录，并与结果状态/完整性一起阅读。
完整但省略的证据不能称为完整写入历史；没有记录不等于未发生，除非captured=true、
本段完整且对应省略标记为false。

## 4. 可信来源

core根据正式AST、parser语句来源侧表和对应let/const/set词法头返回实际来源。身份
包含精确node（fragment使用既有fragment:前缀）、变量ID、写入种类、文件与物理行；
跨文件include、相同行号、嵌套scene和同名变量不得靠当前事件文件或字符串搜索猜测。
歧义、缺来源或任何身份不一致返回不可定位，不回退到变量初始声明。

`resolve_evidence_source(s)`沿用当前快照的完整来源校验与UTF-8语句头范围；新增来源
只可出现在新增变量证据字段中。批量来源总量仍在两侧合计512条动作加首个差异2条
的514上限以内。公开DTO可读不等于跳转授权，编辑器必须先确认来源属于当前对照
结果，再核验当前完整源码、编译选项、内容基线和工作区边界。错误稿、外改、旧快照、
冲突、移源或篡改来源均禁跳；同运行指纹不足以证明当前源位置仍可信。

## 5. 编辑器与机器接口

CLI route-compare与project.compare_routes消费同一runtime结果，不独立重建写入历史。
编辑器可从普通变量终值卡选择变量，展示两条实际路线的写入和语句来源。换A/B只改变
呈现，不交换动作真实身份。回源使用既有作者导航桥；返回恢复同一变量、动作、滚动
和焦点。更新结果后旧动作身份失效，不用相同序号冒认新动作。

A/B、变量名、前后值、写入种类与缺省/截断/未提供信息均需文字表达。长值明确截断，
窄窗口保持可读与来源按钮可达；不只靠颜色区别路线。未应用草稿、输入法组合、只读
和外部冲突继续遵循现有保护，读取证据不自动保存、运行或修改作品。
