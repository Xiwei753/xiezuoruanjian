# 设置 Schema 定义

Status: active
Last verified: 2026-07-31
Truth source: product decision / code
Supersedes: None

本文档概述了应用设置的 JSON schema。

### 设置页 Section 顺序（跨端一致）

1. 外观
2. 编辑器
3. 保存
4. 同步
5. AI
6. 诊断与日志
7. 关于/高级

### `app-meta/settings/settings.local.json`
存储设备特定的配置，**不应**在设备间同步。
包括：
- `themeMode`（字符串，如 "system"、"dark"、"light"）[DEPRECATED — 使用 appearanceMode]
- `appearanceMode`（字符串，"system"|"light"|"dark"，每设备独立，默认 "system"）
- `colorSource`（字符串，"built_in"|"android_dynamic"|"saved_palette"，颜色来源，默认 "built_in"）
- `dynamicColorEnabled`（布尔值，Android Dynamic Color 开关，默认 false）
- `selectedBuiltinThemeId`（字符串，选中的内置主题 ID，默认空）
- `selectedPaletteId`（字符串，选中的调色板 ID，格式 `<device_id>:<fingerprint>`，默认空）
- `locale`（字符串）
- `editorFontSize`（浮点数）
- `editorLineSpacingMultiplier`（浮点数）
- `autoSaveEnabled`（布尔值）
- `autoSaveDelayMs`（整数）
- `editorTypingAnimationEnabled`（布尔值，本地编辑器文字动画开关）
- `editorSmoothCursorEnabled`（布尔值）
- `editorTypingAnimationDurationMs`（整数，毫秒，范围 30..1000，步长 10）
- `editorSmoothCursorDurationMs`（整数，毫秒，范围 30..1000，步长 10）
- `windowWidth`、`windowHeight`（浮点数）
- `autoIndentEnabled`（布尔值，自动首行缩进开关，默认 true）
- `autoIndentWidth`（浮点数，首行缩进宽度字符数，默认 2.0）
- `editorCoordinatedTextCursorAnimationEnabled`（布尔值，协同光标动画开关，默认 false）
- `aiEnabled`（布尔值，AI 功能开关，默认 false）
- `statsDeviceId`（字符串，统计设备 ID，默认自动生成 UUID）
- `desktopSidebarWidth`（浮点数，Linux 和 Windows 端侧边栏宽度像素，默认 240.0；兼容读取 `linux_qt_sidebar_width`、`desktop_sidebar_width`、`linux_sidebar_width`）
- `desktopEditorWidth`（浮点数，Linux 和 Windows 端编辑器宽度像素，默认 0.0；兼容读取 `linux_qt_editor_width`、`desktop_editor_width`）
- `diagnosticsEnabled`（布尔值，本地诊断日志开关，默认 true，不进入同步、不含敏感数据）
- `diagnosticsVerbose`（布尔值，本地诊断详细模式，默认 true，不进入同步、不含敏感数据）

### `app-meta/settings/settings.sync.json`
存储**应该**在所有设备间同步的用户偏好。
包括：
- AI API 密钥（以明文保存，由用户设计明确接受）
- 编辑器偏好（`fontSize`、缩进样式）
- `themeMode`（字符串）[DEPRECATED — 使用 LocalSettings.appearanceMode]
- `monetColor`（字符串）[DEPRECATED — 使用调色板目录]
- `themePaletteJson`（字符串）[DEPRECATED — 使用调色板目录]

### `app-meta/themes/palettes/<device_id>/<fingerprint>.json`
不可变调色板记录目录。每个文件是一份完整的 Material 3 主题快照。
包括：
- `schemaVersion`（整数，当前 1）
- `paletteId`（字符串，格式 `<device_id>:<fingerprint>`）
- `paletteFingerprint`（字符串，SHA-256 前 8 字节 hex）
- `source`（字符串，如 "android_dynamic_color"）
- `sourcePlatform`（字符串）
- `sourceDeviceId`（字符串，Core 持久化设备 UUID；旧数据迁移为 "legacy"）
- `sourceDeviceClass`（字符串）
- `capturedAtMs`（整数，Unix 时间戳毫秒）
- `variant`（字符串，无法可靠识别时为 "system_selected"）
- `lightScheme` / `darkScheme`（ThemeColorScheme，完整 Material 3 语义角色）

### `app-meta/sync/sync_config.json`
存储同步配置。包括：
- `enabled`（布尔值，是否启用同步）
- `backend_type`（字符串，当前为 `github_api`，`git` 为预留）
- `remote_url`（字符串）
- `transport`（字符串，`https_token` 或 `ssh_deploy_key`）
- `branch`（字符串，默认 `main`）
- `auto_sync`（布尔值）
- `sync_interval_seconds`（整数，0 表示仅手动同步，否则最小有效值为 60）
- `username`（字符串）
- `has_network_permission`（布尔值）
- `has_network_state_permission`（布尔值）

### `app-meta/sync/sync_secrets.local.json`
存储敏感信息，如 GitHub 令牌、SSH 私钥。此文件保存在本地，**绝不**应被同步。
