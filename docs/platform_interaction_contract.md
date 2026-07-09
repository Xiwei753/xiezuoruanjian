# 平台系统交互契约

Status: active
Last verified: 2026-07-09
Truth source: code (core/writer_core/src/platform_interaction/)

## 目的

定义平台系统交互的唯一边界：输入法、光标锚点、剪贴板、焦点、动画驱动、平台能力、同步网络能力分别归谁管。各端（Linux Qt / Windows / Android / Harmony）必须遵守此契约，不允许绕过 Core 自己猜能力或建业务状态机。

## 核心原则

1. **`core/writer_core/src/platform_interaction/` 是唯一平台系统交互契约定义处。**
2. **各端启动时必须上报真实能力，不允许吹牛。** 未接入就 false/Unavailable。
3. **设置页、编辑器开关、同步按钮、动画开关只读 capabilities，不再各端自己猜。**
4. **平台端只做展示和系统协议翻译，不写正文编辑逻辑。**

## 能力声明

### Rust Core 定义

文件：`core/writer_core/src/platform_interaction/capabilities.rs`

```rust
pub struct PlatformCapabilities {
    pub supports_ime_preedit: bool,
    pub supports_cursor_anchor: bool,
    pub supports_replacement_commit: bool,
    pub supports_text_animation: bool,
    pub supports_smooth_cursor: bool,
    pub supports_reflow_animation: bool,
    pub supports_clipboard: bool,
    pub supports_context_menu: bool,
}
```

### 各端当前真实能力

| 能力 | Linux Qt | Windows | Android | Harmony |
|------|----------|---------|---------|---------|
| IME preedit | ✓ | ✓ | ✓ | ✗ |
| cursor anchor | ✗ (空桩) | ✓ | ✓ | ✗ |
| replacement commit | ✓ | ✗ | ✗ | ✗ |
| text animation | ✓ | ✓ | ✓ | ✗ |
| smooth cursor | ✓ | ✓ | ✓ | ✗ |
| reflow animation | ✓ | ✓ | ✓ | ✗ |
| clipboard | ✓ | ✓ | ✓ | ✓ |
| context menu | ✓ | ✓ | ✓ | ✓ |

### 工厂方法

| 平台 | 工厂方法 | 说明 |
|------|---------|------|
| Linux Qt | `PlatformCapabilities::linux_qt()` | cursor_anchor 为空桩 |
| Android | `PlatformCapabilities::android()` | replacement_commit 未实现 |
| Windows | `PlatformCapabilities::windows()` | replacement_commit 未实现 |
| Harmony | `PlatformCapabilities::harmony()` | 仅 clipboard + context_menu |

## 输入法

### 归谁管

| 层 | 职责 | 文件 |
|----|------|------|
| Core | `NormalizedTextInputEvent` 定义、`TextInputAdapter` trait | `core/.../text_input.rs` |
| Linux Qt | Qt → NormalizedTextInputEvent 转换、QInputMethodEvent 协议翻译 | `apps/Linux_qt/src/platform/linux_qt/text_input_adapter.rs` |
| Windows | CoreTextEditContext → NormalizedTextInputEvent 转换 | `apps/windows/Platform/IPlatformAdapters.cs` |
| Android | InputConnection → NormalizedTextInputEvent 转换 | `apps/android/.../platform/TextInputAdapter.kt` |

### Linux Qt 三层架构

```
Layer 1: QtInputSurface (C++ SujianEventFilter)
  - Qt 官方事件入口：keyPressEvent / inputMethodEvent / query
  - 不写正文业务，不写动画逻辑
  - 委托给 PlatformImeAdapter 处理 fcitx5/ibus 语义

Layer 2: Linux PlatformImeAdapter (C++ 内嵌)
  - LinuxImeAdapter: 直接插入，不延迟，按 Qt inputMethodEvent 语义

Layer 3: EditorInputController (Rust)
  - 接收归一化输入事件：PlainText / Shortcut / Preedit / Commit 等
  - 调用 SujianEditorItem / EditorEngine 修改正文和生成视觉事务
  - Linux IME 语义只存在 Layer 2，正文编辑和动画不关心具体输入法
```

### 事件流

```
Qt Event → SujianEventFilter (C++) → FFI extern "C" → EditorInputController (Rust)
  → EditorInputHost trait → SujianEditorItem → EditorBuffer + VisualTransaction
```

## 光标锚点

### 归谁管

| 层 | 职责 | 文件 |
|----|------|------|
| Core | `CursorAnchorRequest` / `CursorAnchorAdapter` trait 定义 | `core/.../cursor_anchor.rs` |
| Linux Qt | QInputMethod::update 触发、IME query 响应 | `apps/Linux_qt/src/platform/linux_qt/cursor_anchor_adapter.rs` (空桩) |
| Windows | CoreTextEditContext candidate window anchoring | `apps/windows/Platform/IPlatformAdapters.cs` |
| Android | CursorAnchorInfo → EditorView | `apps/android/.../platform/CursorAnchorAdapter.kt` |

### Linux Qt 当前状态

- `LinuxQtCursorAnchorAdapter` 为空桩
- IME query 在 `qt_surface.rs` C++ 侧直接读 QML property
- IME cursor update 在 `ime_visual.rs` 通过 cpp! 宏直接调用 `QInputMethod::update()`
- **迁移目标**：IME query 所需数据由 CursorAnchorAdapter 提供，C++ 只做协议翻译

## 剪贴板与焦点

### 归谁管

