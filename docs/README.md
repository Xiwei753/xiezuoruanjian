# 项目文档

Status: active
Last verified: 2026-06-11
Truth source: product decision / code
Supersedes: docs/README.md (previous version)

本目录包含项目的各类技术文档和设计规范。

## 活动文档 (Active Documents)

| 文档名称 | 用途说明 |
|:---|:---|
| [TECHNICAL_ROUTE.md](TECHNICAL_ROUTE.md) | 全局技术路线与架构约束（唯一事实来源） |
| [CROSS_PLATFORM_CAPABILITY_CONTRACT.md](CROSS_PLATFORM_CAPABILITY_CONTRACT.md) | 跨平台能力契约与 Core-first 架构约束 |
| [API_CONTRACTS.md](API_CONTRACTS.md) | 接口边界与交互契约（合并 Bridge, Backend, QML 契约） |
| [PRODUCT_DESIGN.md](PRODUCT_DESIGN.md) | 产品设计与视觉契约（合并产品定位, 莫奈取色, 星图, 设置设计） |
| [workspace_format.md](workspace_format.md) | 工作区物理磁盘布局规范（权威定义，绝对禁止修改） |
| [settings_schema.md](settings_schema.md) | 应用设置与偏好 JSON Schema 规范 |
| [sync_rules.md](sync_rules.md) | 数据同步与多端冲突合并策略 |
| [starmap_semantics.md](starmap_semantics.md) | 星图语义模型与独立引用安全机制 |
| [editor_engine_route.md](editor_engine_route.md) | 自绘编辑器渲染引擎与统一编辑事件事务层路线 |
| [desktop_ime_notes.md](desktop_ime_notes.md) | Desktop (Linux) 输入法候选闪烁闪退避坑排错指南 |

## 使用说明

- 修改任何底层业务逻辑，必须优先在 Rust Core 中设计并通过 FFI 暴露，遵守 `CROSS_PLATFORM_CAPABILITY_CONTRACT.md`。
- 修改自研编辑器逻辑前，必须仔细查阅 `editor_engine_route.md` 与全局 `TECHNICAL_ROUTE.md`。
- `workspace_format.md` 描述的文件与目录结构为最高优先级规范，绝对不能为了前端 UI 展现进行妥协修改。