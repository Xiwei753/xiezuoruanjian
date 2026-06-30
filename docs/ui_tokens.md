# UI Tokens — 跨端设计令牌契约

> **权威定义**：本文档是三端（Android / Desktop Qt / Harmony）UI token 的唯一事实来源。
> 修改 token 值时必须同步更新三端实现，禁止单端擅自变更。

---

## 1. 跨端 Token 命名契约

### 1.1 圆角（Radius）

| Token 名 | 值 | 说明 |
|----------|-----|------|
| `radiusXs` | 4dp | 小元素（状态点、小标签） |
| `radiusSm` | 8dp | 列表项、小按钮 |
| `radiusMd` | 12dp | 输入框、中等组件 |
| `radiusLg` | 16dp | 卡片、FAB |
| `radiusXl` | 28dp | 对话框、大面板 |
| `radiusPill` | 999dp | 全圆角（胶囊形） |

### 1.2 海拔（Elevation）

| Token 名 | 值 | 说明 |
|----------|-----|------|
| `elevation0` | 0dp | 无阴影 |
| `elevation1` | 1dp | 卡片静止 |
| `elevation2` | 3dp | FAB、底栏 |
| `elevation3` | 6dp | 抽屉、模态 |

### 1.3 Surface 层级

| Token 名 | 说明 |
|----------|------|
| `surfaceContainerLowest` | 最底层 surface（最亮/最暗） |
| `surfaceContainerLow` | 低层级 surface |
| `surfaceContainer` | 默认 surface 容器 |
| `surfaceContainerHigh` | 高层级 surface |
| `surfaceContainerHighest` | 最高层 surface |
| `surfaceDim` | 暗淡 surface |
| `surfaceBright` | 明亮 surface |

### 1.4 组件 Shape Token

| Token 名 | 值 | 说明 |
|----------|-----|------|
| `cardRadius` | radiusLg (16dp) | 卡片圆角 |
| `dialogRadius` | radiusXl (28dp) | 对话框圆角 |
| `fabRadius` | radiusLg (16dp) | FAB 圆角 |
| `bottomBarRadius` | 0dp | 底栏圆角 |
| `inputFieldRadius` | radiusMd (12dp) | 输入框圆角 |

---

## 2. 各端实现位置

| 端 | 文件 | 格式 |
|----|------|------|
| Android | `res/values/shape_appearance.xml` + `res/values/colors.xml` + `res/values/themes.xml` + `res/values/styles.xml` | XML |
| Desktop Qt | `qml/DesignTokens.qml` + `qml/AppShadow.qml` | QML |
| Harmony | `ets/system/DesignTokens.ets` + `ets/system/HarmonyThemeAdapter.ets` + `ets/system/ThemePalette.ets` | ArkTS |

---

## 3. 莫奈取色路线

### 3.1 废弃：monetColor

`monetColor` 是旧的单色同步字段，仅同步 `system_accent1_500` 的 hex 值。
**已废弃，禁止扩展。** 保留兼容读取，新功能必须使用 `theme_palette`。

### 3.2 正式：theme_palette

`theme_palette` 是跨端同步的完整调色板，由 Android 12+ 的 Dynamic Color 产出。

**数据结构：**
- `source`: `"android_dynamic_color"` — 标识来源
- `updated_at_ms`: 更新时间戳
- `device_id`: 产出设备 ID
- `variant`: 变体名（如 `"tonal_spot"`）
- `light_*`: 亮色调色板（primary, onPrimary, primaryContainer, ...）
- `dark_*`: 暗色调色板

**规则：**
- **只有 Android 端产出** `theme_palette`，其他端只消费。
- 非 Android 端不读取壁纸色，不伪装莫奈。
- 只有同步仓库存在 `theme_palette.source = "android_dynamic_color"` 时，设置里才显示"使用 Android 同步主题色"开关。
- 没有数据就不显示开关。
- Desktop 默认走 `SystemPalette` + 素笺纸面。
- Harmony 默认走 HDS / 沉浸光感。
- **正文纸面只用 neutral/surface，不直接吃高饱和 primary。**

### 3.3 跨端复用规则

跨端复用的是**颜色 token**，不是系统效果 cosplay：
- Android 的 Dynamic Color 效果（ripple、shape morph）不移植到其他端。
- Desktop 不实现壁纸取色。
- Harmony 不实现壁纸取色。
- 其他端只消费同步来的语义色值（primary, surface, outline 等）。

---

## 4. 禁止事项

| 禁止行为 | 原因 |
|---------|------|
| 在 QML 中硬编码 `radius: <数字>` | 必须使用 DesignTokens |
| 在 QML 中硬编码 `color: "#xxxxxx"` | 必须使用 DesignTokens 语义色 |
| 在 Android XML 中硬编码 FAB 底部避让 | 必须使用 FabPlacementHelper |
| 继续扩展 `monetColor` | 已废弃，使用 theme_palette |
| 非 Android 端读取壁纸色 | 违反跨端复用规则 |
| 在正文区域使用玻璃/模糊/强阴影 | 破坏纸面稳定性 |

---

