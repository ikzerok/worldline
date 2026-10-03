# 版本变更

## 0.19.0

### 当前稿的可信诊断来源

- 正式词法、解析和分析生产入口保留原稿来源证据；主/related 独立区分 target、expression、statement、declaration、document、unavailable，不按 code、message 或同名文字猜位置
- 表达式、正文插值、choice 条件、转义端点和 include 合并语句按实际来源归属投影；有证据才给 span，缺证据明确降级，生产者矩阵不等于每个恢复分支均已实测
- Problems schema 仍为 1，位置加法提供 context.version=1、有界原文、全稿 slice/局部 hit 范围及 full/partial/no_text；512 字节窗口沿 grapheme 边界显示，0 字节预算仍保留角色与完整权威位置
- 上下文参与报告摘要和完整位置重验；旧无 context 报告可只读查询/分页，导航须刷新。未知版本、来源冲突、观测变化或位置证据不符不能跳到另一处原文
- RPC 新增工具能力 authoring.problem_source_context.v1；旧 Diagnostic 八字段形状、请求、游标与退出码保留，新能力不进入语言或 Story save
- 完整 report 限 32 MiB；主/related 页以及 `wl problems`、成功解析识别的 `project.problems` 完整 JSON 加 LF 各限 1 MiB；通用 JSON 解析失败不属于方法预算。context 与兼容 excerpt 重复字节均计入，成功/错误外壳和巨大 RPC id 也有明确预算行为

### 语义、兼容与验收

- 默认语言 1.9、最高既有 1.13，不新增 DSL 或诊断事实；来源元数据不改变指纹、choice signature、once 身份、保存和运行语义，runtime_version 守卫保持
- 0.18 普通 Story save 的同源码样本已在 0.19 完整继续；当前 0.19 新 trace 重放测试通过。这两项不表示旧 trace/checkpoint 能跨版本重放
- 本地完整 core 1135 通过、0 失败、7 性能专项 ignored；fmt、全目标严格 Clippy、工作区构建通过。Windows 路径 fixture 修正后44项集中回归通过，完整 Windows CI 同步验收；不把普通/原型或不同平台结果相加为唯一测试数

使用见 [工程问题](docs/problems.md)，正式范围见 [诊断来源](spec/diagnostic-sources.md)与[来源上下文](spec/problem-source-context.md)。配对编辑器的完整能力、负面边界及最终验收记录见 [0.19 说明](../worldedit/docs/releases/v0.19.0.md)；跨仓链接用于同级检出。

## 0.18.0

### 当前缓冲的统一工程问题报告

- core 新增只读 ProblemsReport，汇总活动语言内容、工作区清单及十类已注册静态资料域；原始未知格式和坏 UTF-8 保持，不迁移或自动修复
- 报告独立呈现严重性、逐域覆盖、partial/truncated、来源观测与预算；文档级位置不伪造精确字符，主位置与全部相关位置使用同一有界来源接口
- 一次报告至多编译一次；查询、分页与来源展开零编译。编译只读取当前活动缓冲，不偷偷补读未加载的磁盘 include
- CLI `wl problems` 与 RPC `project.problems` 消费同一 core DTO；报告绑定的问题 ID、分页游标和观测检查拒绝跨报告误定位，稳定 partial 可缓存，变化中的观测必须重建
- 统一查看不统一阻断：旧 check、故事运行、保存、导出与发布继续各自门禁。本地化交换包的选中字符串检查及实际发布选择重验不属于静态报告
- 默认语言 1.9、最高既有 1.13 与运行语义不变；错误优先于提醒、提示排序。没有新 DSL、严重性策略切换或自动修复

使用见 [工程问题](docs/problems.md)，精度、覆盖和预算见 [规范](spec/problems.md)。普通 Story save、版本绑定 trace 与 checkpoint 的兼容边界必须分别核对，不能从语言版本未变推断跨工具版本重放。

## 0.17.0

### 可靠源码整理与只读解释

- 规范无语义次序的正式引用多重集，修复安全移动预览与应用因无序枚举随机误拒；重复引用及语句、选择、效果等有序语义保留
- 新增四类失败回执 SourceChanged / IllegalPath / SemanticChange / UnableToProve；继续整体重验、一次内存应用及既有可恢复 journal 保存，不承诺跨文件物理原子
- 新增 schema 1 世界对象精确查找和一至两跳焦点上下文，六类来源、真实精度、限定范围、完整性及预算分开表达；文字提及默认关闭
- 新增偏序比较、稳定 follows 证据链、真正强连通环与受阻下游投影，保留 A213 数量、旧语言比较范围及 partial 保护
- CLI / agent 使用同一 core，协议仍为 1，以新能力字段发现入口；现有关系查询语义不变，默认语言 1.9、最高既有 1.13 不变

### 重放与验证边界

- 0.16 trace 交给 0.17 按既有 runtime_version 守卫拒绝；普通 Story save 的同源码 fixture 已加载并逐字段核对，跨版本 checkpoint 未独立实测，三者不能互相推断
- 同 0.17 合法源码移动、保存重开后，入口 trace、checkpoint-origin trace 和普通 save 已独立验证
- 候选本地 core 1041 通过、0 失败、4 性能 ignored；最终合并、配对与正式发行以对应 CI 和版本报告为准

