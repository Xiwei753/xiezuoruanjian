# Screen Policy 验收表

> 本文档是三端 UI 布局的唯一验收标准。
> 三端实现时只允许对照本表，不允许自己发明位置。
> 与 Core `screen_policy.rs` 的 `resolve_screen_policy` 输出同步更新。

## 1. Home 页面

### 1.1 Home + SinglePane

| ActionSlot | ActionRole | ActionPlacement | Android 控件 | Harmony 控件 | Desktop 控件 |
|-----------|-----------|----------------|-------------|-------------|-------------|
| settings | Settings | TopTrailing | IconButton (TopAppBar) | Button (icon) | AppButton (icon-only, ghost) |
| search | Search | TopTrailing | IconButton (TopAppBar) | Button (icon) | AppButton (icon-only, ghost) |

### 1.2 Home + SupportingPane

| ActionSlot | ActionRole | ActionPlacement | Android 控件 | Harmony 控件 | Desktop 控件 |
|-----------|-----------|----------------|-------------|-------------|-------------|
| settings | Settings | TopTrailing | IconButton (TopAppBar) | Button (icon) | AppButton (icon-only, ghost) |
| search | Search | TopTrailing | IconButton (TopAppBar) | Button (icon) | AppButton (icon-only, ghost) |

### 1.3 Home + TwoPane

| ActionSlot | ActionRole | ActionPlacement | Android 控件 | Harmony 控件 | Desktop 控件 |
|-----------|-----------|----------------|-------------|-------------|-------------|
| settings | Settings | TopTrailing | IconButton (TopAppBar) | Button (icon) | AppButton (icon-only, ghost) |
| search | Search | TopTrailing | IconButton (TopAppBar) | Button (icon) | AppButton (icon-only, ghost) |

---

## 2. Workspace 页面

### 2.1 Workspace + SinglePane

| ActionSlot | ActionRole | ActionPlacement | requires_confirmation | Android 控件 | Harmony 控件 | Desktop 控件 |
|-----------|-----------|----------------|----------------------|-------------|-------------|-------------|
| create_project | CreateProject | Floating | false | FloatingActionButton | Button (prominent, large) | AppButton (primary) |
| delete | Delete | ContextMenu | true | ContextMenuItem → AlertDialog | Menu item → AlertDialog | AppButton (danger) + 确认弹窗 |
| rename | Rename | ContextMenu | false | ContextMenuItem | Menu item | AppButton (ghost) |

### 2.2 Workspace + SupportingPane

| ActionSlot | ActionRole | ActionPlacement | requires_confirmation | Android 控件 | Harmony 控件 | Desktop 控件 |
|-----------|-----------|----------------|----------------------|-------------|-------------|-------------|
| create_project | CreateProject | TopTrailing | false | TextButton (TopAppBar) | Button (prominent) | AppButton (primary) |
| delete | Delete | ContextMenu | true | ContextMenuItem → AlertDialog | Menu item → AlertDialog | AppButton (danger) + 确认弹窗 |
| rename | Rename | ContextMenu | false | ContextMenuItem | Menu item | AppButton (ghost) |

### 2.3 Workspace + TwoPane

| ActionSlot | ActionRole | ActionPlacement | requires_confirmation | Android 控件 | Harmony 控件 | Desktop 控件 |
|-----------|-----------|----------------|----------------------|-------------|-------------|-------------|
| create_project | CreateProject | TopTrailing | false | TextButton (TopAppBar) | Button (prominent) | AppButton (primary) |
| delete | Delete | ContextMenu | true | ContextMenuItem → AlertDialog | Menu item → AlertDialog | AppButton (danger) + 确认弹窗 |
| rename | Rename | ContextMenu | false | ContextMenuItem | Menu item | AppButton (ghost) |

---

## 3. Writing 页面

### 3.1 Writing + SinglePane

| ActionSlot | ActionRole | ActionPlacement | Android 控件 | Harmony 控件 | Desktop 控件 |
|-----------|-----------|----------------|-------------|-------------|-------------|
| back | Back | TopLeading | IconButton (NavigationIcon) | Button (navigation) | AppButton (icon-only, ghost) |
| save | Save | TopTrailing | TextButton (TopAppBar) | Button (prominent) | AppButton (primary) |

### 3.2 Writing + SupportingPane

| ActionSlot | ActionRole | ActionPlacement | Android 控件 | Harmony 控件 | Desktop 控件 |
|-----------|-----------|----------------|-------------|-------------|-------------|
| back | Back | TopLeading | IconButton (NavigationIcon) | Button (navigation) | AppButton (icon-only, ghost) |
| save | Save | TopTrailing | TextButton (TopAppBar) | Button (prominent) | AppButton (primary) |

