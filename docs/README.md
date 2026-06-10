# 项目文档

本目录包含项目的各类技术文档和设计规范。

## 主要文件

| 文件 | 用途 |
|------|------|
| `architecture.md` | 项目架构说明 |
| `core_api.md` | Rust 核心库 API 文档 |
| `settings_schema.md` | 设置项 Schema 定义 |
| `settings_design.md` | 设置功能设计文档 |
| `workspace_format.md` | 工作区格式规范（权威定义） |
| `sync_rules.md` | 同步规则说明 |
| `ai_development_guide.md` | AI 开发指南 |
| `ai_tool_calling.md` | AI 工具调用说明 |
| `input_animation_design.md` | 输入动画设计 |
| `desktop_ime_notes.md` | Desktop (Linux) 输入法注意事项 |
| `CAPABILITY_MATRIX.md` | 功能矩阵 |
| `CROSS_PLATFORM_CAPABILITY_CONTRACT.md` | 跨平台能力契约 |
| `TECHNICAL_ROUTE.md` | 技术路线图 |

## 使用说明

- 开发前请阅读 `architecture.md` 了解项目结构
- 修改核心库后需同步更新 `core_api.md`
- 新增设置项需更新 `settings_schema.md`
- `workspace_format.md` 是工作区格式的权威定义，不可随意修改