# 研究记录、代码依据与技术取舍

**核对日期：2026-09-19；设计状态：待评审。** 本次已通过GitHub连接读取两个仓库，不是只根据README提出建议。外部功能资料和依赖信息均使用项目维护方/官方文档。

## 1. 已确认与未验证

已确认：两仓库本次最新提交与前次调研基线相同；现有对象引用、编辑事务、工作区文件、编辑器状态、附件打开和ZIP约束。

未验证：没有克隆并编译两仓库，没有运行Rust测试、GUI或实际性能基准。本包内Python验证只检查文档契约样例、需求依赖和若干纯函数性质，不是产品实现验收。读取源码不等于确认不存在其他实现；结论限定在列明文件及当前规范。

## 2. 影响实施的发现

| 发现 | 证据 | 设计影响 |
|---|---|---|
| Project只有wl文档缓冲，refresh仅纳入wl | R04 | 地图JSON必须进入统一编辑/保存体系，不能由UI直接写磁盘 |
| Project::edit克隆候选并拒绝任意编译错误 | R06 | 复用事务思想；新增Presentation命令单独校验，防止一处未完成正文锁死地图修复 |
| 原保存预检后逐文件写入，并逐文件推进保存基线 | R04 | 不能称为跨文件原子提交；需故障记录、整体结果与恢复流程 |
| export_file_contents检查无事件并拒绝打包 | R04 | 纯设定项目的工程保存/交换不应要求可运行故事 |
| TargetRef已是kind/id，目录反查目前按列表过滤 | R05 | 保留身份契约，新增按身份、入边、出边、标记的派生索引 |
| UI有共享快照和Vec<Project>历史 | R07 | 将新元数据纳入Project即可复用历史思路；图片字节不能跟随每步历史全量复制 |
| 图片当前主要交外部应用打开 | R08 | 地图需要新增解码/纹理/裁剪/命中管线，不把附件支持误算成已有地图支持 |
| 浏览器包限制4096文件、64MiB | R09 | 首版不宣称支持任意超大地图；缓存和缩略图不得挤爆包体 |
| CI未固定worldline checkout版本 | R12 | 配对SHA进入CI和发布记录；路径依赖的lock文件不能固定旁边仓库的实际内容 |

## 3. 技术结论

**采用：** 当前Rust/egui；由core拥有语义和展示文档验证；.wl存世界内容及明确关系，JSON仅存地图/版式/模板/协作元数据；本地文件/Git作为协作基础；内存索引支持局部网络；二维图片地图作为首版。

**不采用：** sim crate、世界调度器、NPC代理、资源结算、地图绑定StoryState；也不以三维引擎、图数据库、在线地图服务、完整GIS、实时协同服务器或全面新DSL重写为前置条件。

**渲染验证：** 官方0.32.0 Scene可作缩放和平移参考，但首版建议将Camera2D和命中测试做成独立纯函数，标签与图标在屏幕层保持固定字号。须在仓库实际锁定版本编译小型验证后决定直接用Scene还是封装Painter，不把本设计示意写成已可编译调用。[E01]

**图像验证：** 候选image仅启用PNG/JPEG；先读头和尺寸、检查像素乘法溢出，再解码。不能只设置max_alloc就声称有硬内存上限。必须同时控制输入尺寸、单图像素、并发任务与CPU/GPU缓存预算。[E02]

**关系查询：** 首版用BTreeMap/HashMap身份索引与邻接表，不需要服务端图库。自动布局只生成坐标；关系连线仍由作者提交。热点有基准数据后才考虑空间索引或图算法依赖。

**大型底图：** M1限制尺寸并提供明确错误。若M4确有大图需求，再单独评审预览图和瓦片缓存；产生的仅为显示缓存，不是程序生成地形。不要先解码任意大图再声称已限流。

## 4. 同类产品启发的落点

E03/E04支持本方案的“底图—标记—条目—子地图”阅读路径；E05支持跨对象关系的产品需求；E06支持可选模板与问题引导。它们只证明功能方向已有先例，不能证明本项目的实现难度、性能或兼容性已经解决。

## 5. 来源登记

编号说明：本节R01–R18是**研究来源登记**，与[需求清单](REQUIREMENTS.md)第3节“原报告覆盖”里的报告任务编号（同样以R开头）不是同一命名空间；引用时以文档区分。

### R01 · worldline 当前基线提交

`https://github.com/ikzerok/worldline/commit/dcb6479873f86b08354ba92b098dc260aa976624`

本次连接读取提交元数据；0.2.0，提交日期2026-09-10。

### R02 · worldedit 当前基线提交

`https://github.com/ikzerok/worldedit/commit/b0955adcccd740d9b3b46cee1d0f503ad25f5257`

本次连接读取提交元数据；0.2.0，提交日期2026-09-10。

### R03 · worldedit Cargo.toml

`https://github.com/ikzerok/worldedit/blob/b0955adcccd740d9b3b46cee1d0f503ad25f5257/Cargo.toml`

本次读取；egui/eframe 0.32范围，worldline为同级路径依赖。

### R04 · worldline core/src/project.rs

`https://github.com/ikzerok/worldline/blob/dcb6479873f86b08354ba92b098dc260aa976624/core/src/project.rs`

本次读取1–250及290–435行，并搜索无事件打包限制；缓冲仅扫描wl，保存逐文件替换。

### R05 · worldline core/src/catalog.rs

`https://github.com/ikzerok/worldline/blob/dcb6479873f86b08354ba92b098dc260aa976624/core/src/catalog.rs`

