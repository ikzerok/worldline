# 真实试玩状态检索（工具 0.31）

只读能力 `runtime.state_inspection.v1`。默认语言1.9、最高显式1.13不变；不改变
运行指纹、Save、ReplayTrace、ReplayCheckpoint或DSL。不允许任意改值、倒放或迁移旧轨迹。

## 1. 真正的观测与运行绑定

`Story::inspect_state(&StateInspectionQuery)` 和 `OwnedStory::inspect_state` 返回同一
runtime投影。查询只借用Story，不执行表达式、推进语句、耗RNG、增加回合/访问，
不改trace、存档、动作捕获或Project。UI/CLI/RPC不得自行解释语义JSON。

每次新建、load/from_checkpoint和restart有独立`run_id`。`compiled_snapshot`绑定此
实例所借用的不可变Program及Analysis，与`fingerprint`一同提供；它不是仅凭运行
指纹推断源码未变化的许可。编辑器另核验完整源码集合、编译选项、内容基线和稿本代次。
查询`stamp`包含run、trace代次及执行修订；继续、成功选择或重新开始使旧分页绑定失效。
这些身份是进程内瞬态工具身份，不进持久存档，不承诺跨进程唯一或可迁移。

### 检查戳的JSON编码

`InspectionStamp`的`run_id`、`compiled_snapshot`、`fingerprint`、`trace_generation`、
`revision`五个身份字段在Rust内部仍为u64；**只有本新能力的检查戳JSON编码统一为规范
十进制字符串**。`stamp`输出与`query.expected_stamp`输入采用完全相同的规则，以便
JavaScript客户端直接JSON.parse/JSON.stringify后原样回送，不经Number转换丢失精度。

规范字符串只能是`"0"`或首位为1–9、其余为ASCII数字的十进制整数，数值范围为
0–18446744073709551615（u64::MAX）。例如`"9007199254740993"`和
`"18446744073709551615"`必须逐位保真。禁止前导零、正负号、空白、空字符串、
指数、小数、非ASCII数字或溢出；即使数值较小，也拒绝JSON number。五字段均必填，
检查戳的未知字段也拒绝。非法编码是查询参数错误，RPC返回-32602；编码合法但与当前
运行不一致仍返回STALE_INSPECTION，不能把格式错误当作合法的过期比较。

客户端应将返回的整个`stamp`作为不透明身份对象回送，不能用`parseInt`、`Number`
或重新计算fingerprint生成预期戳。此规则仅改变新InspectionStamp，不更改现有compile
响应、trace、Save、ReplayCheckpoint、state_view或其他接口的数值字段及持久格式。
0.31发布前的数字检查戳草稿不作为兼容输入保留。

- 首次：本条真实trace的`initial_observation`写入时捕获，通常是首次到达选择或结束；
  不是构造完成/全局初始化时的状态，也不宣称所有首次都为“首次暂停”
- 每次真正新增initial/step observation同时保留typed观察。重复查看或再次continue一个
  已暂停组不增加观测，也不把当前值挪到前次
- 当前已到达并记录选择/结束边界时，上一观测是严格在其之前的上一条已完成观察；
  若当前是首次观察，上一观测不存在
- 成功选择后尚未继续、正在bounded续行、步数/时间预算停止、取消或运行错误时，
  当前是明确标记的实时部分状态，上一观测为最新已完成观察；未有完成观察则不存在。
  部分推进不伪造trace observation。暂停/停止只是宿主状态，不制造观察
- `start_trace_from_here`开启新trace代次、清空旧基线；既有暂停组按原API真实记录
  initial（checkpoint origin），否则等待下一真实完成观察；不冒充入口整条历史
- 从checkpoint恢复只拥有本次实际生成的观测，不补造checkpoint之前的首次/上次。
  未验证导入trace不接受为活跃Story。旧逐步入口可保留标明“原记录只读”的原JSON

## 2. 身份与值

范围为全局变量、当前活跃的各片段调用帧局部/参数、独立状态标签集。全局声明来自
真实Symbols（包括尚未执行的块内声明），局部声明来自真实当前fragment；已结束帧
不冒充当前locals。每次片段调用分配单调调用ID，嵌套与重复同名调用不会合并；
frame身份不同不能比较为同一个局部变量。保存恢复产生新run及新调用身份。

