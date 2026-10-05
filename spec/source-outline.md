# 当前源码结构与可信定位（工具 0.25）

本功能为当前单个活动 `.wl` 文件的只读结构投影，不增加 DSL、运行状态或持久文档。
源码仍是唯一正文。当前对象仅指光标的语法归属，不代表执行位置、状态或选中的目录对象。

## 声明覆盖与身份

明确支持 world、storyline、event、scene、character、entity、relation_type、
relation_def（投影 kind 为 relation）、period、tag、anchor_def（anchor）、state、asset、
顶层或 storyline 内 let/const、rule、fragment、schema。只列正式 parser 接受的显式
声明；不列 include、alias、mark、attach、anchor_link、bind、field、property、运行
语句、执行块内变量、隐式 main/迁移生成对象。显示名使用声明自身的 as/summary，缺失
时回退 ID；不把别名或目录合并结果替代源码声明。entity 的类型另给 entity_type。

源顺序是唯一默认顺序。storyline 包含其词法块内 event 与全局变量；event
包含 scene，scene 包含嵌套 scene；if/choice 不增加大纲行，但其中的 scene 保留
最近显式声明父项。period 的 parent 是资料关系，不是源码层级。scene 的 id 是
完整 event 与各级 scene 点分路径。相同 ID、相同显示名与重复 storyline
块均保留各自行、范围与 occurrence（文件内从零开始的源顺序序号）；parent 是父项
occurrence，不以名称合并。语义重复错误不会抹掉已确认的语法结构。fragment 内不允许 scene，错误片段中的 scene 不生成独立条目；fragment 自身仍只代表其语法声明，不宣称语义有效。显式无名 storyline 以 main 表示，仅隐式 main 不列出。

## 正式来源与范围

投影在当前 Project 语言/能力选项下，只对传入的精确当前编辑正文调用正式 lexer 与
parser。允许正文是 Project 已应用但未保存稿，或当前编辑器尚未应用的精确源码；
不修改 Project，不缓存另一份全文，不读取 include 正文，也不要求其它文件编译成功。
顶层 include 由既有词法识别并从单文件结构解析中移除；非法缩进 include 仍报错。
默认 1.9，显式 1.10–1.13 沿用既有门禁，未知版本/必需能力不可用。

header 为声明头从首个语法字符到最后语法字符的 UTF-8 字节半开范围，不含缩进、
尾注释、CR/LF。body 从 header.start 到正式 parser 消费的最后语句末端；绝不延伸
到下一声明或吸收尾随空白/注释。字符列到字节仅通过原始文本边界转换，CRLF、中文、
emoji 与注释中的多字节字符均保持原始字节。范围来自 parser 消费边界与正式词法
来源，不在 UI 用正则、行首关键字或目录单键反推。

current_item(byte_offset) 只接受正式语句字符范围内的位置，返回最深包含声明。
纯空白/注释行、独立关联语句、尾随空白、EOF 等返回 None；它不选择“最近对象”。
内部块注释位置也返回 None。字节偏移落在 UTF-8 字符内部时返回 None。

## 预算与失败

硬预算为单文件 512 KiB、16384 个物理行、每行 16384 字节、正式词法行 16384、
最多 4096 个声明、语法缩进层级 64。限制含边界本身；超限不返回截断可跳列表。
序列化完整公开投影（serde JSON UTF-8）最多 2 MiB，以计数 writer 精确检查，不分配第二份 JSON。
先检查文本/行预算，再正式词法；递归 parser 前保守限制词法缩进与去注释后的分隔符
嵌套深度 64、每行分隔符/算符 256 个、not 单词 64 个。该资源守卫包含字符串/正文
字面量中的符号，不是额外语法解析；超限只关闭结构导航，不改语言合法性。所有预算失败明确 BudgetExceeded。
空文件与只有注释/无声明的合法文件是 Ready 且 entries 为空。

任何当前文件词法/语法错误、未闭合块注释、不能证明范围时，整份 SyntaxInvalid /
Unavailable、entries 为空，不沿用上次成功列表。重复 ID 等分析错误不影响来源真实
性，本投影不自称全工程语义检查。非活动文件、墓碑、工作区能力错误、外部冲突或
未完成保存事务均明确不可用。浏览器检查限于授权导入快照，不宣称实时宿主磁盘同步。

## Rust 契约与过期

公开模块 `worldline_core::source_outline`：SourceOutline、SourceOutlineEntry、
SourceOutlineStatus。Project::source_outline(path: &Path, source: &str) -> SourceOutline；
SourceOutline 字段 path: PathBuf、status、message: Option<String>、entries: Vec<...>。
Entry 字段 occurrence: usize、parent: Option<usize>、depth: usize、kind: String、id:
String、display: String、entity_type: Option<String>、line: u32、header/body: Range<usize>。
SourceOutline::current_item(byte_offset: usize) -> Option<&SourceOutlineEntry>；
SourceOutline::matches_source(source: &str) -> bool 仅检查正文版本，不代替 Project 守卫。
状态为 Ready、SyntaxInvalid、BudgetExceeded、Inactive、Unavailable。

Project::source_outline_range(outline: &SourceOutline, source: &str, occurrence: usize)
-> Result<Range<usize>, String> 在跳转前重新检查路径、活动身份、Project 内容基线、
外部刷新代次、语言/能力、磁盘保存基线与当前精确正文，再重建/比对结构。返回当前
header；过期、被改动结果或范围不确定时拒绝并返回中文原因。调用者每次编辑、
撤销/重做、外部刷新、清单变化、IME 组合都应失效 UI 缓存，组合期间不导航。
查询 token 只为会话内意外陈旧检测，不是安全签名。不得把新正文显示配旧偏移。

UI 按文件身份与真实正文/工程代次缓存一次投影，不每帧全工程解析。返回作者位置
同样绑定原文件和正文版本，只有一致时恢复字符选区和滚动。本轮不新增 CLI/RPC；
以上是共享 core Rust 合同，不建立第二套机器目录或正文存储。
