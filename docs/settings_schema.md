# 设置 Schema 定义

Status: active
Last verified: 2026-08-08
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

### `<app_data_root>/settings.local.json`（平台数据根，非 app-meta）
存储设备特定的配置，**不应**在设备间同步。
包括：
- `themeMode`（字符串，如 "system"、"dark"、"light"）[DEPRECATED — 使用 appearanceMode]
- `appearanceMode`（字符串，"system"|"light"|"dark"，每设备独立，默认 "system"）
- `colorSource`（字符串，"built_in"|"android_dynamic"|"saved_palette"，Android 本地颜色来源，默认 "built_in"）
- `dynamicColorEnabled`（布尔值，Android Dynamic Color 开关，默认 false）
- `selectedBuiltinThemeId`（字符串，选中的内置主题 ID，默认空）
- `selectedPaletteId`（字符串，Android 本地选中的调色板 ID，格式 `<device_id>:<fingerprint>`，默认空）
- `locale`（字符串）
- `editorFontSize`（浮点数）
- `editorLineSpacingMultiplier`（浮点数）
- `autoSaveEnabled`（布尔值）
- `autoSaveDelayMs`（整数）
- `editorTypingAnimationEnabled`（布尔值，本地编辑器文字动画开关）
- `editorSmoothCursorEnabled`（布尔值）
- `editorTypingAnimationDurationMs`（整数，毫秒，范围 30..1000，步长 10）
- `editorSmoothCursorDurationMs`（整数，毫秒，范围 30..1000，步长 10）
- `autoIndentEnabled`（布尔值，自动首行缩进开关，默认 true）
- `autoIndentWidth`（浮点数，首行缩进宽度字符数，默认 2.0）
- `editorCoordinatedTextCursorAnimationEnabled`（布尔值，协同光标动画开关，默认 false）
- `aiEnabled`（布尔值，AI 功能开关，默认 false）
- `statsDeviceId`（字符串，统计设备 ID，默认自动生成 UUID）
- `diagnosticsEnabled`（布尔值，本地诊断日志开关，默认 true，不进入同步、不含敏感数据）
- `diagnosticsVerbose`（布尔值，本地诊断详细模式，默认 true，不进入同步、不含敏感数据）

### `<app_data_root>/settings.sync.json`
存储**应该**在所有设备间同步的用户偏好。
包括：
- AI API 密钥（以明文保存，由用户设计明确接受）
- 编辑器偏好（`fontSize`、缩进样式）
- `themeMode`（字符串）[DEPRECATED — 使用 LocalSettings.appearanceMode]

Android Monet / 设备配色只属于当前 Android 设备，不写入 `settings.sync.json`。旧版本写下的
`monetColor`、`themePalette` / `themePaletteJson` 等字段会在首次读取或进入同步流程时被一次性
规范化并从磁盘移除，`fontSize` / `themeMode` 等仍有效的字段保留；其他设置键和未知字段
原样保留。规范化后的文件才是实际同步、计算哈希和写入 manifest 的内容。

### Android 本地设备配色：`<app_data_root>/local-theme-palettes/<device_id>/<fingerprint>.json`
仅 Android 使用，不进入 Git 同步、近场同步或任何跨端设置。每个文件是一份完整的
Material 3 主题快照，用于在 Android 本机切换已保存的设备配色。
包括：
- `schemaVersion`（整数，当前 1）
- `paletteId`（字符串，格式 `<device_id>:<fingerprint>`）
- `paletteFingerprint`（字符串，SHA-256 前 8 字节 hex）
- `source`（字符串，如 "android_dynamic_color"）
- `sourcePlatform`（字符串，固定为 "android"）
- `sourceDeviceId`（字符串，Android 本地设备 UUID；旧数据迁移为 "legacy"）
- `sourceDeviceClass`（字符串）
- `capturedAtMs`（整数，Unix 时间戳毫秒）
- `variant`（字符串，无法可靠识别时为 "system_selected"）
- `lightScheme` / `darkScheme`（ThemeColorScheme，完整 Material 3 语义角色）

旧版本的 `<app_data_root>/themes/palettes/` 只作为 Android 升级迁移输入读取一次；迁移本身
不删除、重写或上传旧文件。用户在 Android 删除某条已保存配色时，会同时删除新旧目录中的
对应副本，避免下次启动重新导入。新客户端及 Qt/HarmonyOS/Windows 均忽略该目录，
设置页也不提供设备配色或已保存设备配色入口。

### `<app_data_root>/app-meta/sync/config.local.json`
存储全局同步配置（Issue #630）。全应用只有一份全局 `SyncProfile`，不再区分应用级/作品级两套用户配置。包括：
- `enabled`（布尔值，是否启用同步）
- `backend_type`（字符串，当前为 `github_api`，`git` 为预留）
- `remote_url`（字符串）
- `transport`（字符串，`https_token` 或 `ssh_deploy_key`）
- `branch`（字符串，默认 `main`）
- `auto_sync`（布尔值）
- `sync_interval_seconds`（整数，0 表示仅手动同步）
- `username`（字符串）
- `has_network_permission`（布尔值）
- `has_network_state_permission`（布尔值）

### 全局同步凭据
存储敏感信息（如 GitHub 令牌、SSH 私钥）使用统一的全局安全存储条目（例如 `sync_token_global`），**绝不**进入同步、**绝不**按作品分散保存。作品目录下只保留该 target 的本地 `state.local.json / manifest.sync.json / conflicts.json`。
