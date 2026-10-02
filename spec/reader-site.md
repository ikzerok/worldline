# 静态世界网站与发布配置

本规范扩展 [reader-export.md](reader-export.md) 的 v1/v2；类型分析、正文、地图和授权均由 core 产生。阅读包是公开内容的离线静态投影，不提供在线身份权限，也不执行故事条件、效果或状态迁移。界面不得称其为可玩的故事运行时。完整工程备份仍保存全部普通文件，包括未引用文件；不能用阅读包替代备份。

## 1. 选择版本与明确授权

`ReaderExportSelection` 保留原字段和类型：

```text
schema_version: u32
required_features: Vec<String>
site_title: String
objects: Vec<TargetRef>
fields: Vec<ReaderFieldSelection>
maps: Vec<ReaderMapSelection>
manuscripts: Vec<ReaderManuscriptSelection>
attachments: Vec<String>
```

`ReaderFieldSelection={target:TargetRef,keys:Vec<String>}`；`ReaderMapSelection={id:String,placements:Vec<String>,raster_layers:Vec<String>}`；`ReaderManuscriptSelection={id:String,chapters:Vec<String>}`。所有选择 DTO 拒绝未知字段。省略 required_features、fields 或 maps 等于空数组，其余原字段不变；章节数组决定阅读顺序。

v1 不允许字段或新能力。v2 必须且只能声明 `reader.fields.v1`，即使 fields 为空也保留该能力。v3 必须声明 `reader.world_site.v1`，可额外声明 `reader.fields.v1` 和 `reader.story_details.v1`；fields 非空时必须有字段能力。重复能力、未知能力和未知版本均拒绝，不静默降级。这些是选择级能力，不写入工程清单。

v3 对象选择明确公开 display、全部对象别名和本规范允许的类型结构；UI 在勾选前说明。property 仍逐键选择，默认不选；支持原有 character/entity/world/relation，键名和值同时公开。附件必须单独选择 asset ID。故事条件和效果还须额外勾选 `reader.story_details.v1`，v3 本身不授权它们。

对象、字段、地图、章节和附件的引用均不扩大选择。公开对象类型沿用 event、fragment、rule、scene、character、entity、world、storyline、period、anchor、state、tag、relation、variable；asset 只能进 attachments，map 只能进 maps，文件路径不能冒充对象。

未公开端点用一般性提示，不输出其 ID、显示名、源路径或作者备注。反向链接只来自允许的正文链接、明确的类型关系和显式选择的强引用字段；不得遍历完整 catalog.references 公开未勾选属性、speaker、效果等隐含联系，即使两端页面都公开。

## 2. 页面与语义投影

站点包含首页、typed 分类目录、对象详情、书稿目录及章节正文、时间结构、关系目录和局部图、地图目录/矢量页、静态故事目录及分支说明。对象、关系、地图与故事之间仅链接到本次已公开页面。

| 内容 | 允许投影 |
| --- | --- |
| 所有公开对象 | 类型、display、aliases、已授权静态正文与逐键 property |
| entity | 声明的 subtype；不以 subtype 推导属性授权 |
| state | 公开 owner；隐藏 owner 用一般提示 |
| anchor | 明确关联的公开对象；未公开关联端点隐藏 |
| storyline / character | 已公开事件对故事线/人物的明确成员联系及安全反链 |
| variable | 值类型；不公开初始值 |
| event | 公开 period/storyline/character 成员，显式前后关系 |
| period | 公开父子层级、直接事件成员 |
| relation | 类型、方向、端点、scope；未公开目标隐藏 |
| 遗留人物关系 | 来源和目标都公开时展示；不伪造独立关系 ID |
| rule | 公共参数及返回类型签名；不输出内部计算表达式 |
| manuscript chapter | 选定标题/摘要；来源对象另行授权后嵌入其公共正文及前后章导航 |

timeline 表示显式偏序，不根据私有 rank、未公开中间节点或全图排序推断一个公共总顺序。公开事件没有公开时段时放入通用区域，不静默丢弃。关系局部 SVG 图仅包含已公开关系及端点。