本次读取1–180行；TargetRef、对象目录、引用和线性反查实现。

### R06 · worldline core/src/authoring.rs

`https://github.com/ikzerok/worldline/blob/dcb6479873f86b08354ba92b098dc260aa976624/core/src/authoring.rs`

本次读取220–330行；Project::edit在副本编译成功后提交。

### R07 · worldedit src/app.rs

`https://github.com/ikzerok/worldedit/blob/b0955adcccd740d9b3b46cee1d0f503ad25f5257/src/app.rs`

本次读取1–205行；共享Snapshot、阅读目标、图坐标、Vec<Project>撤销记录。

### R08 · worldedit src/media.rs

`https://github.com/ikzerok/worldedit/blob/b0955adcccd740d9b3b46cee1d0f503ad25f5257/src/media.rs`

本次完整读取；当前附件在系统应用打开或由浏览器下载，不是地图纹理管线。

### R09 · worldedit src/archive.rs

`https://github.com/ikzerok/worldedit/blob/b0955adcccd740d9b3b46cee1d0f503ad25f5257/src/archive.rs`

本次读取1–215行；4096文件/64MiB限制、路径检查、旧入口清单worldedit-project.json。

### R10 · worldline core/src/compiler.rs

`https://github.com/ikzerok/worldline/blob/dcb6479873f86b08354ba92b098dc260aa976624/core/src/compiler.rs`

本次读取1–170行；内存覆盖与目录递归加载共用编译。

### R11 · worldedit rust-toolchain.toml

`https://github.com/ikzerok/worldedit/blob/b0955adcccd740d9b3b46cee1d0f503ad25f5257/rust-toolchain.toml`

本次读取；工具链固定1.98.0。不表示本环境已经安装或执行该版本。

### R12 · worldedit CI

`https://github.com/ikzerok/worldedit/blob/b0955adcccd740d9b3b46cee1d0f503ad25f5257/.github/workflows/ci.yml`

本次读取；Windows任务另行checkout worldline但未指定ref，包含WASM clippy。

### R13 · worldline spec/semantics.md

`https://github.com/ikzerok/worldline/blob/dcb6479873f86b08354ba92b098dc260aa976624/spec/semantics.md`

此前会话已读取，基线未变；区分作者资料和演练状态，定义指纹与效果时序。

### R14 · worldline spec/relations.md

`https://github.com/ikzerok/worldline/blob/dcb6479873f86b08354ba92b098dc260aa976624/spec/relations.md`

此前会话已读取，基线未变；RelationGraph为剧情控制流，时间偏序另行定义。

### R15 · worldline spec/catalog.md

`https://github.com/ikzerok/worldline/blob/dcb6479873f86b08354ba92b098dc260aa976624/spec/catalog.md`

此前会话已读取，基线未变；标签、对象链接、别名、素材和锚点。

### R16 · worldline core/src/ast.rs

`https://github.com/ikzerok/worldline/blob/dcb6479873f86b08354ba92b098dc260aa976624/core/src/ast.rs`

此前会话已读取，基线未变；属性为字符串/数值/布尔，人物关系尚无独立ID。

### R17 · worldline spec/workspace.md

`https://github.com/ikzerok/worldline/blob/dcb6479873f86b08354ba92b098dc260aa976624/spec/workspace.md`

此前会话已读取，基线未变；全部wl递归分析、目录边界和完整工程复制。

### R18 · worldedit docs/cli-coverage.md

`https://github.com/ikzerok/worldedit/blob/b0955adcccd740d9b3b46cee1d0f503ad25f5257/docs/cli-coverage.md`

此前会话已读取，基线未变；现有CLI/RPC不具备编辑器写入控制。

### E01 · egui 0.32.0 Scene参考实现

`https://github.com/emilk/egui/blob/0.32.0/crates/egui/src/containers/scene.rs`

本次从官方仓库读取；支持缩放、平移和变换。注释提醒放大时文字模糊。此为0.32系列参考，不代表已核对lock内补丁版本。

### E02 · image 0.25.8 Limits

`https://docs.rs/image/0.25.8/image/struct.Limits.html`

本次读取；宽高限制为严格限制，max_alloc不是所有解码器都保证遵守。作为候选依赖研究，不是已集成。

### E03 · World Anvil互动地图

`https://www.worldanvil.com/features/maps`

本次读取；上传地图图片、关联资料、地图互链和图层。不照搬跑团中的位置跟踪。

### E04 · LegendKeeper功能表

`https://www.legendkeeper.com/features/`

本次读取；标记、嵌套地图、标签筛选与Wiki连接。仅借鉴展示组织，不承诺复制云端协作。

### E05 · Kanka功能表

`https://kanka.io/features`

本次读取；世界条目、关系、地图等功能类别，用于确认跨类型内容组织方向。

### E06 · World Anvil创作模板

`https://www.worldanvil.com/features/worldbuilding-templates`

本次读取；多种可定制世界设定模板及创作提示。本包模板问题为原创设计。

### E07 · Cargo依赖指定官方文档

`https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html`

本次读取；path与git依赖、rev及锁文件语义，用于跨仓库版本配对方案。

## 6. 仍须通过工程验证关闭的问题

字体和图层缩放在锁定版egui下的清晰度；Windows逐文件替换及中断恢复；WASM解码是否阻塞主线程；原有指纹在新声明加入后是否保持预期；共享文件中未知JSON字段的保留；两仓库成对构建。这些都已进入实施计划，不以“不确定”作为省略设计的理由。
