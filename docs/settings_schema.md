# 设置 Schema 定义

Status: active
Last verified: 2026-06-11
Truth source: product decision / code
Supersedes: None

本文档概述了应用设置的 JSON schema。

### `app-meta/settings/settings.local.json`
存储设备特定的配置，**不应**在设备间同步。
包括：
- `themeMode`（字符串，如 "system"、"dark"、"light"）
- `locale`（字符串）
- `editorFontSize`（浮点数）
- `editorLineSpacingMultiplier`（浮点数）
- `autoSaveEnabled`（布尔值）
- `autoSaveDelayMs`（整数）
- `editorTypingAnimationEnabled`（布尔值，本地编辑器文字动画开关；Linux/Android 当前强制停用或仅记录事件占位，等待自绘编辑器）
- `editorSmoothCursorEnabled`（布尔值）
- `editorTypingAnimationDurationMs`（整数，毫秒）
- `editorSmoothCursorDurationMs`（整数，毫秒）
- `windowWidth`、`windowHeight`（浮点数）

### `app-meta/settings/settings.sync.json`
存储**应该**在所有设备间同步的用户偏好。
包括：
- AI API 密钥（以明文保存，由用户设计明确接受）
- 编辑器偏好（`fontSize`、`themeMode`、缩进样式）

### `app-meta/sync/sync_config.json`
存储同步配置。定义哪些文件/目录应该被同步。
包括：
- `remote_url`（字符串）
- `branch`（字符串）
- `auto_sync`（布尔值）
- `proxy`（对象）

### `app-meta/sync/sync_secrets.local.json`
存储敏感信息，如 GitHub 令牌、SSH 私钥。此文件保存在本地，**绝不**应被同步。