开启 story_details 后按正式 AST 投影事件前提、效果摘要、choice 可见/可用条件、once、if/else 和跳转/END。表达式含任何未公开或未知标识时整条隐藏，连同其字面量，不只替换标识保留敏感文本。副作用和状态操作只显示允许引用的静态说明；不执行、不求值、不引入 runtime。say 仅公开文本，speaker/direction 不因正文或人物页被选中而授权。local/let、规则内部计算、作者 source_note 保持私有。相关页面标明“静态分支阅读，不执行条件或状态”。

## 3. 地图精确公开

placements 同时接受 legacy placement ID 与 scene node ID，二者在整张地图内唯一；重复和缺失选择报错。选中 group 不会授权其后代。scene serializer 仅保留选中节点和必要祖先样式/变换，祖先 ID/name/annotation/target/extra 不输出。显式选中节点不被作者默认隐藏状态筛掉；无选择过滤的普通 scene 导出仍尊重作者显隐。

公开节点链接由 reader 提供 `ScenePublicLink={href:Option<String>,anchor:String,label:String}`。href 只能为安全相对路径，anchor 只能含小写字母、数字和连字符且无冲突。节点 name 不自动成为公开标签；缺省 label 继承已公开目标 display，未公开目标使用一般提示。已选标记明确标签/注释及其可见文字属于选择内容，须在作者预览核对。

地图底图必须另行选择其 asset，且仅为既有图像白名单。作者 measurement、临时尺子、来源文件与未知 metadata 不进入包。Bezier 与变换由 core 正式 scene 模型序列化，reader/UI 不建立另一套 SVG 解析器。

每层先按既有稳定顺序画 legacy，再画该层 scene，最后进入下一层；选中顺序不改变堆叠。`to_safe_svg_layers_with_links` 一次校验 scene/links、一次收集祖先，然后逐层输出与 canvas/viewBox 一致的安全 SVG，不能每层重复整张图的准备工作。保留导入产生的透明 synthetic root 归一化，避免重复往返增深。

地图页具有本地缩放、平移与重置；图元的 opaque anchor 可供对象反链和搜索定位。map ID 用来区分同名地图，不能按标题寻找目标。scene/legacy target 只有在公开集合内才能建立站内链接。

## 4. 路由与公开资源

固定页面为 `index.html`、`objects/index.html`、`objects/kind-<kind>.html`、`manuscripts/index.html`、`maps/index.html`、`timeline.html`、`relations.html`、`stories.html`、`search.html`。v3 首页提供分类计数与导航。共有资源为 `style.css`、`reader.js`、`search-data.js`、`search-index.json`、`reader-manifest.json`。

v3 身份路径为 `objects/r<hash>.html`、`maps/r<hash>.html`、`manuscripts/r<hash>.html`、`assets/r<hash>.<允许扩展名>`。hash 是序列化身份 tuple 的 FNV-1a-64、小写 16 位十六进制；对象/资产/地图 tuple 为 `[kind,id]`，章节为 `["chapter",manuscript_id,chapter_id]`。同一路径冲突提前拒绝。FNV 用于稳定命名和误变校验，不是安全签名。路径不含原 ID、标题或工作区路径。

v1/v2 保持 `objects/o0001.html`、`maps/m0001.html`、`manuscripts/m0001-c0001.html`、`assets/a0001.<ext>`。保存的 profile 可以保留这些兼容路径；编号为至少四位十进制。路由覆盖仅接受上述对应类别的形状，不接受任意自定义文件名、绝对路径、反斜杠、scheme 或目录逃逸。目标身份重复和路由重复都拒绝。

选择次序、增加其他内容、修改 display/alias 不改变原 v3 身份路径。已保存 profile 重构身份时仅改 routes.target，保留 output_path。新增选择得到新路径，旧路由不重排；v1/v2 新身份从未被 routes 占用的序号槽分配，已移除身份保留的路径也不可重用；删选项不授予其原目标访问权，保留的路由表只用于未来恢复同一身份的稳定路径。

