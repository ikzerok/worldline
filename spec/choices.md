# 语言 1.12：读者可见的锁定选择

本页是显式语言 1.12 的选择扩展。工具版本独立；默认语言仍是 1.9，显式 1.10/1.11 的行为不变。工程清单可声明 `content.choice_presentation.v1`，未知客户端按工作区契约只读保留；独立源码调用须显式传入 1.12。

## 源码与作者说明

```worldline
let key = false
event door
  choice once "进入档案室" if true enable key disabled "还缺银钥匙"
    你打开档案室。
  choice "离开"
    -> END
```

语法为 `choice [once] "标签" [if 显示条件] [enable 可选条件 disabled "禁用说明"]`。两种条件均为布尔表达式，按源码顺序先求显示条件；显示为假或 once 已用时隐藏，不求 enable、不显示说明。enable 与 disabled 必须成对，说明必须为非空的单行静态字符串。说明按纯文字呈现，不解释变量插值、对象链接或执行代码，也不泄露调试条件值。说明中花括号是文字，不触发随机数。标签保留原有插值与链接规则。

同一暂停只真实求值一次 enable；显示、调试证据及投影只读缓存，不额外求值。enable 为 false 的选择仍有稳定身份、标签和作者说明；不能执行它的正文。全组均隐藏或禁用时明确落穿到组后，不创建等待输入的假暂停。已用 once 仍隐藏；disabled 不消耗 once。

## 执行、身份与兼容

`Story::choices()` 和既有 `choose(index)` 保持仅包含可选项的零起索引。新增 `choice_presentations()` 返回源码顺序的全部可见项：`id/label/links/line/offset/enabled/index/disabled_reason`。enabled 为 true 时 index 指向旧 choices 数组；禁用时 index 为 null，disabled_reason 为作者文字。隐藏项不进入投影。

`choose_presentation(index)` 与 `choose_id(id)` 明确校验启用状态。拒选禁用项或失效身份不改变游标、回合、变量、once、随机流、trace 或暂停缓存。旧 choose 索引始终只定位可选项，不能把 presentation 下标当成旧 choices 下标。

没有 enable 的作品不改变指纹、身份、存档形状或随机求值顺序。新条件及说明参与运行指纹；稳定 choice 身份加入可选条件，说明不参与身份。源码行号、文件路径不参与身份。片段中的选择仍使用既有调用路径身份。存档声明 `runtime.choice_presentation.v1`，载入重新建暂停投影；随机条件仍遵守既有“暂停存档重求值”的规则。trace 观察额外保存新能力投影并比较它，拒选不写 trace。

## CLI / RPC 消费协商

- CLI `play --json` 默认只返回旧 choices。显式 `--choice-presentation` 后额外返回 `choice_presentation`，不改变 stdin 旧可选索引。人类菜单默认显示锁定行，无编号，输入序号仍只定位可选项
- RPC `initialize` 声明服务端能力 `runtime.choice_presentation.v1`。客户端在 `session.open` 的 `capabilities` 字符串数组显式申请，响应返回已协商数组；只有已协商 session 的 continue/choose 响应添加 `choice_presentation`
- `session.choose` 既有 index 不变；已协商消费者也可提供 `choice_id` 或 `presentation_index`，三者严格择一。禁用选择是故事错误 `ok:false/run_error`，参数格式错误才是 JSON-RPC error
- 默认消费者得到确定降级：仍可运行，仅看可选项。能力协商不向玩家公开条件解释树、变量或内部事实

## 验收边界

覆盖旧语言拒绝新语法、旧 choices/身份不变、混合隐藏/锁定/可选、全锁落穿、布尔类型检查、一次性、片段局部、暂停续档、同种子重放、失效/禁用选择零推进、说明不耗随机数、默认与新 CLI/RPC 消费者。禁用说明目前不进入本地化交换白名单，交换成功不表示该说明已经翻译。文本包 locale 消费不在本扩展范围；源码说明改变仍按指纹拒绝旧档。

公开读者站点仍是静态白名单阅读包，不执行选择条件，也不是带状态的播放器；本扩展不自动把禁用说明或条件调试信息加入公开阅读包。可执行锁定菜单由runtime、CLI和编辑器试玩消费上述投影。
