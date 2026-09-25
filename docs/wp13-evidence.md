# WP-13 长文保真与性能证据

本记录对应 worldline#10。实现基线为 worldline `6dbc642` 与配对 worldedit `aea00af`，工具链 Rust 1.98.0，Windows x86_64 MSVC。

## 长文往返

`core/tests/long_content.rs` 构造 1,400 段中文正文，包含中文引号、反斜杠、换行、emoji、别名和未知自定义属性。测试先创建 history 实体，再把模板分类切换为 narrative，最后执行 Project::save / Project::open。

验收要求是正文、未知属性和别名在模板类型切换、保存和重开后保持原值；对象 ID 不改变。测试不依赖模板文件保存实例值，因此删除或升级模板不会删除正文。

## M2/M3 性能复测

当前配对提交重新运行 release-only 网络/资料测试：D5 数据 1,000 对象、3,000 关系，160 个暖态样本。网络帧 P95=11.481ms、最大=13.340ms；暖态资料切换 P95=4.355ms、最大=5.582ms，分别低于 33ms / 200ms 门槛。

既有 core D5 局部关系查询记录位于组合目录 `target/relations-profile-pagination.log`。最慢暖态筛选查询 P95=0.4475ms、最大=0.9859ms，低于 150ms 门槛；首次工程载入单独计时，没有混入暖态查询。

这些数字只代表本机 release 构建与记录的数据集，不外推到未实测平台；WASM 的静态检查/构建证据仍由配对检查单独记录。
