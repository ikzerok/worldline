# ADR-0003：延期 DOCX/EPUB 产品接入，优先评审 EPUB 单向导出

- 日期：2026-09-26。
- 状态：本轮技术范围决定为**延期产品集成**；保留可复现研究原型。未来 EPUB 子集为候选，尚未批准发布。
- 关联：CAP-07E / [worldline#21](https://github.com/ikzerok/worldline/issues/21)。
- 变更范围：仅本 ADR 与 `docs/adr/cap07e-prototype/`；不修改语言、默认工程加载、core/runtime、CLI/RPC 或产品依赖。

## 决定与理由

本轮不把 DOCX/EPUB 导入、导出或往返加入产品。后续如继续，先评审从明确书稿顺序生成 **EPUB 单向静态阅读副本**：Unicode 正文、章节标题、内部链接和工作区内 PNG/JPEG 图片。其前提是书稿/发布契约已通过独立验收，内容公开边界由 core 产生。它不反向成为作品真源，不执行条件，不导出游戏引擎适配器。

DOCX 导入/往返暂缓；DOCX 纯文本或有限样式导出也不在本轮采用。EPUB 导入暂缓。拒绝“通用格式无损”“保留所有 Word 样式和审阅记录”“ZIP 可打开即合规”等承诺。拒绝宏、嵌入可执行对象、脚本、DRM/加密包、自动下载外部素材和隐式跨工作区读取。

理由是实际原型只证明有限数据可生成/保留，尚不满足可接受的保真和拒绝边界：DOCX 复杂样例重写后 XML 无效且多个部件丢失；畸形 XML 被库接受。EPUB 导出包通过本原型的资源与字节检查，但未做格式一致性验证与阅读器呈现。WASM 需要显式随机源配置，编译通过也不等于浏览器可用。延期不阻止 Markdown 导入与静态阅读包按各自工单推进。

## 官方资料与依赖候选

访问日均为 2026-09-26。版本号是本次锁定/读取的版本，不声称是唯一或最佳方案。许可证列仅记录上游声明，不构成完整分发审查。

| 候选 | 证据、许可证与方向 | 原型/支持判断 |
|---|---|---|
| docx-rs 0.4.22 | [上游仓库](https://github.com/bokuweb/docx-rs)、[read_docx API](https://docs.rs/docx-rs/latest/docx_rs/fn.read_docx.html)、[LICENSE](https://github.com/bokuweb/docx-rs/blob/main/LICENSE)；本地 `cargo info docx-rs@0.4.22` 声明 MIT；读/写 DOCX | 原型关闭默认 `image` 特性；原生实际执行，联合 WASM 编译；不宣称图片关闭配置可保真。上游 WebAssembly 定位不是本产品浏览器验收 |
| epub-builder 0.8.3 | [版本化文档](https://docs.rs/epub-builder/0.8.3/epub_builder/)；MPL-2.0；生成 EPUB，非 EPUB 导入库 | 只启用 zip-library，关闭 zip-command；显式选 V30，文档支持 3.0.1，不写作 3.3 完全实现；提供 XHTML/CSS 仍是调用方职责 |
| zip 6.0.0 | [版本源码/元数据](https://docs.rs/crate/zip/6.0.0)；锁定包的 Cargo 元数据为 MIT；ZIP 容器预检 | 只开 deflate；独立读取上限与实际解压计数，不向磁盘解包。docx-rs 另解析了 zip 8.6.0，锁文件保留两版本，不能假定预检库与格式库行为完全一致 |
| quick-xml | [上游](https://github.com/tafia/quick-xml)；`cargo info quick-xml@0.37.5` 为 MIT；流式 XML 候选 | 本原型不自行构建 OOXML/EPUB 解析器；docx-rs 锁文件实际使用 0.41.0。事件解析不能替代关系解析、样式解释和保真模型 |
| 独立完整转换器 | 未在本次选择、安装或调用 | 进程依赖不适合作为本轮桌面/Web 共用路径；不能据此断言没有可用工具 |

DOCX 不只是一个正文 XML：WordprocessingML 文档涉及包部件与关系，段落和 run 是不同层次，见 [Microsoft 的结构说明](https://learn.microsoft.com/en-us/office/open-xml/word/structure-of-a-wordprocessingml-document)。EPUB 的阅读顺序、资源清单、导航和内容文档有独立约束，见 [W3C EPUB 3.3](https://www.w3.org/TR/epub-33/)；因此简单收集 ZIP 中的文本不等于章节导入。未来 EPUB 交付还需 [W3C EPUBCheck](https://www.w3.org/publishing/epubcheck/) 及阅读器检查，本次没有运行 EPUBCheck。

## 固定原型与可复现方法

[原型清单](cap07e-prototype/Cargo.toml)、[锁文件](cap07e-prototype/Cargo.lock)、[Rust 程序](cap07e-prototype/src/main.rs)、[样例与差异脚本](cap07e-prototype/probe.py) 构成独立 Cargo workspace；没有 worldline 依赖，不进入产品默认构建/加载。仅用于合成、小型研究输入，不用于处理用户作品。

原型用 `read_docx → build → pack` 实际往返 DOCX，用 `EpubBuilder` 实际生成 EPUB。Python 标准库生成固定部件、检查 XML 文本/引用/资源并记录差异；不是用 ZIP 打开成功替代格式成功。输入部件先经 ElementTree 语法检查，但没有 OOXML schema/Word 验证，故结论只适用于本合成夹具和所选特性，不把失败概括为该库所有文档均失败。

从组合目录执行（`python` 为 Python 3；不需要 Word 或 Python 第三方包）：

```powershell
cargo build --manifest-path worldline/docs/adr/cap07e-prototype/Cargo.toml --locked --target-dir .scratch/cap07e-target
python worldline/docs/adr/cap07e-prototype/probe.py .scratch/cap07e-target/debug/cap07e-prototype.exe .scratch/cap07e-evidence
cargo check --manifest-path worldline/docs/adr/cap07e-prototype/Cargo.toml --locked --target wasm32-unknown-unknown --target-dir .scratch/cap07e-target
cargo check --manifest-path worldline/docs/adr/cap07e-prototype/Cargo.toml --locked --target wasm32-unknown-unknown --features browser-rng --target-dir .scratch/cap07e-target
```

`probe.py` 当前**预期退出 1**：它记录“损坏 XML 应拒绝”的期望，但实测库返回成功，报告该不符合项；这不是全部验收通过。第三条 WASM 命令预期失败，第四条增加显式随机源后编译成功。产物与本机原始报告位于 `.scratch`，不上传生成 ZIP、编译缓存或本机日志。换系统时调整可执行文件扩展名。

固定复杂输入 `fixture.docx` SHA-256：`7c5567effcb59769a2773660c81f865680074fc191cb5515220e911d76eb9bb6`。脚本显式固定 ZIP 成员时间并输出 SHA；不同压缩库版本可能改变容器压缩字节，应同时检查脚本内容和语义差异，不把跨机器哈希变化自动当语义变化。

## 实际结果与损失矩阵

本机：Windows，`rustc 1.98.0 (88d9e12ae 2026-08-18)`，原生 debug 构建；2026-09-26 单次采样。以下不是 Word/阅读器视觉结果，不是用户验证，也不是跨版本兼容保证。

| 内容 | 固定输入与期望 | DOCX 实际往返 | EPUB 实际导出 / 决策 |
|---|---|---|---|
| 中文/emoji | `中文 🌌`；复杂样例另含 `&amp;` | 简单样例文字保留；复杂输出 XML 解析失败，不能判保真 | XHTML 原字节保留；不证明字体、换行或屏幕阅读效果 |
| 章节/标题 | 第一章、Heading1 | 简单样例 Heading1 引用保留；没有自动推断书稿章节 | nav/OPF 存在，单章标题由调用方给出；多章顺序需未来 core DTO |
| 链接 | DOCX 外部 hyperlink；EPUB 内部脚注锚点 | 复杂输出未找到原链接 Target | XHTML 锚点原字节保留；目标完整性/阅读器点击未验；外部链接不得自动抓取 |
| 相对图片 | DOCX `media/pixel.png`；EPUB `images/pixel.png` | 复杂输出不含原 PNG 字节，部件名丢失；本配置禁用 image | 生成章节相对路径能在包中解析且 PNG 字节相同；没有图片解码或布局验收 |
| 脚注 | 脚注正文与引用 | 复杂输出 XML 部件中未找到脚注文本 | XHTML 脚注标记与文字字节保留；呈现/互操作未知 |
| 批注 | Word 批注与正文范围 | 批注文本仍在某 XML 部件；范围/作者/日期保真未证明 | XML 注释只是示例字节，不映射 worldline 讨论模型；默认不导出私人审阅数据 |
| 样式 | 标题引用、粗体、CSS | 简单样例粗体元素保留；字体、分页、列表、表格和布局未测 | 提供简单 CSS；主题/字体、页码、复杂样式均不承诺 |
| 未知字段/部件 | 未知命名空间元素、customXml | customXml 部件丢失；复杂主 XML 无效，元素语义检查为 null | XHTML data 属性原字节保留，不证明未知 EPUB 部件往返；EPUB 导入未实现 |

复杂 DOCX 的输出语法错误：`not well-formed (invalid token): line 1, column 1178`。命令退出 0 与格式成功明显不同。原型没有“修复”数据后宣布通过；纯文本小样的成功也不能抵消复杂样例的丢失。

| 负例 | 实际行为 | 限制 |
|---|---|---|
| 未闭合 XML | docx-rs 返回成功，偏离拒绝期望 | 上游解析不能代替严格文档校验；本原型预检不检查 XML 全部结构 |
| 截断 ZIP | 拒绝，找不到 EOCD | 不是所有损坏类别的覆盖 |
| DTD/实体声明 | 原型预检拒绝 `dtd_rejected` | 不执行外部实体；UTF-16 等非 UTF-8 XML 也未作为支持子集 |
| `../escape.xml` | 拒绝 `unsafe_or_duplicate_path` | 未向磁盘解包；并未证明所有路径规范化攻击已覆盖 |
| 高压缩比 padding | 拒绝 `ratio_limit` | 合成压缩炸弹替代样例，不使用危险公开样本 |
| 单部件 4 MiB + 1 | 拒绝 `entry_limit` | 部件上限只是原型实验参数 |
| 输入 1 MiB + 1 | 拒绝 `package_limit` | 读取封顶；不是产品文件大小承诺 |
| `vbaProject.bin` | 拒绝 `binary_part_rejected` | 粗粒度拒绝全部 `.bin`，不声称识别所有宏/嵌入对象格式 |

## 内存、解压边界与 WASM

原型实验值：输入最多 1 MiB，最多 64 个 ZIP 成员，单成员最多 4 MiB，累计实际解压最多 8 MiB，声明压缩比最多 100。部件逐个限流读取，拒绝越界名称、重复名称、符号链接与 DTD，不把压缩文件头声称的大小当唯一上限。这些数值只存在于可丢弃原型，不写入产品阈值契约。

Rust 全局分配器记录整个进程的**峰值在用堆字节**，不是 RSS、浏览器内存或强制分配预算；计时为一次 debug 进程内采样，不含编译和进程启动，不用于性能承诺：

| 样例 | elapsed_us | peak_rust_heap_bytes |
|---|---:|---:|
| DOCX 复杂往返 | 5832 | 133737 |
| DOCX 简单往返 | 3620 | 113761 |
| 畸形 XML 被接受 | 5830 | 109868 |
| 高压缩比拒绝 | 884 | 67443 |
| 超单部件上限拒绝 | 863 | 71535 |
| 超包上限拒绝 | 1361 | 3146054 |
| EPUB 导出 | 3128 | 444347 |

输入上限不等于堆上限：例如封顶读取的 Vec 容量增长仍使峰值高于输入大小。格式库会再次读取包并建立模型；当前没有 CPU 时间/递归深度/总分配强制限制。脚本给每个进程 30 秒超时，但这不是产品进程内取消机制。加密、重复成员、符号链接、累计上限、CRC 伪造和深层 XML 的规则尚无完整负例覆盖；不能宣称安全边界验收完成。

默认 WASM 检查实测因 `uuid` 未指定随机源失败。原型 `browser-rng = ["uuid/js"]` 后，联合依赖在 `wasm32-unknown-unknown` 编译通过。原型主程序使用文件 API；浏览器实际接入仍需字节输入/输出适配、JS 随机源与主线程阻塞处理，**没有浏览器运行测试**。这不改变桌面/Web 产品独立验收要求。

## 未来实施拆分与重新开启门槛

若未来选择继续，首先由 core 票制定书稿投影到导出 DTO 的契约：稳定章节身份、明确顺序、标题层级、公开内容范围、链接策略、素材边界和逐项损失报告。纯文本可以保留为文本；不能还原的样式必须报告，不能被导入器静默转换成虚构语义关系。原文件保持不可变，导入只生成预览提案，经明确确认才进入 Project 事务。

独立适配器票负责 EPUB 有限单向生成、依赖/许可证复核、实际字节预算、取消与错误、EPUBCheck、至少两阅读器和 WASM 字节 API 运行验证；之后 editor 票只做章节/公开范围选择、损失确认与下载/保存，不再解析 DOCX 或 EPUB。图像检查包括解码尺寸上限；出站链接不触发抓取。DOCX 若重新开启，须扩充带真实编辑器来源且可分发的合法样例，先解决损坏拒绝和未知数据丢失，再决定是导出还是导入，不能直接沿用本原型。

研究交付已有可复现样例、实际差异、内存测量和编译支持记录；**产品保真、损坏拒绝、阅读器、完整资源边界与浏览器运行仍未通过**。本 ADR 的结论是延期，不是这些能力已交付；是否据此关闭研究票由主任务评审，原型失败不得被计入产品测试通过数。
