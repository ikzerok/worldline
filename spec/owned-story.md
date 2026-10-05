# 拥有型试玩会话（工具 0.29）

`OwnedStory` 拥有一份稳定的 Program/Analysis，并在其生命期内保存原有 `Story<'p>`。
这是 runtime 的资源生命周期包装，不是新的解释器、存档或工作区。原借用型 Story API、
语言、fingerprint、choice身份、trace/schema/runtime兼容规则保持不变。

## 拥有权与访问

拥有者先进入稳定存储，再创建借用其AST和符号表的Story；销毁时先释放Story再释放
编译数据。构造错误、替换会话、移动包装器、换工程与关闭都必须正常释放资源。
不能使用 `Box::leak`、手写unsafe延长生命周期、伪造`'static`或让调用方修改拥有的AST。
安全自引用库封装在runtime内部，不把库的生成类型泄露成作品格式。

允许绑定本次借用的只读Story访问，以及受高阶生命周期约束的可变操作闭包/明确代理。
调用方不得取得`&mut Program`、`&mut Analysis`或能逃出拥有者生命期的Story。
每个会话只在显式开始时克隆一次准确的Program/Analysis对，不为渲染、选择或暂停
逐帧序列化/恢复checkpoint，不重新求值来模拟保留状态。

## 编辑器

PlayState 保存拥有型会话，重新开始显式替换旧会话；暂停、停止、运行错误仍保留当次
可查看状态，直至显式替换或关闭。源码更新的旧快照提示与草稿保护不变，不能隐式
应用输入或把已在运行的Story替换成最新稿。编辑器只消费runtime，不另写语言逻辑。

## 验收

固定seed下，拥有型与借用型Story的输出、选择、状态、save和trace逐次一致；覆盖
随机条件/标签、once、fragment、enter/exit/done、预算续行、错误和checkpoint。
验证构造成功/失败与频繁替换后的owner释放，移动包装器后仍安全；不把allocator
保留内存误报为存活owner。以同一1000对象原生负载复测首次+20+20重启的RSS斜率，
同时验证换工程与关闭。native与WASM构建分别检查，不声称单平台测量覆盖所有平台。
