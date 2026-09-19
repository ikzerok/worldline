# 需求—工作包—测试追踪

本表连接[65项需求](REQUIREMENTS.md)、[18个实施工作包](IMPLEMENTATION.md)及[测试计划](TEST_PLAN.md)。所有测试编号均为计划，不代表产品测试已通过；跨阶段质量项在后续工作包继续复测。机器版本见[requirements.json](requirements.json)中的work_packages与design_documents。

主文档：[worldline PRD](../PRD.md) / [worldline架构](../ARCHITECTURE.md) / [worldedit README](https://github.com/ikzerok/worldedit/blob/d0de7fabc44af6c6d69e3c742d1641303b232cfd/README.md)（该固定提交尚未包含 `docs/design/`；配对提交建立后再更新）。共同的字段定义归[契约](../../../spec/presentation.md)。

| 需求ID | 首次阶段 | 实现/复测工作包 | 计划测试ID | 设计依据 |
|---|---|---|---|---|
| BND-001 | M0 | WP-00, WP-01 | T-BND-001 | 共同契约及测试计划 |
| WL-001 | M1 | WP-03 | T-WL-001 | worldline主文档 |
| WL-002 | M1 | WP-02 | T-WL-002 | worldline主文档 |
| WL-003 | M1 | WP-02, WP-03 | T-WL-003 | worldline主文档 |
| WL-004 | M1 | WP-02, WP-06 | T-WL-004 | worldline主文档 |
| WL-005 | M1 | WP-02, WP-06 | T-WL-005 | worldline主文档 |
| WL-006 | M1 | WP-06 | T-WL-006 | worldline主文档 |
| WL-007 | M1 | WP-03 | T-WL-007 | worldline主文档 |
| WL-008 | M1 | WP-03, WP-11 | T-WL-008 | worldline主文档 |
| WL-009 | M1 | WP-06 | T-WL-009 | worldline主文档 |
| WL-010 | M1 | WP-03 | T-WL-010 | worldline主文档 |
| WL-011 | M1 | WP-03, WP-05 | T-WL-011 | worldline主文档 |
| WL-012 | M1 | WP-01, WP-02 | T-WL-012 | worldline主文档 |
| WL-013 | M2 | WP-09 | T-WL-013 | worldline主文档 |
| WL-014 | M2 | WP-09 | T-WL-014 | worldline主文档 |
| WL-015 | M2 | WP-09 | T-WL-015 | worldline主文档 |
| WL-016 | M2 | WP-09 | T-WL-016 | worldline主文档 |
| WL-017 | M2 | WP-08 | T-WL-017 | worldline主文档 |
| WL-018 | M3 | WP-12 | T-WL-018 | worldline主文档 |
| WL-019 | M3 | WP-12, WP-13 | T-WL-019 | worldline主文档 |
| WL-020 | M4 | WP-15 | T-WL-020 | worldline主文档 |
| WL-021 | M3 | WP-12 | T-WL-021 | worldline主文档 |
| WL-022 | M3 | WP-12 | T-WL-022 | worldline主文档 |
| WL-023 | M4 | WP-16 | T-WL-023 | worldline主文档 |
| WL-024 | M4 | WP-16 | T-WL-024 | worldline主文档 |
| WL-025 | M4 | WP-15 | T-WL-025 | worldline主文档 |
| WL-026 | M2 | WP-09 | T-WL-026 | worldline主文档 |
| WL-027 | M2 | WP-11 | T-WL-027 | worldline主文档 |
| WL-028 | M1 | WP-03, WP-06 | T-WL-028 | worldline主文档 |
| WL-029 | M1 | WP-03, WP-04 | T-WL-029 | worldline主文档 |
| WE-001 | M1 | WP-05 | T-WE-001 | worldedit主文档 |
| WE-002 | M1 | WP-04 | T-WE-002 | worldedit主文档 |
| WE-003 | M1 | WP-04 | T-WE-003 | worldedit主文档 |
| WE-004 | M1 | WP-05 | T-WE-004 | worldedit主文档 |
| WE-005 | M1 | WP-05 | T-WE-005 | worldedit主文档 |
| WE-006 | M1 | WP-05 | T-WE-006 | worldedit主文档 |
| WE-007 | M1 | WP-05 | T-WE-007 | worldedit主文档 |
| WE-008 | M1 | WP-05 | T-WE-008 | worldedit主文档 |
| WE-009 | M2 | WP-10 | T-WE-009 | worldedit主文档 |
| WE-010 | M2 | WP-10 | T-WE-010 | worldedit主文档 |
| WE-011 | M2 | WP-10 | T-WE-011 | worldedit主文档 |
| WE-012 | M2 | WP-10 | T-WE-012 | worldedit主文档 |
| WE-013 | M2 | WP-10 | T-WE-013 | worldedit主文档 |
| WE-014 | M1（网络入口随WE-010于M2补验） | WP-05, WP-10 | T-WE-014 | worldedit主文档 |
| WE-015 | M2 | WP-10 | T-WE-015 | worldedit主文档 |
| WE-016 | M3 | WP-12 | T-WE-016 | worldedit主文档 |
| WE-017 | M3 | WP-12 | T-WE-017 | worldedit主文档 |
| WE-018 | M1 | WP-04, WP-05 | T-WE-018 | worldedit主文档 |
| WE-019 | M4 | WP-14, WP-15 | T-WE-019 | worldedit主文档 |
| WE-020 | M4 | WP-16 | T-WE-020 | worldedit主文档 |
| WE-021 | M4 | WP-16 | T-WE-021 | worldedit主文档 |
| WE-022 | M1 | WP-05, WP-06 | T-WE-022 | worldedit主文档 |
| WE-023 | M1 | WP-06 | T-WE-023 | worldedit主文档 |
| WE-024 | M3 | WP-12 | T-WE-024 | worldedit主文档 |
| WE-025 | M2 | WP-11 | T-WE-025 | worldedit主文档 |
| WE-026 | M1 | WP-05 | T-WE-026 | worldedit主文档 |
| Q-001 | M0 | WP-00, WP-07, WP-17 | T-Q-001 | 共同契约及测试计划 |
| Q-002 | M1 | WP-05, WP-07, WP-10, WP-17 | T-Q-002 | 共同契约及测试计划 |
| Q-003 | M1 | WP-01, WP-03, WP-07 | T-Q-003 | 共同契约及测试计划 |
| Q-004 | M1 | WP-04, WP-06, WP-07 | T-Q-004 | 共同契约及测试计划 |
| Q-005 | M2 | WP-09, WP-10, WP-13 | T-Q-005 | 共同契约及测试计划 |
| Q-006 | M1 | WP-06, WP-07, WP-17 | T-Q-006 | 共同契约及测试计划 |
| Q-007 | M0 | WP-00, WP-17 | T-Q-007 | 共同契约及测试计划 |
| Q-008 | M3 | WP-13 | T-Q-008 | 共同契约及测试计划 |
| Q-009 | M4 | WP-16, WP-17 | T-Q-009 | 共同契约及测试计划 |

工作包不是GitHub PR编号；填写真实PR、执行平台和验收结果之后，才能改变需求实现状态。
