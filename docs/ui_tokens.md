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