`InspectionKey { group, name, call_id }`，group=`global|local|state`，非局部call_id为空。
局部另提供fragment与调用深度。稳定排序为group、call_id、name。state值使用TagSet，
集合按既有runtime规范的稳定标签顺序，集合比较按成员；不把集合当字符串比较。

单元格状态为`present|uninitialized|not_in_scope|unrecorded|omitted`。present携带原
`Value`（Num/Str/Bool/Tag/TagSet/StateRef）；0、false、空字符串、空集合均为真实值。
未初始化是声明存在但值尚不存在；not_in_scope仅指真实基线存在、该调用当时不存在；
unrecorded表示不存在观察；omitted表示有观测但预算未保留此项/完整值。不得补0/null。

比较结果`unchanged|changed|not_comparable`。两个已知present按Value类型和实际值比较；
已知未初始化与present可比较为changed。缺观察、不同调用身份或省略不能称无变化。
首次与上次各自提供比较；changed-only默认依`previous`，可选`first`。

## 3. 有界查询与明确不完整

query包含`text`（名称、fragment、当前完整真实值及首次/前次已保留显示值的Unicode小写不敏感子串）、
`group`可选、`changed_only`、`compare_to:first|previous`、`offset`、`limit`和可选
`expected_stamp`。默认50、最多100项，text最多256字符；非法额度/过期stamp返回
明确错误，不静默当无命中。结果有精确匹配总数、总项目数、不可比较数（名称/组筛选后、changed-only排除前）、offset、
limit、next_offset及stamp；筛选先于分页。分页不扩大历史保留范围。offset大于或等于匹配总数时返回保留请求offset的空items且next_offset为空，不静默钳位/回第一页；匹配数仍精确。

为避免额外永久全历史，瞬态基线只保留首次、上一、最近真实观察；每份最多4096项、
1MiB已知值，单值序列化最多2048字节；超出显式omitted，不删除原trace。检索当前完整字符串/标签逐字符扫描，辅助内存只随有界查询词增长；历史省略值不能搜到未保留部分，不能暗示历史检索完整。当前值超出
单值预算也只提供有界显示、不伪装成完整typed值。显示截断必须标明；不能把截断
文本相等当成值相等。每页不复制或绘制全量历史。完整页JSON最多1MiB，超出明确返回INSPECTION_OUTPUT_LIMIT；不能删除字段冒充完整。

## 4. 来源、证据与返回

全局变量/state仅提供真实声明来源`DeclarationSource {kind,id,file,line}`；core
`resolve_state_inspection_source`在同CompileResult中核对Symbols/catalog身份及正式
词法声明头，返回statement_header范围。局部无可验证声明来源时明确不可定位。
声明来源不代表最后写入；值变化不代表写入因果，同值写入仍可能真实发生。

检查器联到既有当次缓存条件证据、state_history和已录制路线的状态动作/变量写入
证据；不从diff猜测动作，不额外重放或开启永久last-write捕获。缺写入证据明确标记。
来源请求同时携带运行stamp和项目scope；来源属于本页当前项、同运行同编译快照、
项目无过期/草稿/IME/冲突且磁盘基线确认后才导航。失效则拒绝，不回旧行号。
通过既有作者导航返回，同一试玩会话/位置及检查器筛选保留；返回选择按钮时不触发选择。

宽窗为正文/选择与独立检查区；窄窗提供明确状态检查入口与返回，不将检索置于长选择
或调试列表底部。停止/错误/预算/旧稿/路径未完整均用文字标示，不能仅用颜色。

## 5. 机器接口和验收

RPC `session.inspect`参数为session_id与可选query，返回runtime DTO；CLI交互试玩的
只读state查询使用相同query与结果：暂停等待输入时输入`inspect`或`inspect {"text":"name","changed_only":true}`，返回一行type=inspection；再次等待输入，不重新调用continue。普通数字选择输入保持原规则。查询只对已经存在的真实会话执行，不为读值隐式
选择/续行。参数错误属于协议调用错误；未有基线不是错误，按unrecorded返回。

回归覆盖所有Value、全局/局部同名、局部重入、初始化变化、同值写入、空状态集、
首观察缺前次、构造/预算/错误/取消部分状态、checkpoint/新trace/restart清空、
省略/检索/跨页/过期绑定；前后save/trace/RNG完全相同。编辑器覆盖来源拒绝、窄窗、
键盘、返回位置与选择焦点。native/WASM共享语义，未运行平台不得声称通过。
