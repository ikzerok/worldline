# 当前源码行列定位（工具 0.26）

本功能只定位作者当前看到的精确源码，不增加语言语法、对象身份、书签或折叠。
正文始终是唯一真源。所有请求解析、行列换算、上下文和跳转校验均由 core 提供；
UI 仅显示结果、提交当前完整正文，并消费校验后的零宽 UTF-8 字节范围。

## 坐标与请求

- 行、列均从 1 开始。列按 Unicode 标量值计数，Tab 为 1 列，组合码点分别计列；
  不使用 UTF-8 字节、UTF-16 code unit、字素簇或屏幕视觉宽度。
- LF 与 CRLF 各为一个物理换行。独立 CR、Unicode 行/段分隔符仍是普通标量。
  空文档为 `1:1`，结尾换行之后保留一个空行。
- 每行正文有 N 个标量时允许列 1 到 N+1；N+1 是行尾插入点，在换行符之前。
  文件末尾插入点合法。任何结果均为 UTF-8 边界，不能落在 CRLF 中间。
- 请求为 `行:列` 或 `行`（列默认为 1）。先对整个输入执行 Unicode `trim`，
  内部不允许空白。数字只允许 ASCII 十进制，允许前导零；零、负数、正号、溢出、
  空字段、多余冒号和超出行列范围均返回中文错误，不截断或夹取。
- 编辑器光标使用从 0 开始的整份文档 Unicode 标量索引；CRLF 占两个标量，
  位于两个标量之间的索引被拒绝。字节索引同样从 0 开始。

## Core API

`source_coordinates` 模块公开：

- `SourceCoordinates::new(source: &str) -> Result<SourceCoordinates, String>`
- `SourceCoordinates::position_at_character(&self, source: &str, index: usize)
  -> Result<SourcePosition, String>`
- `SourceCoordinates::position_at_byte(&self, source: &str, index: usize)
  -> Result<SourcePosition, String>`
- `SourceCoordinates::locate(&self, source: &str, request: &str)
  -> Result<SourcePosition, String>`
- `SourceCoordinates::line_count(&self) -> usize`
- `SourcePosition { line, column, byte_offset, character_offset }`，全部为 `usize`
- `SourceLineContext { text, byte_range, start_column, truncated_start, truncated_end }`
- `SourceJumpPreview { path, position, line_count, max_column, context }`，其中
  `line_count` 是全文物理行数，`max_column` 是目标行最后合法列（正文标量数+1）；
  另有不可构造/序列化的私有校验信息
- `Project::preview_source_jump(path: &Path, source: &str, request: &str)
  -> Result<SourceJumpPreview, String>`
- `Project::resolve_source_jump(&self, preview: &SourceJumpPreview, source: &str)
  -> Result<Range<usize>, String>`

坐标索引绑定精确正文，复用时必须传入同一正文；同长度不同内容也拒绝。
公开预览可序列化用于显示，但不支持反序列化为授权跳转凭据。
短行上下文为不含换行符的完整原行；长行只投影目标附近最多 240 个标量，
`byte_range` 为正文中的真实 UTF-8 半开范围，`start_column` 表示片段首列，
两端截断分别明确标识。空行上下文为空，范围为行首零宽范围。

## 边界、过期与只读

Project 接受已跟踪、未删除且位于当前工作区的 `.wl` 源码。归档及未活动源码
可做纯文本定位，不因此进入活动集合。当前未应用编辑正文不必与 Project 缓冲相同。
语法错误、缺失对象、未闭合注释、语言能力尚未启用导致的源码诊断不会阻止纯文本定位；
不调用 AST、编译或语义范围推断。

未知清单格式、语言版本、必需能力及登记诊断仍遵守工作区保护。每次预览和确认均
检查工作区边界、链接/目录联接、未载入新增源码或清单、磁盘保存基线、未解决冲突
及保存事务；失败不刷新、不恢复、不写入。浏览器仅能核验用户已导入的授权快照。
仅有操作系统只读权限的普通源码仍可定位，不要求写权限；这不解除任何写入保护。
源码参数为 Rust `&str`，只能提供有效 UTF-8；磁盘源码或登记文档被外改为无效 UTF-8
时按保存基线冲突拒绝，保留其原始磁盘字节，不替换为损失文本。

私有校验信息绑定规范化路径、工作区根、当前精确正文、Project 内容基线、
刷新代次及编译选项；另保存原始解析目标。确认重新执行全部保护并重建整个公开
预览，任何路径、坐标、索引、上下文或截断标识被更改均拒绝。旧正文、其他工程、
已应用改动、外部刷新和清单变动也拒绝，需重新预览。保存本身不使未变的预览过期。

所有查询只读，不修改正文、Project 保存基线、撤销快照、活动源码集合、语言版本
或运行指纹，不自动保存、运行或迁移工程。

## 有界预算

单份正文最多 2 MiB UTF-8，最多 65,536 个物理行（包含结尾空行）；
原始请求最多 128 字节（包括两端空白），上下文最多 240 个 Unicode 标量。
各上限含边界值，超限明确返回错误。先检查正文/请求长度再扫描或复制。
工作区保护沿用有界库存与保存基线预算，不建立无界全文输出；长单行只截取上下文，
不因单行较长而另行拒绝。上述工具预算不修改语言可接受范围。
