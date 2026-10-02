# 0.16 候选验证摘要

当前是开发候选，尚未公开发行。完整内容见 CHANGELOG，精确最终提交和平台门禁在最终配对阶段补齐；不能将早期draft CI当成发行验收。

本轮 Linux 已实际执行 core/CLI/agent/runtime 的源码生命周期、注册字段、资源/路径边界、取消/陈旧/篡改、保存重开和旧存档/检查点/entry trace复用测试；SVG样式、能力保护、数值/工作预算、导入/交换与公开白名单回归亦真实执行。独立静态审查与实际测试分开记录。

最终要求 workspace全量测试、严格Clippy、格式与600行限制、release构建、固定负载，以及对应editor普通/原型native/WASM配对检查。Windows由exact SHA的CI运行证明；本地Windows target cargo check仅证明可编译，不是Windows执行或GUI验收。

本地仅因磁盘容量关闭incremental与dev/test的DWARF符号，debug断言和优化层级不变，release配置不变。原始日志与截图保存在仓外；此文件只记录正式验证范围。

## Linux 候选实测

- 完整 workspace：986通过、0失败、3个显式忽略；忽略的SVG导入与2000对象阅读站基准另外以release执行
- 1000图元SVG，5个新夹具：预览加应用45.2–60.1ms，最大进度间隙31.2ms，取消响应0.22ms内
- 2000对象/2000别名阅读站，5个新夹具：预览加生成366–397ms，中位374ms；输出2018文件、5,250,107字节；取消30–34ms
- 上述为Linux x86_64、release、进程/OS缓存保持温热；不是冷启动或浏览器体验数字。创建加预览保存发布配置的同步路径340–373ms，超过250ms，仍如实保留此边界
- native严格Clippy、core WASM严格Clippy与Windows全workspace/all-targets交叉编译通过。Windows句柄锁定/junction回归需最终Windows CI实际执行，不以交叉编译替代