v3 搜索条目为 `{title,url,text,kind,aliases}`；v1/v2 保留 `{title,url,text}`。中文子串、英文大小写不敏感、kind 过滤和摘录均仅使用公共文本。每个 preview.content 主页面按 output_path 对应一个正文完全一致的搜索条目；地图图元可额外生成 `url#anchor` 条目，kind 为 map_placement；同一地图的额外条目按其公开正文片段出现顺序输出，title 为该片段的公开标签，text 为同一片段原文，供调用方用单向游标核对，不能附带不在该页公开正文中的文字。不能要求预览和索引数组 zip 相等。

所有 HTML、CSS、脚本与资源采用包内相对路径。搜索数据通过本地 script 载入，不使用 fetch、CDN、远程字体或服务器。深层页面和双击 file:// 打开遵守同一相对路径规则。搜索 DOM 使用 textContent，不把输入或索引正文当 HTML。

原生文件路径与公开 URL 分别验证。`reader_export::portable_output_path(&Path) -> Result<String,String>` 只对已经作为原生相对输出路径处理的值进行校验和 `/` 正规化：Windows 的原生反斜线分隔符可正规化；Linux/Unix 的字面反斜线文件名拒绝。所有平台继续拒绝盘符/UNC/设备名、绝对路径、父目录、空段、点段、控制字符及不安全的 URL 标点。公开 URL 和 profile 路由字符串始终只接受 `/`，不能调用原生正规化绕过其反斜线拒绝。包的键、manifest/search/preview 的公开路径及 profile.routes.output_path 序列化与 native 写入使用同一正规化规则；资源审计在 portable 字符串命名空间解析相对 URL，不再先构造带系统分隔符的 PathBuf 再当 URL 检查。此规则不适用于完整工程快照或 profile 配置原文的工作区文件名；合法的 a&b.json 等普通工程文件仍按原工作区契约保留。

manifest 包含 schema_version、title、pages、attachments，并增加 `hash_algorithm:"fnv1a64"` 与 `resources:[{path,bytes,hash}]`。resources 覆盖除 manifest 自身外全部文件；不包含作者专用 exclusions、源路径或未选 ID。hash 记录文件完整字节。生成后审计每个 HTML href/src、CSS url 和片段 ID：拒绝远程/绝对路径、逃逸、缺资源与缺 anchor，禁止 iframe/object/embed/base、内联 style 元素和 CSS import；SVG namespace 不是外部依赖。

## 5. 发布 profile 文档

工程清单使用 `reader_profiles:{"id":".world/reader-profiles/id.json"}` 注册，非空时要求 `reader.profiles.v1`。沿用 authoring document 路径、只读保护、原始字节保留、外部刷新、快照、撤销和保存事务；未知 schema/required feature 只读保留，不能编辑或删除。清单无 `ProjectManifest` 新 Rust 类型，仍由 Registry 管理。

`ReaderPublicationProfile` 是 schema 1：

```text
schema_version: u32
required_features: Vec<String> // 必须且只能 reader.profiles.v1
id: String                    // 1..80 个 ASCII 字母、数字、_ 或 -
title: String                 // 1..160 字符
selection: ReaderExportSelection
routes: Vec<ReaderProfileRoute>

ReaderProfileRoute {
    target: Option<TargetRef>,
    manuscript_id: Option<String>,
    chapter_id: Option<String>,
    output_path: String
}
```

route 身份要么是 target（包括 asset/map），要么是同时存在的 manuscript_id+chapter_id，两者互斥；章节的其中一个 ID 不能独存。profile 及清单拒绝重复 JSON 键。未知可选 profile 顶层字段允许读取，保存时按原 JSON 合并保留。route 与其他已知 DTO 拒绝未知字段。

已保存 profile 的失效对象、字段、map/placement/raster、章节和附件不能在候选刷新时自动裁剪；未知 required_features 不能丢弃。直到作者显式移除失效项之前，预览/保存报错。保持 selection 原数组顺序，尤其章节顺序；新章节按明确候选顺序追加。

