# worldline 共同契约样例

这些文件是共同契约的验证夹具，不是可运行工程，也不是 worldedit 0.2.0 已支持的输入。它们由设计验证器读取，不进入 worldline 的源码递归加载、运行时或产品资源。

这些JSON用于验证提议的持久格式和查询DTO，**不是已经能在worldedit 0.2.0打开的项目**。没有附带地图图片。content.index.json是测试桩，不能作为真实内容的第二真源。

map_harbor和map_overview展示同一tag对象被多处引用；project.json展示新清单；relation样例是M2查询DTO，语义关系仍应存.wl；graph_view只存布局不存边；templates.catalog包含16类原创模板。

language_1_10_proposal.wl.txt是未来语法示意，不能作为当前1.9源码直接运行。后缀刻意不是.wl，以免在旧工程被递归载入。

从组合目录运行`python docs/design/tools/validate_design.py`只检查设计文件，不启动任何世界过程，也不修改用户仓库。