| 层 | 职责 | 文件 |
|----|------|------|
| Core | `ClipboardRequest` / `ClipboardResult` / `FocusRequest` / `FocusState` 定义 | `core/.../clipboard_focus.rs` |
| Linux Qt | QClipboard / forceActiveFocus / QMenu | `apps/Linux_qt/src/platform/linux_qt/clipboard_focus_adapter.rs` (空桩) |
| Windows | WinRT Clipboard / Focus | `apps/windows/Platform/IPlatformAdapters.cs` |
| Android | ClipboardManager / InputMethodManager | `apps/android/.../platform/ClipboardFocusAdapter.kt` |

### Linux Qt 当前状态

- `LinuxQtClipboardFocusAdapter` 所有操作返回 Unavailable
- 实际剪贴板在 `SujianEditorItem::clipboard_copy/paste()` 通过 cpp! 宏直接调用 QClipboard
- 实际焦点在 `input::focus_item()` 通过 cpp! 宏直接调用 forceActiveFocus
- **迁移目标**：cpp! 剪贴板/焦点调用收敛到适配器

## 动画驱动

### 归谁管

| 层 | 职责 | 文件 |
|----|------|------|
| Core | `AnimationDriveRequest` / `AnimationDriver` trait / `AnimationSuppressReason` 定义 | `core/.../animation_driver.rs` |
| Core | `EditorVisualTransaction` / `AnimationMode` / `EditorEngine` | `core/.../transaction.rs` |
| Linux Qt | QML AnimationTimer + EditorAnimationOverlay | `apps/Linux_qt/src/platform/linux_qt/animation_driver_adapter.rs` |
| Windows | SujianAnimationController + SujianAnimationOverlay | `apps/windows/Editor/Animation/` |
| Android | SujianEditorView animation layer | `apps/android/.../platform/AnimationDriver.kt` |

### Linux Qt 当前状态

- 抑制/恢复状态管理已真实接入
- drive_animation / cancel / finish 为空桩
- 实际动画由 QML AnimationTimer + EditorAnimationOverlay 驱动
- **迁移目标**：SujianEditorItem 通过 AnimationDriver 驱动动画

## 同步网络能力

### 归谁管

| 层 | 职责 | 文件 |
|----|------|------|
| Core | `SyncCapabilityDto` / `get_sync_capability()` | `core/.../api/sync_api.rs` |
| Core | `SyncStatus` / `SyncService` / `SyncBackend` | `core/.../sync/` |
| Linux Qt | QML sync button 灰态由 Core capability 决定 | `apps/Linux_qt/src/backend/sync_backend.rs` |
| Windows | 同步页 UI | `apps/windows/Pages/SyncPage.xaml.cs` |
| Android | 同步页 UI | `apps/android/.../` |

### 同步按钮灰态单一来源

- `sync_can_run` 和 `sync_block_reason` 由 Core `get_sync_capability()` 决定
- 阻塞检查链：workspace → enabled → remote_url → token
- 平台端只做展示和 token 输入，不自己判断"能不能同步"
- 错误使用 Core 返回的 `block_message_key` / i18n message key

### 同步后端路线

- **唯一正式路线**：`BackendType::GithubApi`（GitHub REST API + Token）
- **Legacy**：`BackendType::Git`（libgit2），不再新增功能
- 参见 `docs/sync_rules.md`

## 错误提示

### 归谁管

| 层 | 职责 | 文件 |
|----|------|------|
| Core | `WriterError` → `message_key` + `params` | `core/.../api/error.rs` |
| Core | `ResultEnvelope<T>` → `messageKey` + `messageArgs` | `core/.../api/envelope.rs` |
| Linux Qt | `MessageKeyMapper` 白名单验证 + QML qsTr | `apps/Linux_qt/src/backend/message_key_mapper.rs` |
| Windows | `EnvelopeResult` (简化版，缺少 messageKey) | `apps/windows/Bridge/WriterCoreBridge.cs` |
| Android | UniFFI typed error | `apps/android/.../` |

### Windows 待收口

- Windows `EnvelopeResult` 只有 `userMessage`，缺少 `messageKey` / `messageArgs`
- **迁移目标**：Windows EnvelopeResult 对齐 Core `ResultEnvelope` schema

## 迁移清单

### 已完成（第一阶段）

- [x] Core `PlatformCapabilities` 工厂方法更新为各端真实能力
- [x] Linux Qt `LinuxQtCapabilitiesAdapter` 使用 Core 工厂方法
- [x] 各端适配器添加 TODO 注释标记迁移路径
- [x] 同步 capability 已由 Core `get_sync_capability()` 单一来源
- [x] 错误提示已由 Core `message_key` + `MessageKeyMapper` 单一来源

### 待迁移（大迁移，无法一次完成）

- [ ] Linux Qt IME query 从 QML property 迁移到 CursorAnchorAdapter 数据源
- [ ] Linux Qt IME cursor update 从 cpp! 宏迁移到 CursorAnchorAdapter
- [ ] Linux Qt 剪贴板从 cpp! 宏迁移到 ClipboardAndFocusAdapter
- [ ] Linux Qt 焦点从 cpp! 宏迁移到 ClipboardAndFocusAdapter
- [ ] Linux Qt 动画驱动从 QML signal 迁移到 AnimationDriver
- [ ] Windows SujianEditor 正文变更走 Core EditorEngine / EditorTransaction
- [ ] Windows EnvelopeResult 对齐 Core ResultEnvelope schema (messageKey/messageArgs)
- [ ] Windows 桥接从裸 JSON envelope 收口到 typed DTO / typed error
- [ ] Android PlatformAdapterRegistry.initialize() 接入启动流程
- [ ] Android 输入事件走 NormalizedTextInputEvent 归一化
- [ ] Harmony 平台适配层补全