### 3.3 Writing + TwoPane

| ActionSlot | ActionRole | ActionPlacement | Android 控件 | Harmony 控件 | Desktop 控件 |
|-----------|-----------|----------------|-------------|-------------|-------------|
| back | Back | TopLeading | IconButton (NavigationIcon) | Button (navigation) | AppButton (icon-only, ghost) |
| save | Save | TopTrailing | TextButton (TopAppBar) | Button (prominent) | AppButton (primary) |

---

## 4. Settings 页面

### 4.1 Settings + SinglePane

| ActionSlot | ActionRole | ActionPlacement | Android 控件 | Harmony 控件 | Desktop 控件 |
|-----------|-----------|----------------|-------------|-------------|-------------|
| back | Back | TopLeading | IconButton (NavigationIcon) | Button (navigation) | AppButton (icon-only, ghost) |

### 4.2 Settings + SupportingPane

| ActionSlot | ActionRole | ActionPlacement | Android 控件 | Harmony 控件 | Desktop 控件 |
|-----------|-----------|----------------|-------------|-------------|-------------|
| back | Back | TopLeading | IconButton (NavigationIcon) | Button (navigation) | AppButton (icon-only, ghost) |

### 4.3 Settings + TwoPane

| ActionSlot | ActionRole | ActionPlacement | Android 控件 | Harmony 控件 | Desktop 控件 |
|-----------|-----------|----------------|-------------|-------------|-------------|
| back | Back | TopLeading | IconButton (NavigationIcon) | Button (navigation) | AppButton (icon-only, ghost) |

---

## 5. Sync 页面

### 5.1 Sync + SinglePane

| ActionSlot | ActionRole | ActionPlacement | Android 控件 | Harmony 控件 | Desktop 控件 |
|-----------|-----------|----------------|-------------|-------------|-------------|
| back | Back | TopLeading | IconButton (NavigationIcon) | Button (navigation) | AppButton (icon-only, ghost) |
| sync | Sync | Floating | FloatingActionButton (extended) | Button (prominent) | AppButton (primary) |

### 5.2 Sync + SupportingPane

| ActionSlot | ActionRole | ActionPlacement | Android 控件 | Harmony 控件 | Desktop 控件 |
|-----------|-----------|----------------|-------------|-------------|-------------|
| back | Back | TopLeading | IconButton (NavigationIcon) | Button (navigation) | AppButton (icon-only, ghost) |
| sync | Sync | Floating | FloatingActionButton (extended) | Button (prominent) | AppButton (primary) |

### 5.3 Sync + TwoPane

| ActionSlot | ActionRole | ActionPlacement | Android 控件 | Harmony 控件 | Desktop 控件 |
|-----------|-----------|----------------|-------------|-------------|-------------|
| back | Back | TopLeading | IconButton (NavigationIcon) | Button (navigation) | AppButton (icon-only, ghost) |
| sync | Sync | Floating | FloatingActionButton (extended) | Button (prominent) | AppButton (primary) |

---

## 6. LayoutPlan 新增字段验收

| ShellMode | side_panel_width_vp | primary_pane_weight | detail_panel_max_width_vp |
|-----------|--------------------|--------------------|--------------------------|
| SinglePane | 0.0 | 1.0 | 0.0 |
| SupportingPane | 0.0 | 1.0 | 0.0 |
| TwoPane | 0.0 | 2.0 | 960.0 |

---

## 7. 硬编码清理验收

| 平台 | 清理项 | 验收标准 |
|------|-------|---------|
| Android | 2f/3f 权重 | 使用 LayoutPlan.primaryPaneWeight，不包含硬编码 2f/3f |
| Android | 56dp margin | 使用 bottomNav.measuredHeight，不包含硬编码 56 |
| Android | WindowInsets TODO | safeTopVp/safeBottomVp/keyboardVisible 从 WindowInsets API 获取 |
| Harmony | system 主题 | 支持 Light/Dark/System 三种模式，无 TODO |
| Desktop | DesignTokens 注释 | 不包含 "aligned with Android" 或 "colors.xml" |

---

## 8. StyleAdapter 降级策略

| 未知枚举 | 降级行为 |
|---------|---------|
| 未知 ActionRole | Android: 默认 TextButton / Harmony: 默认 Button / Desktop: 默认 AppButton (ghost) |
| 未知 ActionPlacement | 三端均降级为 TopTrailing |
| 未知 PaneRole | 三端均降级为右侧面板容器 |