## 5. Issue #360 说明

Android 负责产出 Dynamic Color，其他端只消费同步 palette，不伪装莫奈。
跨端复用的是颜色 token，不是系统效果 cosplay。

---

## 6. 静态检查

运行 `scripts/check_ui_tokens.sh` 验证：
- QML 中无新增硬编码圆角/阴影颜色
- Android 中无新增硬编码 FAB 底部避让
- monetColor 不再扩展

---

## 7. theme_palette 命名契约

### 7.1 两层 JSON 不是同一个

`theme_palette` 在系统中存在两层 JSON 序列化，**键名格式不同，不可混用**：

| 层级 | 数据结构 | `serde` 属性 | JSON 键名格式 | 用途 |
|------|---------|-------------|-------------|------|
| **SyncableSettings 持久化层** | `SyncableSettings`（内嵌 `ThemePalette`） | `#[serde(rename_all = "camelCase")]` | camelCase（如 `lightSurfaceContainer`） | 写入 `settings.sync.json` 磁盘文件 |
| **跨端 themePaletteJson 层** | `ThemePaletteDto` | 无 `rename_all` | snake_case（如 `light_surface_container`） | Android→Core 同步通道，`theme_palette_json` 字段传递 |

**关键区别：**
- `SyncableSettings` 和嵌套的 `ThemePalette` 都有 `#[serde(rename_all = "camelCase")]`，所以磁盘上的 JSON 键名是 camelCase。
- `ThemePaletteDto` 没有 `rename_all`，所以 JSON 键名是 snake_case。Android `ThemePaletteHelper` 输出 snake_case JSON，Kotlin 端通过 `theme_palette_json` 字段传递。

### 7.2 字段命名对照表（新增 surfaceContainer 字段）

以下列出新增的 surfaceContainer 系列字段的命名对照：

| Rust 字段 | 持久化 camelCase（settings.sync.json） | 跨端 snake_case（themePaletteJson） |
|-----------|--------------------------------------|-----------------------------------|
| `light_surface_container_lowest` | `lightSurfaceContainerLowest` | `light_surface_container_lowest` |
| `light_surface_container_low` | `lightSurfaceContainerLow` | `light_surface_container_low` |
| `light_surface_container` | `lightSurfaceContainer` | `light_surface_container` |
| `light_surface_container_high` | `lightSurfaceContainerHigh` | `light_surface_container_high` |
| `light_surface_container_highest` | `lightSurfaceContainerHighest` | `light_surface_container_highest` |
| `dark_surface_container_lowest` | `darkSurfaceContainerLowest` | `dark_surface_container_lowest` |
| `dark_surface_container_low` | `darkSurfaceContainerLow` | `dark_surface_container_low` |
| `dark_surface_container` | `darkSurfaceContainer` | `dark_surface_container` |
| `dark_surface_container_high` | `darkSurfaceContainerHigh` | `dark_surface_container_high` |
| `dark_surface_container_highest` | `darkSurfaceContainerHighest` | `dark_surface_container_highest` |

### 7.3 数据流路径

```
Android 产出
  → ThemePaletteHelper.extractThemePaletteJson()
  → snake_case JSON 字符串
  → Core ThemePaletteDto 反序列化
  → Core ThemePalette（camelCase 持久化到 settings.sync.json 磁盘）

Core 磁盘读取
  → SyncableSettings（camelCase JSON 反序列化）
  → ThemePaletteDto（snake_case 序列化）
  → 跨端同步（theme_palette_json 字段）
```

**详细流程：**
1. Android 端通过 `ThemePaletteHelper.extractThemePaletteJson()` 产出 snake_case JSON 字符串。
2. Kotlin 端将此 JSON 字符串放入 `SyncableSettingsDto.theme_palette_json` 字段，传给 Core。
3. Core 的 `SyncableSettingsDto → SyncableSettings` 转换中，先从 `theme_palette_json` 解析出 `ThemePaletteDto`（snake_case），再转为 `ThemePalette`（camelCase）。
4. `ThemePalette` 随 `SyncableSettings` 以 camelCase 序列化写入 `settings.sync.json`。
5. 反向同步时，Core 读取 `settings.sync.json`（camelCase），转为 `ThemePaletteDto`（snake_case），序列化为 `theme_palette_json` 传给各端。

### 7.4 旧数据兼容

所有 `ThemePalette` 和 `ThemePaletteDto` 的字段都标注了 `#[serde(default)]`：
- 旧数据缺少新增字段时，反序列化不会报错。
- 缺失字段 fallback 为对应类型的默认值（`String` 为空字符串 `""`，`i64` 为 `0`）。
- 不需要数据迁移脚本。

### 7.5 非 Android 端只消费不同步

- **只有 Android 端产出** `theme_palette`，其他端只消费。
- 非 Android 端不读取壁纸色，不伪装莫奈。
- Desktop 默认走 `SystemPalette` + 素笺纸面。
- Harmony 默认走 HDS / 沉浸光感。
- 只有同步仓库存在 `theme_palette.source = "android_dynamic_color"` 时，设置里才显示"使用 Android 同步主题色"开关。