## 0.16.0（开发中）

### 语言、core 与机器契约

- 新增 core 源码生命周期请求/完整计划：显式源码集合中新建活动文件同步维护 active 与 include；引用归档或非活动源码明确拒绝，不暗中启用
- 支持单个非入口 `.wl` 的受控路径移动，逐处预览正式入站/出站路径与已理解展示文档引用；保持资源解析、加载顺序、默认入口、正式对象和运行指纹，无法证明时零改拒绝
- 计划绑定当前缓冲、保存基线、目的缺失及资源摘要；陈旧、篡改、外改、取消、冲突和不安全路径拒绝。应用一次内存事务，保存与撤销沿用既有可恢复机制
- CLI `source-lifecycle preview|apply` 与 agent `project.source_lifecycle_preview/apply` 消费同一 core 计划；业务失败保持 `ok:false`，协议形状错误才用 JSON-RPC error
- 默认语言仍为 1.9，最高既有显式语言仍为 1.13；不增加 DSL，不自动升级旧作品或激活归档

### SVG 与静态世界站

- typed scene 支持有界虚线数组/偏移、继承、odd 数列、none 与全零语义，通过 `presentation.vector_stroke_dash.v1` 同时保护地图和 scene；缺能力的显式字段只读保留
- 安全 SVG 导入、原生场景编辑、共享 serializer 与读者地图贯通。非支持单位/百分比、有效虚线文字和超预算输入整批拒绝，不静默改为实线
- 在进入渲染后端前核对派生 dash 工作量、f32 精度与大旋转椭圆；SVG 输出规范化弧旋转，保留存储中的原角度
- 读者地图仅在实际带 href 的导航链接上抑制拖动；没有 href 的 SVG 定位锚点仍可起拖，避免底图拖动被误拦
- 读者关系图采用独立图行、两行 Unicode grapheme 摘要、裁切边界和完整 HTML 文本链接，长标签不再溢出碰撞目标；公开白名单、稳定路由和静态阅读边界保持不变

### 使用与边界

- 源码组织见 `docs/source-lifecycle.md` / `spec/source-lifecycle.md`；SVG 见 `spec/vector-scene.md`；阅读站见 `spec/reader-site.md`
- 不支持入口/目录批量/素材移动；不能保持加载顺序、资源或存档兼容时拒绝。不是自动合并工具
- 世界 description 仍是既有纯字符串；静态世界站不执行故事条件、效果或状态，不是可玩 runtime 包
- 本开发版本尚未公开发行。完整最终配对验证及真实平台覆盖另行记录，早期 draft CI 不作最终验收


产品版本与语言版本独立。当前默认语言为 1.9，最高支持 1.13；打开旧作品不会自动启用新能力。历史变化与兼容说明另见 [发布说明](docs/release.md)。

## 0.15.0 — 开发候选，未正式发行

### 原生矢量地图

- 地图新增可选 `scene`，通过 `presentation.vector_scene.v1` 显式启用；地图 schema 仍为 1，旧 placements、文字、底图和测量继续保留
- 点、折线、多边形、矩形、椭圆、路径、文字和组使用同一可编辑模型；路径保存直线、二次/三次贝塞尔、圆弧、多子路径与闭合段，渲染采样不会回写替换曲线
- 保留组顺序、父子关系、图层、显隐、锁定、仿射变换与受控样式；旧标记与 scene 节点共享同地图 ID 域，按真实图层顺序绘制
- 批次支持创建、更新、删除、排序、复制、成组/拆组、移层、导入与显式迁移；预览绑定修订和文档 hash，成功应用是一次内存事务，保存沿用工作区规则
- 旧标记迁移保留稳定 ID、引用、批注锚点、样式、未知可选字段和叠放；仅接受可保真的整层或连续后缀，无法保持顺序或样式时整批拒绝
- “新建地点并绑定”组合源码与地图修改；核心一次预览和应用，避免只创建资料或只写入链接
- 未知必需能力/格式只读保留原始字节；陈旧、锁定、冲突、非法几何和取消不会提交部分候选。纯几何编辑不要求故事已经可运行

准确模型、操作与限制见 [矢量场景规范](spec/vector-scene.md)。

### 受限 SVG 交换

- 接受单根 SVG、组、基础图形、原生路径、纯文本与基础 tspan；支持受控颜色、描边、透明度、文字样式、仿射变换和非零 viewBox
- 支持 `preserveAspectRatio` 的 none、对齐及 meet/slice；导入根 viewport 的矩形裁剪随 typed 场景保存，安全导出可再次导入
- 二次/三次曲线、圆弧、文字与组保持可编辑。导入后的场景是编辑真源，输入 SVG 不作为可执行页面或第二份编辑真源
- 整批拒绝脚本、事件属性、DTD/实体声明、外部资源、href、foreignObject、style 元素、任意 CSS、filter、mask、动画、未知元素/属性与嵌套 SVG viewport；给出可定位原因并保留输入
- 裁剪仅接受核心定义的根 viewport 矩形交换形式，不开放任意 clipPath、复合裁剪或外部引用；不承诺完整 SVG/CSS、文字路径或外部字体兼容
- `map_to_safe_svg` 输出旧图元与 scene 的完整矢量部分；底图、作者语义和原始扩展仍由完整工程备份保留。旧 `svg_import::preview/apply` 保持旧采样 placements 契约，新入口使用 scene API