补全路由时可临时借用 `(target, manuscript_id, chapter_id)` 建立有序集合索引，将 R 条已存路由与 N 条预览条目的身份查重成本从重复线性扫描降为 `O((R+N) log(R+N))`。索引只用于成员判断，原 routes 向量不重排，新身份仍按预览条目顺序追加；已有 output_path、完整字段和哈希规则保持不变。此项不是整站或保存过程的总复杂度承诺，也不是跨调用信任缓存；保存 apply 仍完整重算并复核所有基线与计划。

v1/v2 selection 向 v3 迁移保持 routes，并列出 aliases 和 typed 结构的新增公开授权；不自动添加 story_details、fields 或 attachments。apply migration 仅返回校验后的候选，不保存或发布。已有 v3 不重复扩大能力。

## 6. 公开 core API

以下类型位于 `worldline_core::reader_export`；方法位于 `Project`。原 selection API 保留，profile API 使用相同渲染与审计路径，不复制第二套解析/导出实现。

```rust
fn reader_profile_paths(&self) -> BTreeMap<String, PathBuf>;
fn reader_profiles(&self) -> Result<Vec<ReaderPublicationProfile>, String>;
fn create_reader_profile(&self, id: &str, selection: &ReaderExportSelection)
    -> Result<ReaderPublicationProfile, String>;
fn preview_reader_profile(&self, profile: &ReaderPublicationProfile)
    -> Result<ReaderExportPreview, String>;
fn build_reader_profile(&self, profile: &ReaderPublicationProfile, expected_plan_digest: &str)
    -> Result<BTreeMap<PathBuf, Vec<u8>>, String>;
fn export_reader_profile(&self, profile: &ReaderPublicationProfile,
    expected_plan_digest: &str, destination: &Path) -> Result<(), String>;
fn preview_save_reader_profile(&self, profile: &ReaderPublicationProfile)
    -> Result<ReaderProfileSavePlan, String>;
fn apply_save_reader_profile(&mut self, plan: &ReaderProfileSavePlan) -> Result<(), String>;
fn preview_reader_profile_migration(&self, profile: &ReaderPublicationProfile)
    -> Result<ReaderProfileMigrationPlan, String>;
fn apply_reader_profile_migration(&self, plan: &ReaderProfileMigrationPlan)
    -> Result<ReaderPublicationProfile, String>;
```

`ReaderProfileSavePlan={profile:ReaderPublicationProfile,content_baseline:String,document_path:String,document_before_hash:Option<String>,plan_digest:String}`。document_path 是工作区相对路径，None hash 与空文件不同。保存预览验证选择/路由，在 clone 上排演，应用重新检查磁盘基线并重算整个 plan，精确一致才替换内存 Project。它不自行磁盘保存，也不生成站点；UI 沿已有 Project 保存与历史流程处理。

`ReaderProfileMigrationPlan={before:ReaderPublicationProfile,after:ReaderPublicationProfile,authorization_changes:Vec<String>,content_baseline:String,plan_digest:String}`。应用重新计算并比较整个 plan，修改文案、候选、基线或路径均不能授权写入。

只读 ReaderExportPreview/ContentPreview/Included/Exclusion 和 progress 可 Serialize/Deserialize 供 worker 展示。profile、save/migration DTO 亦可传输，但反序列化本身不是授权；apply 必须重算候选，绝不接受任意来源文件字节。preview 的 content 继续只含 title/output_path/text/empty_content，v1 不增加 content；v2/v3 为空正文提供提示。

selection 的 `preview_reader_export` / `build_reader_export` / `export_reader_site` 和 profile 的 `preview_reader_profile` / `build_reader_profile` / `export_reader_profile` 均增加同名 `_with_progress` 版本。原参数不变，末参追加：

```rust
progress: &mut dyn FnMut(&ReaderExportProgress) -> bool
// ReaderExportProgress { phase: String, completed: usize, total: usize }
```

export 方法只在非 wasm 提供。非 progress 方法使用默认继续回调，输出与摘要必须相同。阶段包括 validate、compile、objects、chapters、maps、render、audit、write、publish；计数仅表示当前阶段，不伪造跨阶段完成百分比。callback false 返回以 `READER_CANCELLED` 开头的取消错误，不发布目标。

