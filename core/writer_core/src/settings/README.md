# Settings 模块

本模块管理应用配置与用户偏好偏好的强类型定义与本地保存。

## 核心职责

- **Settings Schema 看守**：严格遵循 `docs/settings_schema.md` 约定的强类型模型，提供字段合法性校验（防溢出、防负值、防无效字号）。
- **两级配置管理**：
  - **本地配置 (Local Settings)**：只存在于当前本地机器，不随云端同步（如 UI 窗口缩放、临时代理设置）。
  - **同步配置 (Syncable Settings)**：自动随云端 Git/REST API 同步（如作品一键排版规则偏好）。
- **JSON 安全落盘**：确保在物理存取时，任何写入都不会破坏原有 JSON 结构。

## 关联文件

- `mod.rs`：Settings 数据的存取管理与定义。
- `schema.rs`：描述同步配置及本地配置模型的强类型结构体。