### 完整静态世界站

- 显式选择 DTO v3 与 `reader.world_site.v1` 提供首页、类型目录、资料详情、时间结构、关系目录/局部图、静态故事、书稿/章节和地图页
- 选中对象授权其 display、全部别名及允许的类型结构；property 仍逐键授权，地图图元、章节、底图和附件分别选择，引用不会自动公开目标
- 搜索包含类型、别名和公开正文，支持中文子串、英文大小写不敏感、类型过滤与摘录；地图标记可定位到公开锚点
- 时间结构只展示公开端点的明确偏序，关系和反向链接只消费已授权内容；隐藏目标使用通用提示，不泄漏私有 ID、名称、源码路径或备注
- `reader.story_details.v1` 额外授权静态条件与效果说明；含私有身份或不能安全投影的表达式整条隐藏。站点不执行条件、效果或状态，不是可玩的故事运行时
- 地图按精确节点白名单公开，选组不自动选后代；只保留显示所选节点必需的祖先变换、样式和裁剪。作者默认显隐不代替公开授权
- v3 使用与排序和显示名无关的稳定路由；站点只使用本地相对资源，搜索由本地脚本加载。manifest 为资源提供字节数与 FNV-1a-64 校验，该校验不是安全签名
- 发布 profile 保存选择与稳定路由，支持预览、保存计划和旧选择迁移；未知字段保留，失效选择不会静默裁掉，旧 v1/v2 不自动扩大公开范围
- 原生站点导出只写工作区外尚不存在的新目录；过期、错误、取消和目标竞态不能覆盖已有站点。发布不自动保存作者原稿

阅读包只控制生成时的内容选择，拿到包的人可以读取其中全部内容；它没有在线账号权限，也不能替代完整工程备份。详见 [世界站规范](spec/reader-site.md)与[旧选择兼容边界](spec/reader-export.md)。

### 机器接口与后台快照

- 新增 `wl scene svg-preview / preview / apply / export`；RPC 为 `scene.svg.preview`、`scene.preview`、`scene.apply`、`scene.export`，通过 `authoring.vector_scene.v1` 发现
- CLI/RPC 共用 core 计划、摘要和安全 SVG；apply 重验原请求、内容基线、修订、文档 hash 与磁盘冲突，不接受任意候选文档字节
- v3 站点沿用 `wl reader-export preview/apply` 和 `reader.export.preview/apply`，没有另一套公开选择或站点生成器
- 当前缓冲快照保留源码、已注册展示文档、只读状态、坏 UTF-8 展示原文、普通附件和删除墓碑；后台计算不刷新、迁移、保存原工程
- typed 请求和二进制负载分别限额；捕获与重建都检查路径、数量和实际字节。Windows 原生分隔符局部规范化，跨平台传输路径保持 `/`，不放宽工作区逃逸检查

参数与保存差异见 [场景协议](spec/scene-protocol.md)、[机器协议](spec/agent-protocol.md)和[后台快照](spec/workspace-snapshot.md)。

### 配对编辑器与资源边界

配对 worldedit 使用同一 core 提供绘制、节点/组/图层编辑、SVG 导入导出、四步读者发布，以及主导航、索引、内容和检查器的共同布局。语言工具不遥控运行中编辑器的手势、窗口或未应用稿；编辑器流程见其 [文档索引](../worldedit/docs/README.md)，此跨仓相对链接用于两仓同级检出。

SVG 单次输入上限为 2 MiB、1000 图元；每图 scene 上限为 5000 节点、32 层级。v3 公开对象上限为 2000，旧 v1/v2 仍为 500；静态站点上限为 10000 文件、128 MiB。附件、路径段、文档、像素及后台快照还有各自限额，以[阈值登记表](spec/presentation.md#11-阈值登记单源)和[发布预算](spec/reader-site.md#7-原子性预算与验证)为准。预算不等于达到上限时的交互性能保证。

### 当前验证状态

0.15.0 仍为开发候选，未正式发行。Linux workspace 与 core head `90000754940c9b8630d294732912c9a9daea79d3` 的 Windows CI 均为 925 通过、0 失败、3 ignored，格式及严格 Clippy 通过；1000 SVG 和 2000 对象世界站的 release 固定负载通过相应门槛。profile 曾出现 269 ms 超限，等价索引优化后重新通过 core 与完整 UI 提交测量，保留原失败记录。

精确提交、机器、fixture、全部测量和待验范围见 [0.15 验证摘要](docs/verification-0.15.md)。真实 `file://` 被工具安全策略拒绝，正常 HTTP 重试也被客户端阻断，实际离线网页与 Browser Worker 未验；静态资源审计、Node WASM 或配对编辑器原生功能结果不能替代浏览器验证。