## 7. 原子性、预算与验证

preview 只在 clone 编译，不能刷新、保存或污染工程。计划摘要绑定 selection、Project content baseline、clone 编译实际消费的全部源码（包括尚未进入原 Project 缓冲的 include）、所选资源字节/显示名、实际路由与公共页面的标题/正文/索引/anchor。build 重算并要求 expected_plan_digest 一致；profile 保存与迁移也绑定其对应完整基线和候选。

native export 只允许工程外、尚不存在的新目标，父目录必须存在。使用同级唯一 staging，逐文件校验后写入；错误或取消清理暂存。最终以操作系统 no-replace 原子 rename 发布，目标即使在最后检查之后由他人创建也不能覆盖。Linux renameat2/RENAME_NOREPLACE、macOS renamex_np/RENAME_EXCL、Windows MoveFileW；不支持的平台安全拒绝，不用可能覆盖目标的 fallback。

| 预算 | 上限 |
| --- | --- |
| v3 对象 / v1-v2 对象 | 2000 / 500 |
| 书稿 / 总章节 | 100 / 5000 |
| 地图 / 每地图 legacy+scene primitives / raster | 100 / 5000 / 128 |
| 附件数 / 单附件 / 附件总量 | 128 / 16 MiB / 64 MiB |
| 每对象字段键 / 总字段键 | 256 / 5000 |
| 总输出文件 / 总包 | 10000 / 128 MiB |
| 站点与 profile 标题 | 1..160 字符 |

超限尽早失败；在对象、章节、地图及资源生成中持续检查字节预算，避免最后才发现大包。进度/取消检查覆盖验证、编译后、逐对象/章节、每 64 个地图 primitives、render/audit/write 与发布前；同步编译的真实取消延迟必须由性能门禁验证。

release 性能回归固定 2000 对象、合理中英正文/alias，五次 fresh fixture 同机每轮 preview+build ≤5 秒，打印每轮毫秒、文件数、bytes、机器信息与原工程 hash。取消请求到返回 ≤500ms，目标/暂存均无输出。profile create+preview_save 和实际 apply_save 分别独立测时；同步 UI 路径超过 250ms 必须转后台，不能只后台化规划却把同样耗时的完整重算留在主线程。应用仍须完整重新校验，不能以反序列化 token 或候选字节代替校验。不得用 debug 结果冒充 release。

行为回归覆盖 v1/v2 兼容、CANARY 全资源隔离、选属性反链边界、typed/timeline/局部图、静态条件隐藏、地图精确白名单及跨层顺序、章节次序、稳定路由/重构迁移、未知字段保留、失效项不裁剪、陈旧/篡改计划、取消、目的目录竞态与完整备份。真实 file:// 浏览器验收与静态资源自动审计分别记录；工具拒绝访问时不绕过限制，也不声称已验证浏览器。

## 8. 关系图标签与文字等价呈现（工具 0.16）

关系图继续只投影公开关系及其两端，不扩大选择。每条关系使用独立的三节点 SVG 行，
避免大量关系共享一张过高 viewBox 后被整图缩小。每个节点具有完整的转义 title/aria-label、
包内链接和明确裁切边界；可见标签按 Unicode grapheme 分为至多两行，有界摘要用省略号
提示，不把任意 UTF-8 字节截断成坏文本，也不拆开组合字符/emoji。摘要只负责图中扫描，
不等于删除公开内容。

每个图行下面同时提供来源对象、关系、目标对象的完整普通 HTML 文本链接；长中文、长
英文词、emoji和混合标点可换行，在窄屏与文本放大时仍可读。图中摘要、title和文字等价物
只使用同一已授权显示名，不输出源路径、未公开端点或作者备注。没有公开关系时不生成
空图。阅读站预览/搜索仍使用已有公共正文投影；重复图形标签不能变成额外私有数据入口。

此项是既有 v3 reader 呈现修整，不改变选择 DTO、稳定路由或语言版本。静态SVG渲染和
HTML闭包检查不能替代真实浏览器、键盘与读屏验收；实际覆盖须分别报告。
