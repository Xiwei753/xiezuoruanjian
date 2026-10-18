# HarmonyOS 平台能力矩阵

本文件记录素笺 HarmonyOS 客户端实际使用的每个系统能力：Kit、接口、最低 API、SystemCapability、权限、ACL 需求、fallback 策略和实现文件。新增系统能力前先查 HarmonyOS 官方 API 和本机 SDK d.ts（`/opt/devecostudio/sdk/default`），再在此登记。

实现文件路径相对于本文件所在目录 `entry/src/main/ets/platform/`（即下表与正文中的路径不以 `platform/` 开头，直接从子目录名写起）。唯一例外是 DLP 防窥：其实现已迁出 entry，放在独立受限模块 `privacy_acl` 中；DLP 路径相对于 `apps/harmony/` 工程目录，写作 `privacy_acl/src/main/ets/impl/apiXX/...`。

## 应用最低安装基线与版本分流

- **最低安装基线**：`compatibleSdkVersion = 12`（HarmonyOS NEXT 5.0.0(12)）。这是应用能安装运行的最低系统版本，由 `apps/harmony/build-profile.json5` 的 `compatibleSdkVersion` 表达，只反映真实需要支持的最低版本，不因开发机/测试机使用 API26 SDK 就一起抬到 API26。
- **编译目标**：`targetSdkVersion = 26`（HarmonyOS 7）。工程使用 API26 SDK 构建，`targetSdkVersion` 保持 API26，与 `compatibleSdkVersion` 不必为同一 API。
- **能力分流**：API20 / API23 / API26 等高版本能力不抬高安装下限。entry 内的高版本能力通过 `PlatformApiResolver`（`version/PlatformApiResolver.ets`）按运行时 API Level 分流到各 `impl/apiXX` facade；DLP 防窥是例外——实现已迁入独立自包含的 `privacy_acl` HSP，在模块内用 `deviceInfo.sdkApiVersion` / `canIUse` 自行判断 API Level 与 SystemCapability，不经过 entry 的 `PlatformApiResolver`：
  - `impl/api11`：普通系统分享（systemShare，since 11）
  - `impl/api12`：碰一碰分享（knockShare 基础重载，since 12）
  - `impl/api20`：握姿感知、隔空传送（gesturesShare，since 20）
  - `impl/api23`：HDS 悬浮页签等（HdsTabs 及导航目的页，since 23）
  - API26 新能力继续单独使用
- **降级语义**：旧系统缺少某个高版本能力时，只降级该能力（`isSupported()` 返回 false、对应入口返回 false，不伪造状态），**不导致整个应用无法安装**。API12～25 的设备可以正常安装运行，只是按运行时版本禁用对应高版本能力。
- 稳定 facade（`*Service.ets`）不直接 import 版本敏感的高版本能力 Kit、不自行判断具体 API Level；Ability Context（`@kit.AbilityKit` 的 `common`）等基础平台类型可以保留。具体高版本系统调用和版本判断收进 `impl/apiXX`，由 `PlatformApiResolver` 按运行时 API Level 分流（DLP 防窥例外：随实现迁入 `privacy_acl` HSP，见防窥屏一节）。

## 能力总览

| 能力 | Kit | 最低 API | SystemCapability | 权限 | ACL | 实现文件 |
|------|-----|---------|------------------|------|-----|---------|
| 握姿感知 | @kit.MultimodalAwarenessKit (motion) | 20 | SystemCapability.MultimodalAwareness.Motion | ohos.permission.DETECT_GESTURE | 否 | awareness/impl/api20/GripPostureApi20.ets |
| 防窥屏 | @kit.DeviceSecurityKit (dlpAntiPeep) | 20（基础）/ 23（requestAntiPeepOptions） | SystemCapability.Security.DlpAntiPeep | ohos.permission.DLP_GET_HIDE_STATUS | 是（仅 ACL product 装配 privacy_acl HSP） | privacy_acl/src/main/ets/impl/api20/DlpAntiPeepApi20.ets, privacy_acl/src/main/ets/impl/api23/DlpAntiPeepApi23.ets |
| 窗口隐私 | @kit.ArkUI (window) | 9 | SystemCapability.WindowManager.WindowManager.Core | ohos.permission.PRIVACY_WINDOW | 否 | privacy/WindowPrivacyService.ets |
| 应用接续 | 无独立 Kit（Ability 生命周期 + module.json5 continuable 配置） | 未限定独立 API（Staged 模型 ability 级配置） | 无独立 SystemCapability（Ability 级配置） | 无 | 否 | continuity/ContinuationEntrypoint.ets, continuity/AppContinuationService.ets |
| 普通系统分享 | @kit.ShareKit (systemShare) | 11 | SystemCapability.Collaboration.SystemShare | 无 | 否 | share/impl/api11/SystemShareApi11.ets, share/SystemShareService.ets |
| 碰一碰分享 | @kit.ShareKit (harmonyShare knockShare) | 12 | SystemCapability.Collaboration.HarmonyShare | 无 | 否 | share/impl/api12/KnockShareApi12.ets, share/TapShareService.ets |
| 隔空传送 | @kit.ShareKit (harmonyShare gesturesShare) | 20 | SystemCapability.Collaboration.HarmonyShare | 无 | 否 | share/impl/api20/GesturesShareApi20.ets, share/AirTransferService.ets |
| 手写笔 | @kit.ArkUI (组件 TouchEvent) | 未限定独立 API（基础输入事件，随 ArkUI） | 无独立 SystemCapability（基础输入事件） | 无独立权限（基础输入事件） | 否 | input/StylusInputService.ets |
| Native HiLog | @kit.BasicServicesKit (hilog) — C 接口 | 12 | SystemCapability.HiviewDFX.HiLog | 无 | 否 | diagnostics/HarmonyDiagnosticsExporter.ets（ArkTS 侧调用 getHilogSnapshot），cpp/corebridge/napi/napi_init.cpp（C++ 侧 OH_LOG_SetCallback） |
| Core File Kit (fileUri) | @kit.CoreFileKit (fileUri) | 12 | SystemCapability.FileManagement.File.FileUri | 无 | 否 | diagnostics/HarmonyDiagnosticsExporter.ets |
| 校对资源文件选择与导入 | @kit.CoreFileKit (picker.DocumentViewPicker, fileIo) | 12 | SystemCapability.FileManagement.UserFileService; SystemCapability.FileManagement.File.FileIO | 无（系统 Picker 授权用户所选 URI） | 否 | feature/editor/correction/CorrectionResourceImporter.ets, feature/editor/ui/WritingScreen.ets, feature/editor/correction/CorrectionSuggestionsPanel.ets |
| 校对资源状态查询 | WriterCore `get_correction_resource_status` | 1 | NAPI async worker | 无 | 否 | feature/editor/correction/CorrectionDtos.ets, feature/editor/correction/NativeCorrectionBridge.ets, corebridge/napi/napi_correction.cpp |
| 系统剪贴板 | @ohos.pasteboard | 12 | SystemCapability.MiscServices.Pasteboard | 无 | 否 | feature/settings/presentation/SettingsViewModel.ets |
| Application 颜色模式 | @kit.AbilityKit (ApplicationContext) | 11 | 无独立 SystemCapability（随 @kit.AbilityKit Application 生命周期） | 无 | 否 | ui/theme/HarmonyColorModeController.ets |
| HDS Tabs（悬浮页签） | @kit.UIDesignKit (HdsTabs) | 23 | SystemCapability.UIDesign.HDSComponent.Core | 无 | 否 | app/navigation/impl/api23/HdsPrimaryTabsApi23.ets, app/navigation/PrimaryTabShell.ets |
| TextController + LayoutManager | @kit.ArkUI (TextController / LayoutManager) | 12（getLayoutManager/getLineCount/getGlyphPositionAtCoordinate/getLineMetrics）/ 14（getRectsForRange） | 无独立 SystemCapability（随 @kit.ArkUI 整体可用） | 无 | 否 | feature/editor/ui/SujianEditor.ets, feature/editor/render/EditorRenderBackend.ets |
| StyledString / MutableStyledString | @kit.ArkUI (StyledString / MutableStyledString) | 12 | 无独立 SystemCapability（随 @kit.ArkUI 整体可用） | 无 | 否 | feature/editor/render/EditorTextStyleProjector.ets |
| ComponentObserver (inspector) | @kit.ArkUI (inspector) | 12（on('layout') 回调） | 无独立 SystemCapability（随 @kit.ArkUI 整体可用） | 无 | 否 | feature/editor/ui/SujianEditor.ets |
| 沉浸光感材质运行态 | @kit.ArkUI (uiMaterial) | 26 | SystemCapability.ArkUI.ArkUI.Full | 无 | 否 | material/impl/api26/HarmonyMaterialRuntimeApi26.ets |
| 应用共享目录 / 捐献沙箱目录 | 无独立 Kit（module.json5 shareFiles profile） | 23（共享目录 scopes）/ 26.0.0（捐献目录 sharingOS*） | 无独立 SystemCapability（模块级配置） | 无 | 否 | entry/src/main/resources/base/profile/share_files.json（工程资源，不在 platform/ 下） |
| 菜单 / 气泡菜单 | @kit.ArkUI (bindMenu / bindContextMenu) | 12（声明式）/ 11（程序化 bindMenu(isShow, ...)） | 无独立 SystemCapability（随 @kit.ArkUI 整体可用） | 无 | 否 | feature/starmap/ui/StarMapScene.ets, feature/starmap/ui/StarMapListScreen.ets, feature/project/ui/HomeScreen.ets |
| 触摸事件拦截 | @kit.ArkUI (onTouchIntercept) | 12 | SystemCapability.ArkUI.ArkUI.Full | 无 | 否 | feature/starmap/ui/StarMapScene.ets |
| 自定义触摸热区 | @kit.ArkUI (responseRegion) | 8 | SystemCapability.ArkUI.ArkUI.Full | 无 | 否 | feature/starmap/ui/StarMapScene.ets |
| 自定义对话框 | @kit.ArkUI (ComponentContent / PromptAction.openCustomDialog) | 12 | SystemCapability.ArkUI.ArkUI.Full | 无 | 否 | feature/starmap/ui/CreateTitleDialog.ets, feature/starmap/ui/StarMapScene.ets |
| 系统日期选择 | @kit.ArkUI (UIContext.showDatePickerDialog) | 10 | SystemCapability.ArkUI.ArkUI.Full | 无 | 否 | feature/stats/ui/StatsScreen.ets |
| display.isFoldable | @kit.ArkUI (display) | 10 | SystemCapability.Window.SessionManager | 无 | 否 | window/TabletWindowPolicy.ets |
| setPreferredOrientation | @kit.ArkUI (window) | 9 | 无独立 SystemCapability | 无 | 否 | window/TabletWindowPolicy.ets |
| setWindowSystemBarEnable | @kit.ArkUI (window) | 12 | 无独立 SystemCapability | 无 | 否 | window/TabletWindowPolicy.ets |
| getWindowStatus / windowStatusChange | @kit.ArkUI (window) | 12（getWindowStatus）/ 11（windowStatusChange） | SystemCapability.Window.SessionManager | 无 | 否 | window/TabletWindowPolicy.ets, ui/adaptive/AdaptiveContext.ets |
| 网络状态（hasDefaultNet / isDefaultNetMetered） | @kit.NetworkKit (connection) | 12 | SystemCapability.Communication.NetManager.Core | ohos.permission.GET_NETWORK_INFO | 否 | corebridge/NativeWriterCoreBridge.ets |
| 短时任务（退后台保存） | @kit.BackgroundTasksKit (backgroundTaskManager) | 9 | SystemCapability.ResourceSchedule.BackgroundTaskManager.TransientTask | 无 | 否 | lifecycle/HarmonyBackgroundSaveTask.ets |

> 说明：标"未限定独立 API/SystemCapability"的项，是该能力随所属 Kit/ArkUI 整体可用、官方未为它单独声明起始 API Level 或 SystemCapability。已查 HarmonyOS 官方文档与本机 SDK d.ts 确认无独立声明，不是未核实留空。

## 握姿感知

- Kit：`@kit.MultimodalAwarenessKit`（`motion` 模块）
- 接口：
  - `motion.on('holdingHandChanged', callback)`
  - `motion.off('holdingHandChanged', callback?)`
- 枚举 `motion.HoldingHandStatus`：
  - `NOT_HELD = 0`
  - `LEFT_HAND_HELD = 1`
  - `RIGHT_HAND_HELD = 2`
  - `BOTH_HANDS_HELD = 3`
  - `UNKNOWN_STATUS = 16`
- 最低 API：20
- SystemCapability：`SystemCapability.MultimodalAwareness.Motion`
- 权限：`ohos.permission.DETECT_GESTURE`（system_grant，normal，`provisionEnable=false`，since 20，**不需要 ACL**）
- fallback：API < 20 或能力不支持时，握姿固定为 `unknown`，不伪造状态。
- 实现文件：`awareness/impl/api20/GripPostureApi20.ets`
- 对外 facade：`awareness/GripPostureService.ets`（通过 `PlatformApiResolver` 分流，不直接 import `@kit.MultimodalAwarenessKit`）

## 防窥屏

- Kit：`@kit.DeviceSecurityKit`（`dlpAntiPeep` 模块）
- 权限：`ohos.permission.DLP_GET_HIDE_STATUS`（system_grant，system_basic，`provisionEnable=true`，since 18，**需要 ACL**）
  - 需在签名 Profile 的 ACL 中声明，并在 `privacy_acl` 模块的 `module.json5` 的 `requestPermissions` 登记。
- SystemCapability：`SystemCapability.Security.DlpAntiPeep`
- **两个独立轴**：
  - **API Level 轴（运行环境差异）**：API20 基础 / API23 扩展，走 `privacy_acl` 模块内 `impl/apiXX` 子目录分流。因 `privacy_acl` 是自包含 HSP、不依赖 entry 的 `PlatformApiResolver`，两个 facade 均在模块内用内联 `isApiAtLeast`（`deviceInfo.sdkApiVersion`）与 `hasSystemCapability`（`canIUse`）判断运行时 API 与 SystemCapability；entry 侧 `ShoulderSurfingService` 只经 Port/Provider 委托，不做版本判断。
  - **ACL 轴（发行签名资格）**：`DLP_GET_HIDE_STATUS` 需要 ACL，仅持有 ACL 的签名 Profile 才能装配。DLP 实现已迁移到独立 `privacy_acl` HSP 模块，仅 ACL product 装配；普通包（default product）不编译 `privacy_acl`、不声明 `DLP_GET_HIDE_STATUS`。

### API20 基础

- 接口：
  - `isDlpAntiPeepSwitchOn()`
  - `on('dlpAntiPeep', callback)`
  - `off('dlpAntiPeep', callback?)`
  - `getDlpAntiPeepInfo()`
  - `passDlpAntiPeepInfo()`
- 枚举 `DlpAntiPeepStatus`：`PASS = 0`，`HIDE = 1`
- 最低 API：20
- 实现文件：`privacy_acl/src/main/ets/impl/api20/DlpAntiPeepApi20.ets`

### API21 扩展

- 接口：`setAntiPeepMaskLayer(windowId)`
- 最低 API：21

### API23 扩展

- 接口：
  - `requestAntiPeepOptions(context)`
  - `publishAntiPeepInformation()`
- 枚举 `AntiPeepOptionsResult`：`SUCCESS = 0`，`FAIL = 1`，`ALREADY_ON = 2`
- 最低 API：23
- 实现文件：`privacy_acl/src/main/ets/impl/api23/DlpAntiPeepApi23.ets`

### fallback

- API < 20 或能力不支持时，防窥状态为 `unknown`（未解析），不伪造为 `false`（安全侧默认）也不伪造为 `true`。`ShoulderSurfingService.isUnknown()` 返回 true，`isSafe()`/`isPeeping()` 均不成立。
- 状态语义：`PASS` → safe（无窥视）；`HIDE` → peeping（被窥视）；API 调用失败 / 未解析 / 不支持 → unknown。
- `requestEnable()` 在 API < 23 时无法打开系统设置页（`requestAntiPeepOptions` 不可用），返回当前真实开关状态，不伪造；`isEnabled()` 永远查询系统真实开关，不读本地缓存假装启用。
- 官方当前限制：DlpAntiPeep 目前只支持 Phone 设备。官方 2026-09-04 最新最佳实践：https://developer.huawei.com/consumer/en/doc/best-practices/bpta-antipeep-protection
- 对外 facade：`privacy/ShoulderSurfingService.ets`（entry 模块内稳定 facade，不直接 import `@kit.DeviceSecurityKit`；仅 import `@kit.AbilityKit` 的 `common` 用于 context 类型；不认识 API Level 数字，版本判断在 `impl/apiXX` facade）。实际 DLP 系统调用在 `privacy_acl` HSP 模块的 `src/main/ets/impl/apiXX` 中，通过 Port/Provider 接口装配，仅 ACL product 编译。

## 窗口隐私

- Kit：`@kit.ArkUI`（`window` 模块）
- 接口：
  - `window.getLastWindow(context)`
  - `win.setWindowPrivacyMode(enabled)`（截图/录屏时隐藏窗口内容，用于隐私保护；旧的 `setSnapshotSkip` 不是公开 SDK API，`@ohos.window.d.ts` 中无此方法）
- 最低 API：9（`setWindowPrivacyMode` 自 API 9 起可用；当前工程最低 API 为 12，始终满足）
- SystemCapability：`SystemCapability.WindowManager.WindowManager.Core`
- 权限：`ohos.permission.PRIVACY_WINDOW`（API 11+ 为 `normal` / `system_grant`，无需 ACL；API 9–10 为 `system_basic`，但工程最低 API 12 已过此限制）
- ACL：否
- 实现文件：`privacy/WindowPrivacyService.ets`

## 应用接续

- Kit：无独立 Kit。接续主链走 Ability 生命周期（`EntryAbility.onContinue` / `onCreate` / `onNewWant`）+ `module.json5` 中 ability 声明 `"continuable": true`，不创造独立的接续 Kit 名（仓库无此 Kit/import）。
- 接口：
  - `module.json5` 中 ability 声明 `"continuable": true`（系统接续开关）
  - 源设备 `EntryAbility.onContinue` 委托 `ContinuationEntrypoint.onContinue()`，从 `AppContinuationService.getCurrentPayload()` 写出 payload
  - 目标设备 `EntryAbility.onCreate` / `onNewWant` 在 `launchReason === CONTINUATION` 时委托 `ContinuationEntrypoint.restoreFromWant()` 还原 payload
- 最低 API：未限定独立 API（Staged 模型 ability 级配置，随 `@kit.AbilityKit` Ability 生命周期，无独立起始 API Level 声明）
- SystemCapability：无独立 SystemCapability（Ability 级配置）
- 权限：无
- ACL：否（接续是 Ability 级配置，无 DLP ACL 依赖）
- 实现文件：`continuity/ContinuationEntrypoint.ets`（接续序列化/恢复入口，从 EntryAbility 提取）、`continuity/AppContinuationService.ets`
- 说明：`EntryAbility` 仅保留 Ability 生命周期调用，接续的序列化/恢复细节收在 `ContinuationEntrypoint`。接续与 DLP ACL 完全独立，普通包（default product）同样支持接续。

## 分享

三个分享 channel 各自独立后端，按本机 SDK d.ts（`@hms.collaboration.systemShare.d.ts` / `@hms.collaboration.harmonyShare.d.ts`）的 `@since` 标注分别确定起始 API Level，不合并成一个版本结论。稳定 facade（`SystemShareService`/`TapShareService`/`AirTransferService`）不 import `@kit.ShareKit`，系统调用和数据构造在各 `impl/apiXX` facade。官方 API 变更：https://developer.huawei.com/consumer/en/doc/harmonyos-releases/js-apidiff-sharekit-6001

### 普通系统分享（systemShare）

- Kit：`@kit.ShareKit`（`systemShare` 模块）
- 接口：`systemShare.SharedData` / `systemShare.ShareController.show(context)`
- 最低 API：11（`@since 4.1.0(11)`，经 d.ts 确认）
- SystemCapability：`SystemCapability.Collaboration.SystemShare`
- 权限：无
- ACL：否
- fallback：API < 11 不支持（低于 compatibleSdkVersion 12，实际始终可用）
- 实现文件：`share/impl/api11/SystemShareApi11.ets`（impl facade）
- 对外 facade：`share/SystemShareService.ets`（不 import `@kit.ShareKit`，委托 impl）

### 碰一碰分享（knockShare）

- Kit：`@kit.ShareKit`（`harmonyShare` 模块）
- 接口：`harmonyShare.on('knockShare', callback)` / `harmonyShare.off('knockShare', callback)`（无 capability 参数的基础重载）
- 最低 API：12（`@since 5.0.0(12)`，经 d.ts 确认；带 `SendCapabilityRegistry` 参数的重载 since 6.0.0(20)）
- SystemCapability：`SystemCapability.Collaboration.HarmonyShare`
- 权限：无
- ACL：否
- fallback：API < 12 不支持（等于 compatibleSdkVersion，实际始终可用）
- 实现文件：`share/impl/api12/KnockShareApi12.ets`（impl facade）
- 对外 facade：`share/TapShareService.ets`（不 import `@kit.ShareKit`，委托 impl）

### 隔空传送（gesturesShare）

- Kit：`@kit.ShareKit`（`harmonyShare` 模块）
- 接口：`harmonyShare.on('gesturesShare', { windowId }, callback)` / `harmonyShare.off('gesturesShare', { windowId }, callback)`
- 最低 API：20（`@since 6.0.0(20)`，经 d.ts 确认）
- SystemCapability：`SystemCapability.Collaboration.HarmonyShare`
- 权限：无
- ACL：否
- fallback：API 12~19 不支持，`isSupported()` 返回 false，`start()` 返回 false，不伪造状态
- 实现文件：`share/impl/api20/GesturesShareApi20.ets`（impl facade）
- 对外 facade：`share/AirTransferService.ets`（不 import `@kit.ShareKit`，委托 impl）

## 手写笔

- Kit：`@kit.ArkUI`（组件 `TouchEvent`，即 `.onTouch` 回调传入的事件）
- 接口：
  - ArkUI 组件 `TouchEvent`（`extends BaseEvent`）
  - `event.sourceTool` / `SourceTool` 枚举识别笔类工具（当前 SDK 只有 `SourceTool.Pen`；`Rubber/Brush/Pencil/Airbrush` 尚未在 `SourceTool` 中定义）
  - 坐标来自 `event.touches[0].x / y`
  - 压力和倾斜来自事件级 `event.pressure / event.tiltX / event.tiltY`（BaseEvent 字段）
  - 时间戳 `event.timestamp`
- 最低 API：未限定独立 API（ArkUI 组件触摸事件，随 ArkUI 整体可用，无独立起始 API Level 声明）
- SystemCapability：无独立 SystemCapability（基础输入事件）
- 权限：无独立权限（基础输入事件）
- ACL：否
- 实现文件：`input/StylusInputService.ets`
- 说明：ArkUI 原始输入只在 `platform/input` 归一化，不把平台事件类型传进 Core。`StylusInputService` 消费 ArkUI 组件 `.onTouch` 传进来的全局 `TouchEvent`，不再依赖 `@kit.InputKit` 的 `TouchEvent / ToolType`。

## Native HiLog（C 接口）

- Kit：`@kit.BasicServicesKit`（`hilog` 模块，C 接口层）
- 接口：
  - `OH_LOG_SetCallback(LogCallback callback)` — 注册 HiLog 回调，接收当前进程内所有 HiLog 日志
  - `LogCallback` 签名：`void(const LogType, const LogLevel, const unsigned int, const char*, const char*)`
- 最低 API：12（`OH_LOG_SetCallback` 随 HarmonyOS NEXT 基础日志能力可用）
- SystemCapability：`SystemCapability.HiviewDFX.HiLog`
- 权限：无
- ACL：否
- fallback：callback 未注册或缓冲为空时，诊断导出 fallback 到 writer_diagnostics 已落盘的日志文件
- 实现文件：
  - C++ 层：`cpp/corebridge/napi/napi_init.cpp`（`RegisterHilogCallback()` 注册回调，环形缓冲区收集日志，`NativeGetHilogSnapshot` 返回内容）
  - ArkTS 层：`diagnostics/HarmonyDiagnosticsExporter.ets`（通过 `NativeDiagnosticsBridge.getHilogSnapshot()` 读取 C++ 层环形缓冲内容）
- 说明：华为官方 FAQ 确认 `OH_LOG_SetCallback` 会接收当前进程 HiLog：https://developer.huawei.com/consumer/cn/doc/doccenter-tools-faq/faqs-app-debugging-77

## Core File Kit（fileUri）

- Kit：`@kit.CoreFileKit`（`fileUri` 模块）
- 接口：`fileUri.getUriFromPath(path: string): string` — 将沙箱文件路径转换为 `file://` URI，用于系统分享
- 最低 API：12（`fileUri.getUriFromPath` 随 `@kit.CoreFileKit` 可用）
- SystemCapability：`SystemCapability.FileManagement.File.FileUri`
- 权限：无
- ACL：否
- fallback：无（低于 compatibleSdkVersion 12 的设备不存在）
- 实现文件：`diagnostics/HarmonyDiagnosticsExporter.ets`
- 说明：Share Kit 官方示例里的沙箱文件 URI 也是 `@kit.CoreFileKit.fileUri.getUriFromPath()`：https://developer.huawei.com/consumer/cn/doc/harmonyos-guides-V5/share-utd-video-V5

## Core File Kit（校对资源导入）

- Kit：`@kit.CoreFileKit`（`picker` 与 `fileIo` 模块）
- 接口：
  - `picker.DocumentViewPicker(context).select(options)` 由 UIAbility 页面选择 `.klm` 或 Core 约定的词典 `.txt`
  - `fileIo.openSync(uri, READ_ONLY)` 打开 Picker 返回的用户文件 URI，再由 `fileIo.copyFile(fd, stagingPath)` 复制到应用沙箱
  - `fileIo.moveFile(stagingPath, destination, 0)` 覆盖更新目标词典；`listFileSync` / `statSync` 检查现有资源
- 最低 API：12（`DocumentViewPicker(context)` 构造函数自 API 12 可用；文件操作本身更早可用）
- SystemCapability：`SystemCapability.FileManagement.UserFileService`、`SystemCapability.FileManagement.File.FileIO`
- 权限：无；系统 Picker 授权所选用户文件 URI，导入后文件在应用私有目录内
- ACL：否
- fallback：Picker 取消或资源复制失败时保留原有 Core 配置与词典文件；资源状态由 Core 汇报。模型文件存在性由 Core 检查，模型格式在首次校对时验证
- 实现文件：`feature/editor/correction/CorrectionResourceImporter.ets`、`feature/editor/correction/CorrectionSuggestionsPanel.ets`、`feature/editor/ui/WritingScreen.ets`；severity 下划线由 `feature/editor/ui/SujianEditor.ets` 渲染
- 词典映射：仅接受 `word_freq.txt`、`same_pinyin.txt`、`same_stroke.txt`、`custom_confusion.txt`、`person_name.txt`、`place_name.txt`、`proper_name.txt`、`ignored_words.txt`，复制到 Core `app_data_root/correction-data/`；更新后由 Core 在下一次校对请求中重读。
- 模型映射：`.klm` 文件复制到同一应用私有资源目录下的唯一文件名，并通过现有 `nativeCorrectionSetModelPath` 配置 Core；模型格式由首次校对时验证。
- 官方参考：[@ohos.file.picker](https://developer.huawei.com/consumer/cn/doc/harmonyos-references/js-apis-file-picker)、[@ohos.file.fs](https://developer.huawei.com/consumer/cn/doc/harmonyos-references/js-apis-file-fs)

## 系统剪贴板

- Kit：`@ohos.pasteboard`
- 接口：
  - `pasteboard.createData(mimeType: string, content: string): PasteData` — 创建剪贴板数据
  - `pasteboard.getSystemPasteboard(): SystemPasteboard` — 获取系统剪贴板实例
  - `systemPasteboard.setData(data: PasteData): Promise<void>` — 写入剪贴板
  - `pasteboard.MIMETYPE_TEXT_PLAIN` — 纯文本 MIME 类型常量
- 最低 API：12（`@ohos.pasteboard` 随 HarmonyOS NEXT 基础剪贴板能力可用）
- SystemCapability：`SystemCapability.MiscServices.Pasteboard`
- 权限：无
- ACL：否
- fallback：写入失败时返回 false，不伪造成功
- 实现文件：`feature/settings/presentation/SettingsViewModel.ets`（`copyDeviceInfoToClipboard()` 方法）

## Application 颜色模式（setColorMode）

- Kit：`@kit.AbilityKit`（`ApplicationContext` 模块）
- 接口：
  - `ApplicationContext.setColorMode(ConfigurationConstant.ColorMode): void` — 设置应用级别的深浅色模式
  - 枚举 `ConfigurationConstant.ColorMode`：
    - `COLOR_MODE_NOT_SET = -1`（跟随系统）
    - `COLOR_MODE_DARK = 0`
    - `COLOR_MODE_LIGHT = 1`
- 最低 API：11（`ApplicationContext.setColorMode` 从 API11 起支持，覆盖 compatibleSdkVersion=12 安装基线）
- SystemCapability：无独立 SystemCapability（随 `@kit.AbilityKit` Application 生命周期）
- 权限：无
- ACL：否
- 优先级：`UIAbility 深浅色 > Application 深浅色 > 系统深浅色`
- fallback：调用失败时只记 hilog error，不阻塞应用运行；UI 仍按系统默认颜色模式渲染
- 实现文件：`ui/theme/HarmonyColorModeController.ets`
- 说明：华为官方文档要求 `setColorMode()` 在页面 `loadContent` 成功之后才能调用，因此 `HarmonyColorModeController` 引入 `pageLoaded` 标志和 `desiredMode` 缓存机制，确保时序正确。官方文档：https://developer.huawei.com/consumer/cn/doc/doccenter-references/api/js-apis-inner-application-uiabilitycontext

## HDS Tabs（悬浮页签）

- Kit：`@kit.UIDesignKit`（HDS UI Design Kit 的 `HdsTabs` 组件）
- 接口：
  - `HdsTabs` 悬浮页签组件
  - `HdsTabsAttribute.barFloatingStyle(style?: HdsTabsFloatingStyle)` — 悬浮页签样式（since 6.1.0(23)）
  - `HdsTabsFloatingStyle.systemMaterialEffect?: SystemMaterialParams` — 系统材质效果
  - `HdsTabsFloatingStyle.miniBar?: HdsTabsMiniBar` — 页签栏同排的 MiniBar 区域（since 6.1.0(23)），承载页签之外的主操作
  - `HdsTabsMiniBar.miniBarBuilder: CustomBuilder | ComponentContent`（必填）— MiniBar 内自绘内容；`enableMiniBarBackground?: boolean`（默认 true，跟随页签栏背景）；`miniBarWidth?: HdsBarWidthRangeOptions`；`miniBarStyle?: HdsBarStyle`；`barLayoutMode?: HdsBarLayoutMode`（since 6.1.1(24)：HORIZONTAL 与页签同排 / VERTICAL 分两排）
  - `HdsTabsMiniBar.onBarStyleChange?: BarStyleChangeCallback(miniBarStyle, tabBarStyle, miniBarWidth, tabBarWidth, mode)` — MiniBar/页签栏折叠态变化回调；`HdsBarStyle { COLLAPSE = 0, EXPAND = 1 }`；`HdsTabsBarChangeMode { NORMAL = 0 旋转/开关屏, USER_CLICK = 1 点击折叠的 MiniBar 或 TabBar, APP_TRIGGER = 2 由 controller 接口触发 }`
  - `HdsTabsController.applyMiniBarStyle(HdsBarStyle)` — 切换 MiniBar 显示风格（自带默认动画；此路径发出的变化是 `APP_TRIGGER`）
  - **以上 `miniBar` / `HdsTabsMiniBar` / `onBarStyleChange` / `applyMiniBarStyle` 仅作为 SDK 事实记录，本项目当前不使用**：Issue #905 第二~五轮曾用 MiniBar 承载主操作「+」，已按复核意见整体撤销（MiniBar 与页签栏同排，不是原右下角独立 FAB 的位置，不能为了拿材质改掉用户要的位置）。本项目 `HdsPrimaryTabsApi23.ets` 的 `barFloatingStyle` 只保留页签栏本身的材质、宽度与底栏逻辑。
  - `hdsMaterial.MaterialType.IMMERSIVE = 101` — 沉浸光感材质类型（since 6.1.0(23)）
- 最低 API：23
- SystemCapability：`SystemCapability.UIDesign.HDSComponent.Core`
- 权限：无
- ACL：否
- fallback：API < 23 使用普通 `Tabs + BottomTabBarStyle`
- 实现文件：`app/navigation/impl/api23/HdsPrimaryTabsApi23.ets`
- 对外 facade：`app/navigation/PrimaryTabShell.ets`（通过 `PlatformApiResolver` 分流）

## HDS Navigation / HdsNavDestination（沉浸光感导航）

- Kit：`@kit.UIDesignKit`（HDS UI Design Kit 的 `HdsNavigation` / `HdsNavDestination` 组件）
- 接口：
  - `HdsNavigation(pathInfos?: NavPathStack): HdsNavigationAttribute` — HDS 导航容器（since 5.1.0(18)）
  - `HdsNavDestination(): HdsNavDestinationAttribute` — HDS 导航目标页（since 5.1.0(18)）
  - `HdsNavDestinationAttribute.titleBar(options?: HdsNavigationTitleBarOptions)` — 标题栏配置
  - `HdsNavDestinationAttribute.hideTitleBar(hide: boolean, animated?: boolean)` — 隐藏标题栏
  - `HdsNavDestinationAttribute.onBackPressed(callback)` — 返回按钮回调
  - `HdsNavDestinationAttribute.onShown(callback)` — 页面显示回调
  - `HdsNavigationTitleBarOptions.content?.title?.mainTitle: ResourceStr` — 标题文本
  - `HdsNavigationTitleBarOptions.style?: TitleBarStyleOptions` — 标题栏样式（Issue #932 登记实际使用字段）：
    - `scrollEffectOpts?: ScrollEffectOptions`（since 5.1.0(18)）— `enableScrollEffect?: boolean`（默认 true）、`scrollEffectType?: ScrollEffectType`（默认 `COMMON_BLUR`）；本项目设 `enableScrollEffect: false` 关闭标题栏滚动模糊
    - `originalStyle?: HdsNavigationTitleBarStyle`（since 5.1.0(18)）— 滚动效果生效前的标题栏样式，`backgroundStyle.backgroundColor` 可设背景色。官方文档明确模糊样式类型（COMMON_BLUR / GRADUAL_BLUR）下背板默认值本来就是透明色，写 `Color.Transparent` 不改变可见像素；Issue #932 复核后本项目改用 `sys.color.background_secondary` 承接标题栏底色——白条根因是标题栏区域无承接层、露出 HDS surface，而非透明配置，且 `backgroundColor` 不能设在 `HdsNavDestination` 本体（会被 HDS surface 覆盖）
    - `systemMaterialEffect?: SystemMaterialParams`（since 6.1.0(23)）— 标题栏的系统材质效果
  - 枚举 `ScrollEffectType`（since 5.1.0(18)）：`COMMON_BLUR = 0`（5.1.0(18)）、`GRADUAL_BLUR = 1`（6.0.0(20)）、`GRADIENT_BLUR = 2`（6.0.0(20)）、`IMMERSIVE_GRADIENT_BLUR = 3`（6.1.0(23)）；本项目只用 `COMMON_BLUR`
- 最低 API：23（本项目使用条件：API23+ 时启用，API < 23 fallback 到普通 Navigation/NavDestination）
- SystemCapability：`SystemCapability.UIDesign.HDSComponent.Core`
- 权限：无
- ACL：否
- fallback：API < 23 使用普通 `Navigation` / `NavDestination`
- 实现文件：
  - `app/navigation/impl/api23/HdsPrimaryTabsApi23.ets`（HdsNavigation 替代 Navigation）
  - `app/navigation/impl/api23/HdsAppDestinationApi23.ets`（HdsNavDestination 壳，根页面：作品/星图/统计）
  - `app/navigation/impl/api23/HdsWorkspaceDestinationApi23.ets`（HdsNavDestination 壳，项目工作区）
  - `app/navigation/impl/api23/HdsWritingDestinationApi23.ets`（HdsNavDestination 壳，写作页）
  - `app/navigation/impl/api23/HdsSettingsDestinationApi23.ets`（HdsNavDestination 壳，设置页）
  - `feature/project/ui/WorkspaceScreen.ets`（HdsNavDestination 替代 NavDestination，条件渲染）
  - `feature/editor/ui/WritingScreen.ets`（HdsNavDestination 替代 NavDestination，条件渲染）
  - `feature/settings/ui/SettingsScreen.ets`（HdsNavDestination 替代 NavDestination，条件渲染）

## TextController + LayoutManager

- Kit：`@kit.ArkUI`（`TextController` / `LayoutManager`）
- 接口：
  - `TextController.getLayoutManager()` —— 获取文本布局管理器（API12+）
  - `LayoutManager.getLineCount()` —— 获取行数
  - `LayoutManager.getGlyphPositionAtCoordinate(x, y)` —— 坐标→字符位置命中测试
  - `LayoutManager.getLineMetrics(index)` —— 获取行度量信息
  - `LayoutManager.getRectsForRange(start, end)` —— 获取字符范围的矩形区域（API14+）
- 最低 API：12（`getLayoutManager`/`getLineCount`/`getGlyphPositionAtCoordinate`/`getLineMetrics`），14（`getRectsForRange`）
- SystemCapability：无独立 SystemCapability（随 `@kit.ArkUI` 整体可用）
- 权限：无
- ACL：否
- fallback：API12-13 无 `getRectsForRange`，通过 `getLineMetrics` + `getGlyphPositionAtCoordinate` 推导 caret/selection 矩形
- 实现文件：`feature/editor/ui/SujianEditor.ets`、`feature/editor/render/EditorRenderBackend.ets`
- 说明：HarmonyOS 官方文档 https://developer.huawei.com/consumer/cn/doc/doccenter-references/api/ts-text-common

## StyledString / MutableStyledString

- Kit：`@kit.ArkUI`（`StyledString` / `MutableStyledString`）
- 接口：
  - `MutableStyledString(text)` —— 创建可变样式字符串
  - `TextStyle({ fontSize, ... })` —— 文本样式
  - `LineHeightStyle({ lineHeight })` —— 行高样式
  - `ParagraphStyle({ textIndent })` —— 段落样式（首行缩进）
  - `styledString.setStyle(style, start, end)` —— 对范围应用样式
- 最低 API：12
- SystemCapability：无独立 SystemCapability（随 `@kit.ArkUI` 整体可用）
- 权限：无
- ACL：否
- fallback：无（等于 compatibleSdkVersion 12，实际始终可用）
- 实现文件：`feature/editor/render/EditorTextStyleProjector.ets`
- 说明：华为官方文档 https://developer.huawei.com/consumer/cn/doc/doccenter-capabilities/arkts-styled-string

## ComponentObserver（inspector）

- Kit：`@kit.ArkUI`（`inspector` 模块）
- 接口：
  - `getUIContext().getUIInspector().createComponentObserver(id: string): ComponentObserver` — 创建组件观察者
  - `ComponentObserver.on('layout', callback: () => void): void` — 注册布局完成回调
  - `ComponentObserver.off('layout', callback: () => void): void` — 注销布局完成回调
- 最低 API：12（`on('layout', callback)` 从 API12 起支持；`ComponentObserver` 从 API10 起可用）
- SystemCapability：无独立 SystemCapability（随 `@kit.ArkUI` 整体可用）
- 权限：无
- ACL：否
- fallback：无（等于 compatibleSdkVersion 12，实际始终可用）
- 实现文件：`feature/editor/ui/SujianEditor.ets`
- 说明：用于解决 setStyledString 后同步读取 LayoutManager 拿到上一版布局的时序问题。华为官方文档明确"文本内容变更后，需等待布局完成才可获取到最新的布局信息"，ComponentObserver 的 `layout` 回调是系统布局完成的官方通知入口。官方文档：https://developer.huawei.com/consumer/cn/doc/doccenter-references/api/ts-text-common

## 沉浸光感材质运行态（uiMaterial）

- Kit：`@kit.ArkUI`（`uiMaterial` 模块）
- 接口：
  - `uiMaterial.getMaterialInfo(): MaterialInfo` —— 读取应用级沉浸材质配置状态（since 26.0.0）
  - `MaterialInfo.state: MaterialState` —— `DEFAULT = 0` / `ENABLE = 1` / `DISABLE = 2`
  - `MaterialInfo.type: MaterialType` —— `IMMERSIVE = 2`
- 最低 API：26
- SystemCapability：`SystemCapability.ArkUI.ArkUI.Full`（随 `@kit.ArkUI` 整体可用，官方未为该方法单独声明 syscap）
- 权限：无
- ACL：否
- fallback：API < 26、能力不支持或调用抛错时，诊断包对应字段写 `unavailable` / `error`，不伪造状态
- 实现文件：`material/impl/api26/HarmonyMaterialRuntimeApi26.ets`
- 语义边界：`MaterialState` 对应的是应用 `module.json5` 里的材质配置状态，只说明"应用级材质开关配成了什么"，**不证明**某个 HdsTabs / HdsNavDestination 已经实际渲染了沉浸光感。诊断包因此分开记录应用级配置（`appMaterialState` / `appMaterialType`）与 HDS 请求值（`hdsRequestedMaterialType` / `hdsRequestedMaterialLevel`），不再用 `state !== DISABLE` 推导 `immersiveMaterialSupported`。
- 说明：本机 SDK d.ts `/opt/devecostudio/sdk/default/openharmony/ets/api/@ohos.arkui.uiMaterial.d.ts`（`MaterialState`：*states of the application-level immersive system material configuration*）。官方文档：https://developer.huawei.com/consumer/cn/doc/HarmonyOS-Guides/arkts-immersive-light-sense-enable

## 沉浸式系统材质（uiMaterial.ImmersiveMaterial / hdsMaterial）

**生效范围一句话结论：页面内容流不生效，标题栏与底部页签生效。** 内容流里挂 `systemMaterial()` 会被 ArkUI 拒绝并打
`Ace: Material inactive: out of scope. Use component in navigation title bar or Tabbar.`；
把组件从 Button 换成外层 `Stack` 日志不变 → **按位置判定，不是按组件类型**。

### 生效三要素（缺一不可，实机逐一验证）

| 项 | 值 | 踩坑 |
|---|---|---|
| 材质类型 | `MaterialType.ADAPTIVE` | 写 `IMMERSIVE` 标题栏不出光（底栏的 IMMERSIVE 不能照抄到标题栏） |
| 挂载方式 | 通用属性 `.systemMaterial()` + `uiMaterial.ImmersiveMaterial` | HDS 专属的 `systemMaterialEffect` 只作用于 HDS 自己渲染的节点 |
| 摘掉遮挡 | `backgroundColor` / `backgroundBlurStyle` / `border` / `shadow` 全清 | 官方明确这些属性会盖在材质层上，导致光看不见 |

### 实现

- Kit：`@kit.ArkUI`（`uiMaterial` 模块）
- 接口：`uiMaterial.isImmersiveMaterialSupported()`、`new uiMaterial.ImmersiveMaterial(ImmersiveOptions)`、`ImmersiveOptions { style, materialColor, colorInvert, applyShadow, interactive, lightEffect }`、`LightEffectOptions { color }`、通用属性 `CommonMethod.systemMaterial(material: SystemUiMaterial | undefined)`（均 since 26.0.0）
- 最低 API：26
- SystemCapability：`SystemCapability.ArkUI.ArkUI.Full`
- 权限：无
- ACL：否
- 实现文件：`material/impl/api26/ImmersiveMaterialApi26.ets`、`material/HarmonyMaterialService.ets`
- **典型使用**：`app/navigation/impl/api23/HeaderActionCapsule.ets` —— 标题栏 `stackBuilder` 自绘的页头动作胶囊。胶囊与「手指下的光」是同一个材质层同时给出的两件事，不是二选一（这一点实测纠正过一次错误判断）。
  - 光效形态：整颗胶囊从手指位置向两侧漫开变亮（手指处 +169 级，两端递减），不是三个圆各自亮。原生 `content.menu.value` 按钮则每按钮各挂一块、圆与圆之间不受影响。
  - 静止态亮度由材质层决定（实测 20 级 → 55 级），不要再自己叠半透明白面。
- **官方文档原文**（《沉浸光感》`ui-design-hds-component-material`）：「HDS导航：通过设置 `TitleBarStyleOptions` 的 `systemMaterialEffect` 参数，可为标题栏按钮设置沉浸光感视效。HDS底部页签：通过设置 `HdsTabsFloatingStyle` 的 `systemMaterialEffect` 参数，可为底部页签设置沉浸光感视效。」两处示例都用 `ADAPTIVE`。
- 底栏那圈「手指下的光」已定位到确切来源：`app/navigation/impl/api23/HdsPrimaryTabsApi23.ets` 的 `buildFloatingStyle()` 里
  `systemMaterialEffect: { materialType: hdsMaterial.MaterialType.IMMERSIVE, materialLevel: hdsMaterial.MaterialLevel.ADAPTIVE }`。
  把这一块注释掉重新装机，按住底栏光斑**完全消失**（只剩一层均匀暗面），加回来恢复 —— 因果确认。
  光斑跟随手指而非绑定某个 tab：按统计 tab、按星图 tab、按两个 tab 中间的空隙，光斑都精确落在手指下方。
  走的是 HdsTabs **专属**属性 → `hdsMaterial.SystemMaterialParams`（`@kit.UIDesignKit`，6.1.0(23)），与通用属性那条路不是同一套实现。
- 标题栏另有一条 HDS 专属入口：`HdsNavigationTitleBarOptions.style.systemMaterialEffect`（`TitleBarStyleOptions`，同一文件 1399 行起）。官方《沉浸光感》明确「可为标题栏按钮设置沉浸光感视效」，实测对 **HDS 原生 `content.menu.value` 按钮**有效（工作区/写作页三个独立圆，按住最近最亮、两侧递减），对 `stackBuilder` 自绘节点**无效** —— 自绘节点要走通用属性 `systemMaterial`。
- 页面内按钮拿定点光的其它候选也都试过，均不可用：
  - `hdsEffect.pointLight`（`@kit.UIDesignKit`，`@since 6.0.0(20)`，`SystemCapability.UIDesign.HDSComponent.Core`）：语义是光源照亮**周围**组件，本就不是单组件按压反馈；且本机 ArkTS 侧构造成功、能力检查通过（`apiLevel=26 hdsCore=true effect=true`），native 层每次报 `HDS_hdsbase: [42]Wrong argument type. int32 expected.`，enum 成员/字面量 int/只留两字段/去掉 options 全被拒。
    - d.ts 形参（本机 `hms/ets/api/@hms.hds.hdsBaseComponent.d.ets`）：`HdsEffectBuilder.pointLight(value: PointLightEffect)`；`PointLightEffect { sourceType?: PointLightSourceType; illuminatedType?: PointLightIlluminatedType; options?: PointLightOptions }`；`PointLightOptions { color?: ResourceColor; intensity?: number; height?: Dimension; bloom?: number }`；`PointLightSourceType { NONE=0, SOFT=1, BRIGHT=2 }`；`PointLightIlluminatedType { NONE=0, BORDER=1, CONTENT=2, BORDER_CONTENT=3, DEFAULT_FEATHERING_BORDER=20 }`。
    - 根因定位：报错来自 native 侧对这个结构的取参，ArkTS 侧类型与枚举取值都合法却仍被拒（连只传 `sourceType` + `illuminatedType`、完全不传 `options` 也一样），即当前 SDK/设备上 `pointLight` 的 native 实参要求与公开 d.ts 对不上。**按实机证据本轮不接入**：`pressShadow` 与它共用同一个 `.visualEffect`，把已知被 native 拒收的调用塞进同一个 `HdsEffectBuilder` 会连累已经能工作的按压视效，不把报错调用直接重复堆回线上。
    - **Issue #937 只读取证补充（pointLight 仍不接入，属能力边界）**：本机 SDK（`~/.harmony-cli/sdk/default`，API 26）的公开 API 元数据 `hms/ets/api/device-define/api-version/UIDesignKit.json` 中，`hdsEffect` 相关 74 条里只有 namespace / `PressShadowType` / `HdsEffectBuilder` / `constructor` / `pressShadow` / `buildEffect` 标有 `publicApi: \"6.0.0(20)\"`；`pointLight`、`PointLightEffect`、`PointLightOptions`、两个点光源枚举及成员全部为 `publicApi: \"\"`（未公开）。Previewer 的 `hms/previewer/systemHsp/hmos/hdscomponent/HdsComponent.hsp` 内除公开方法外还有内部方法 `pointLightSource` / `pointLightIlluminated`，并对 `pointLight` 标注 “mocked implementation”。模拟器镜像（API 24）里 `libhds_base.z.so` 的报错模板来自通用 `GetInt32`/`ParseInt`（`[42]` 是按源码行号输出，不是参数序号），`libhdsvisualeffectsimpl.z.so` 含 `PointLightSourceEffectPara` / `PointLightIlluminatedEffectPara` / `PointLightUnsupportedHandler`。综合判断：当前运行时上 `pointLight` 的对象式实参与 native 取参方式不匹配（native 更像按 int32 或内部属性名读取），**本机无法用它实现动态受光**。该结论不再作为重做 FAB 玻璃表面的前置条件（见下文「Issue #937 现行结构」），也不再重复入参照样试错。
  - `hdsEffect.pressShadow(PressShadowType)`：只支持 Button，官方定义为「按压交互时自动计算背景色变化」的视效，产生的是**系统渐变按压高光**，不是跟随手指的反射点光。同一枚举两个档位都有实机数据：`BLEND_GRADIENT`（中心 85% 白 → 边缘 100% 白）量到由内向外递增的整面高光，白底上削顶到 255 看不出，深色底上整颗刷白、白色 + 号消失；`BLEND_WHITE`（15% 白叠加）在同一表面上按压可见且 + 号仍清晰。**本轮取 `BLEND_WHITE`**（`PrimaryFab.ets` 里改一行即可切换）。官方示例要求 Button 配 `stateEffect: false`，否则内建压暗与按压阴影互盖。
  - `CommonMethod.lightUpEffect(0..1)`（common.d.ts:21813-21845）：只有整体亮度一个参数，**无位置参数**。
  - `HdsVisualComponent`：只有 `DUAL_EDGE_FLOW_LIGHT_WITH_BACKGROUND_MASK` 一个场景，无悬浮按钮场景。HDS 组件库里没有任何悬浮按钮类组件。
- **页面内悬浮按钮不能用沉浸材质（Issue #905 定论）**。真机日志：`Ace: Material inactive: out of scope. Use component in navigation title bar or Tabbar.`；官方《开启沉浸光感》明确生效范围只有 Navigation/NavDestination 标题栏与合规底部 TabBar，内容流里的右下角 FAB **不在范围内**。因此 `ui/components/PrimaryFab.ets` 已删除全部 `systemMaterial` 尝试，不再对外宣称「沉浸光感」；现行实现见下方「Issue #905 现行结构」（毛玻璃 + 反色混色描边 + `pressShadow`，均为独立于沉浸材质的公开能力）。离线数据保留在下面两张表里。**范围提醒**：被否掉的是 `systemMaterial`（沉浸材质）这一条路，不等于内容流里没有可用的模糊与光影——通用 `backgroundBlurStyle` 与 `hdsEffect.pressShadow` 都是独立于沉浸材质的公开能力，第七轮复核后已恢复使用（见下一条与「让 FAB 在深色模式有可见边界」一节）。下面两条是当时的中间尝试，其中第一条现已恢复为现行做法，第二条仍是死路：
  - **现行可用（Issue #905 第七轮恢复）**：通用属性 `backgroundBlurStyle(BlurStyle.Thin)` + `shadow(ShadowStyle.OUTER_DEFAULT_XS)` + 透明底色 + 1vp 描边。它与底栏 `barBackgroundBlurStyle` 是同一套通用模糊；实测圆盘 20~22 级、边界清晰、阴影 3 级，稳定可读。它没有沉浸材质那种跟手追光（那是 `systemMaterial` 的能力），但**普通毛玻璃是真实渲染，不是伪造**：透明底色与模糊必须配套，纯实心 `backgroundColor` 会把模糊整个挡住。
  - **描边再进一步（Issue #905 第八轮）**：原来那条写死的 `border(1vp, sys.color.icon_secondary)` 是**固定灰环**，不会随背景变化，不能叫「反射光」。现改为同尺寸 `Stack` 内叠一层只负责混色的 `Circle().stroke('#66FFFFFF').strokeWidth(1).blendMode(BlendMode.DIFFERENCE, BlendApplyType.FAST).hitTestBehavior(HitTestMode.None)`：`DIFFERENCE` 把描边像素与下层已绘制画面做差值混合，圆环颜色随按钮底下那圈画面（毛玻璃透上来的邻近亮暗/色彩）变化。它采样的是**描边覆盖处的本地像素**，不会去屏幕别的坐标找远处光源；与 `systemMaterial` 的沉浸追光、`hdsEffect.pointLight` 是**三个不同机制**，不得互相冒充。必须只挂在描边 Circle 上（挂到 Button 主体会连正文图标和毛玻璃一起被混色），且必须用 `BlendApplyType.FAST`（`OFFSCREEN` 模式下同层 `backgroundBlurStyle` 可能取不到正确画面，也**不要**给整个 FAB 加 `renderGroup` 或离屏）。反馈过弱/过强时只调描边 Alpha。
  - ~~`systemMaterial(floatingMaterial(false))` + `shadow(ShadowStyle.OUTER_DEFAULT_XS)`~~：**这一条才是**「材质背景层在纯黑底上全透明，`shadow` 没有可投的实体，一起消失，深色模式下圆盘不可见」的原因（材质接管阴影后 `applyShadow` 传 false，改回自定义 shadow 也没有可投实体）。
- ⛔ **以下第二~五轮（把「+」搬到 HDS MiniBar）是已整体撤销的错误设计分支，仅作溯源。** 复核结论：MiniBar 与页签栏同排，**不是**原右下角独立 FAB 的位置；不能为了让材质生效就改掉用户要的位置，也不能把「搬到 MiniBar」当成光感已复刻。相关内容（`app/navigation/PrimaryActionChannel.ets`、`style.miniBar`、`primaryActionMiniBar()`、`miniBarStyleChange` 折叠/展开逻辑、三页隐藏页面 FAB 的 `isHdsMiniBarHostingPrimaryAction()`）已全部从代码删除；`PrimaryFab.ets` 恢复为内容流右下角**唯一**主操作载体。
- ~~**Issue #905 第二~五轮的起点：改把主操作搬到合规宿主**~~。窄屏主操作「+」曾不再由内容流 FAB 呈现，而是复用页面上已有的那一个 `HdsTabs`（`app/navigation/impl/api23/HdsPrimaryTabsApi23.ets`），在同一个 `barFloatingStyle` 里加 `miniBar: { miniBarBuilder: this.primaryActionMiniBar }`，把「+」放在页签栏同排的 MiniBar 区域，由 HDS 宿主透出系统沉浸光感；点击只派发 `PrimaryActionChannel` 里登记的**页面原有**创建入口（新建作品 / 新建卷 / 新建星图），不另起业务逻辑。内容流 `PrimaryFab.ets` 因此降级为「普通主操作按钮」，只在 API < 23 或宽屏/无 HDS 底部 TabBar 的版型使用。MiniBar 与页签同排，**不保证落在原来的右下角 FAB 位置**——这是官方公开组件的几何限制，不是漏改。
- ⛔【已撤销】**Issue #905 第三轮：MiniBar 的折叠/点击语义已按官方 API 处理，但「原右下角位置 + 原生光感 + 加号一次点击新建」三者仍无法同时满足**。官方 d.ts 行为：宽度 < 600vp 时首次创建的 MiniBar 默认 `COLLAPSE`；点击**折叠的** MiniBar 或 TabBar 会以 `USER_CLICK` 模式触发折叠/展开变化；宽度 < 440vp 时 MiniBar 展开与 TabBar 折叠还会重排底部导航。本实现把该 MiniBar 当**主操作宿主**而不是可折叠导航区，因此用 `miniBarStyle: HdsBarStyle.EXPAND` 让它创建即展开，避免「第一次点按被系统吃成展开 MiniBar」。~~`onBarStyleChange` 里凡非 `APP_TRIGGER` 模式把它变成 `COLLAPSE` 就立刻拉回 EXPAND；`miniBarWidth` 三档都钉 48，不随 440/600/840vp 宽度档位改档重排。~~ **这两句已被下一段（第四轮）更正：无条件拉回会和用户展开底部页签栏的动作抢控制权，`barWidth.smallWidth` 也需回到官方 236vp。****未使用** `barLayoutMode`（`HORIZONTAL` 即官方默认同排，`VERTICAL` 分两排且是 since 6.1.1(24)）：VERTICAL 只是把 MiniBar 单独放到另一排，同样不构成右下角独立 FAB 位置，故不引入。**结论：原生光感材质存在于合规宿主上这一点成立，但「位置复刻」和「加号单击即为新建」在公开组件下无法与前者同时保证；真机未连接，上述交互未经目视确认，本条按未完成如实保留。**
  - 阴影档位实机灰度落差（Pocket 2，量按钮右边缘相对背景）：不设 0 级 / `OUTER_FLOATING_MD` 35 级 / `OUTER_FLOATING_SM` 14 级 / `OUTER_DEFAULT_XS` 3 级。`OUTER_DEFAULT_*` 与 `OUTER_FLOATING_*` 是两套并行档位，**不能按名字里的「SM」推断轻重**。
  - **描边不是不能用，而是不能压在 systemMaterial 的 Button 本体上**。挂 `border` 到受光的 Button 会盖掉材质层；放到独立节点（`hitTestBehavior(HitTestMode.None)`）就没有这个冲突（#811 用 `zIndex(1)`，第八轮用同尺寸 `Stack` 叠一层混色 `Circle`，不再需要 `zIndex`）。
- ⛔【已撤销】**Issue #905 第四轮：窄屏宽度预算 + `onBarStyleChange` 的双状态语义（更正上一段的两处断言）**。
  - `barWidth.smallWidth` 由自取的 280vp 改回**三页签官方小屏标准宽 236vp**（`76 * 3 + 8`）。宽度预算：360vp 窗口下 `236`(页签栏) + `48`(MiniBar) + 小于 600vp 时 `barSideMargin` 默认左右各 16vp = **316vp**，给 HDS 自己的两栏间隔留出余量；原 280vp 会把整个底部宽度挤满，不能断言两块都能常驻展开。中/大档仍为 400/500，未改。
  - `miniBarWidth` 三档钉 48 只保证 **MiniBar 自身不换档重排**，不代表窄屏下页签栏与 MiniBar 都能常驻展开——这由官方 HDS 双栏状态管理决定，上一段「48vp 就不会重排」的说法不准确，以本段为准。
  - `onBarStyleChange` 会同时通知 `miniBarStyle` **和** `tabBarStyle`，必须一起判断，不能再只看 MiniBar 一侧无条件 `applyMiniBarStyle(EXPAND)`。**（本段两处旧断言已被第五轮更正，见下一条）**
  - `miniBarStyle: HdsBarStyle.EXPAND` 仍作为**首次创建首选**保留；`primaryActionMiniBar()` → `PrimaryActionChannel.dispatch()` 的单击主操作链未变。
  - **结论不变**：原生光感材质存在于合规宿主上成立，但「原右下角 FAB 位置复刻」做不到，且上述窄屏交互未经真机目视确认（`hdc list targets` 为空）。
- ⛔【已撤销】**Issue #905 第五轮：折叠态点击的真实状态转换（更正上一段「`miniBarStyle === COLLAPSE` 就拉回即可单击」的错误）**。
  - 点**折叠态** MiniBar 是把它从 `COLLAPSE` 切成 `EXPAND`，`onBarStyleChange` 回调拿到的是**变化后**的 `miniBarStyle === EXPAND`。所以「只在回调里看到 `COLLAPSE` 才 `applyMiniBarStyle(EXPAND)`」恰好漏掉这次点击本身，不能声称一次点按就新建。
  - 现在**不再调用** `controller.applyMiniBarStyle()`：页签栏与 MiniBar 的互斥展开交回官方 HDS 状态管理，应用不与用户展开底栏的动作抢控制权。
  - 加号点击统一收敛到 `PrimaryActionChannel`，不重复实现创建方法：展开态加号点击走 `primaryActionMiniBar().onClick`；折叠态被点开（`mode === USER_CLICK` + 前值 `COLLAPSE` + 现值 `EXPAND` + `tabBarStyle` 仍为 `COLLAPSE`，说明用户点的是 MiniBar 而不是底栏）视为同一次加号点击，在回调里派发一次当前页主操作。`NORMAL`（旋转/开关屏）与 `APP_TRIGGER`（代码主动切换）都不派发。
  - 因此新增非响应式字段 `previousMiniBarStyle`，在**每次**回调最前面「先取前值、再写当前值」（含提前返回的 `NORMAL`/`APP_TRIGGER` 分支），否则记录的前值会和 HDS 实际状态失步。
  - **未解除的不确定项**：折叠态点击与自绘 `Row.onClick` 是否会在**同一次指针事件**里都触发，SDK 文档没有承诺；真机若确认双触发，应在唯一主操作入口按同一输入事件去重（不靠延时过滤，也不把新建改成点两下）。真机未连接，本条交互仍未经目视确认。
- ⛔ **第二~五轮撤销收束（Issue #905 第六轮）**：以上关于 MiniBar 的宽度预算、折叠/点击语义、单击派发等结论**只记录当时把「+」放到 MiniBar 的错误分支**，现行代码已不存在 MiniBar 主操作；原生沉浸光感在**原右下角独立 FAB** 上仍不可用（真机 `Material inactive: out of scope`，官方生效范围只有标题栏与合规底部 TabBar），该点按未完成如实保留。
- 参数要点（照官方《组件适配沉浸光感》Button 一节）：材质样式取薄档 `ULTRA_THIN` / `THIN`；`materialColor` 必须带透明度，不透明纯色会把材质滤镜完全挡住；开了材质后不要再设 `backgroundColor` / 背景模糊 / `border`，它们会盖在材质层之上；THIN/ULTRA_THIN 时 `fontColor` 要用系统可反色资源（如 `sys.color.icon_primary`）才跟随反色；开了 `lightEffect` 后按钮默认点击态/悬浮态反馈由材质接管。
- 应用级开关：`entry/src/main/module.json5` 已配 `ohos.arkui.UIMaterial.state = "enable"`；ENABLE 下 Button 不会默认开启，必须显式传 `systemMaterial`。
- 说明：声明位于本机 SDK `openharmony/ets/api/@ohos.arkui.uiMaterial.d.ts`（`ImmersiveStyle { ULTRA_THIN=0, THIN=1, REGULAR=2 }`）与 `hms/ets/api/@hms.hds.hdsMaterial.d.ets`（`MaterialType { NONE=0, ADAPTIVE=100, IMMERSIVE=101 }`、`MaterialLevel { EXQUISITE=0, GENTLE=1, SMOOTH=2, ADAPTIVE=10 }`）。官方文档 docId：`开发指南/ArkUI_方舟UI框架/UI开发_ArkTS声明式开发范式/沉浸光感/沉浸光感开发指导/组件适配沉浸光感/arkts-immersive-light-sense-component-adaptation`、`…/沉浸光感常见问题/arkts-immersive-light-sense-faq`、`开发指南/UI_Design_Kit_UI设计套件/沉浸光感/ui-design-hds-component-material`、`FAQ/UI框架/UI界面/HarmonyOS下HdsNavigation与HdsTabs实现滚动模糊及沉浸光感材质效果的解决方案/faqs-arkui-1095`

### stackBuilder 自绘区的两个坑（`HdsAppDestinationApi23.ets` 实测）

- **HDS 返回按钮不会因为 stackBuilder 存在就自动避让**。它画在标题栏自己的层上，直接压住自绘标题（实测返回按钮面占 x 50~174px，星图详情页标题被完全盖住）。非根页必须自己让出槽位：`.padding({ left: isRoot ? 16 : 60, right: 16 })`（16 内边距 + 40 返回按钮 + 4 间距）。
- **返回按钮自带 label**：HDS 会把目标页标题当返回按钮文案画在圆形按钮里，标题一长就被裁成一两个字（实测星图详情页返回按钮里露出「1」）。置 `content.backIcon = { label: '' }` 清掉。
- stackBuilder 是自绘区域，**不吃 HDS 给 `mainTitle` 准备的内边距**，左右 16vp 要自己补，否则标题顶到屏幕左边缘（实测左边界 53px → 3px）。
- `stackBuilder` 的类型是 `CustomBuilder`，**里面的组件调用必须写在 `@Builder` 里**。直接在箭头函数体里写 `SomeComponent({...})` 会在运行时抛 `TypeError: class constructor cannot called without 'new'`（ArkTS 把它当普通函数调用了）。这条对所有 `CustomBuilder` 字段通用（`@BuilderParam`、`stackBuilder`、类型为 `CustomBuilder` 的属性）。

### 页面内容流里的材质与阴影（实测）

- 材质挂到内容流的 Button 上，**hilog 会打 `Material inactive: out of scope`，且深色模式下材质确实不渲染**。干净对照（重启 app、无对话框、无触摸）：作品页 FAB 静止 0 级（圆盘内部与纯黑背景同色，只有加号字形 219），按住同为 0 级。**即内容流里拿不到材质的折射/高光，也没有按压光。**
- **「按压有响应」是测量假象，不要再当结论**：早前几轮测到的 0→61 / 0→109，实际来源是「新建作品」对话框的遮罩把整个 FAB 抬到 109 级，以及 `uinput -T -d` 未配对 `-u` 造成的卡住按压态（同样是 109）。判据：值在整个圆盘内均匀、与页面其它元素同步变化 → 是遮罩不是按压光。
- **抓帧必须用 PNG**：`devecocli ui screenshot` 出 PNG；`hdc shell snapshot_display` 出的是 JPEG，在纯黑底上会在字形周围产生 100~110 级的压缩振铃，看起来像材质亮起（`hilog | grep` 之外还要看这一步）。按住用 `uinput -T -d x y` 后台 + `uinput -T -u x y` 收尾，漏掉 `-u` 会让组件停在按压态。
- 深色模式纯黑底上，材质静止态几乎全透明，圆盘边界看不见；这是材质本身的特性，不是属性被谁删了。
- **材质阴影与自定义 shadow 在深色纯黑底上都不显**：材质背景层透明，`shadow` 按组件自身背景形状外投，背景透明则无物可投。
- 官方对替换关系的明确表述只覆盖阴影：「沉浸式系统材质默认自带阴影效果（`applyShadow` 为 true），优先于 `shadow` 通用属性，此时自定义的 `shadow` 设置不会生效。如需使用自定义阴影，将 `applyShadow` 置为 false 后再设置 `shadow`。」**`border` 没有对应的替换通道**，自定义 `border` 会盖在材质层之上。
- `materialColor` 的官方语义：「对所有档位的算力设备均生效。在高算力和中算力设备上，该参数为材质滤镜再混合一层纯色效果；在低算力设备上，该参数作为背景色 backgroundColor 属性值。」给它不透明纯色会遮挡材质滤镜（实测页头胶囊传 `#33FFFFFF` 糊成一片发灰起雾的乳白，与底栏的通透感差 3 倍以上）。

### 让 FAB 在深色模式有可见边界：试过的全部路径

`ui/components/PrimaryFab.ets`。按尝试顺序，每条都有实机数据：

| 方案 | 实测结果 | 结论 |
|---|---|---|
| `backgroundBlurStyle(BlurStyle.Thin)` + `shadow(ShadowStyle.OUTER_DEFAULT_XS)` + 半透明面 + 发丝描边 | 圆盘 20~22 级，边界清晰，阴影 3 级与底栏胶囊一致 | 稳定可读；**Issue #905 第七轮恢复为现行做法**（无沉浸材质跟手追光） |
| `systemMaterial(floatingMaterial(false))` + 自定义 `shadow(OUTER_DEFAULT_XS)` | 静止 0 级、圆盘不可见；`shadow` 完全没出来 | 材质背景层透明 → 无物可投 |
| `systemMaterial(floatingMaterial(true))`（材质自带阴影） | 静止 0 级，材质阴影在纯黑底上不显 | 同上 |
| `systemMaterial(...)` + `stateEffect: false` | 按压无响应（干净对照） | 内容流拿不到按压光 |
| `materialColor: '#33FFFFFF'` | 页头胶囊糊成发灰起雾的乳白，比底栏差 3 倍以上 | 违反「materialColor 必须透明」 |
| `hdsEffect.pointLight`（`PointLightSourceType` / `PointLightIlluminatedType` 全枚举） | native 每次报 `HDS_hdsbase: [42]Wrong argument type. int32 expected.`，无任何光 | 本机不可用，别再试 |
| `hdsEffect.pressShadow(BLEND_GRADIENT)` | 整面叠白（中心 85% 白、边缘 100% 白），白底上削顶到 255 看不出；深色底上整颗刷白、白字消失 | 不是跟随手指的光，且会吃掉白色 + 号 |
| `hdsEffect.pressShadow(BLEND_WHITE)` | 15% 白叠加，按压可见且 + 号仍清晰 | **Issue #905 第七轮采用**：系统渐变按压高光，非跟手反射光 |
| `lightUpEffect(0..1)` | 只有整体亮度，无位置参数 | 死路 |
| **独立边缘层 1vp `border('#22FFFFFF')` + `hitTestBehavior(None)`（Issue #811 方案，已被 Issue #905 取代）** | 描边像素值 60，与 `#22FFFFFF` 的 13% 白相符；截图可见清晰细圆环 | 当时可用；#905 后不再拆「Button + 独立描边层」两套结构 |
| **固定 `border(1vp, sys.color.icon_secondary)`（#905 第一~七轮）** | 圆环可见，但**颜色固定**，不随背景变化 | 当作「深色模式最低轮廓」可用；**不是反射光**，第八轮已替换为下面的反色混色环 |
| **`Circle().stroke('#66FFFFFF')` + `blendMode(BlendMode.DIFFERENCE, BlendApplyType.FAST)` + `hitTestBehavior(None)`（Issue #905 第八轮，Issue #937 已移除）** | 描边与下层画面做差值混合，圆环颜色随按钮底下那圈画面变化 | 随**本地背景像素**变化，但没有方向与衰减，无法按光源位置形成亮暗分区；#937 重做玻璃表面时删除，不再叠加第二套边框 |
| **Canvas 圆环 conic 渐变描边边缘高光 + 极低不透明度系统色表面层（Issue #937 现行）** | 圆周 1vp 圆环用 `createConicGradient` 描边分亮度：左上方向最亮、沿圆周向两侧衰减、其余位置保留很轻的基础轮廓（同一套环形视觉）；「+」符号画在最上层，不被装饰层覆盖，配合毛玻璃与 `pressShadow` 形成完整玻璃按钮 | 方向/强度支持由 `lightDirectionTurns` / `lightIntensity` 入参驱动（`@Monitor` 变化时清理并重画同一个 56vp `Canvas`）；当前没有可确定的真实配对光源（HDS 底栏选中高亮为内部渲染、不可测，也不假称它自动传给内容层按钮），默认保持静态约定方向；**静态方向渐变高光只能称作玻璃光泽，不是实时反射/点光源**；实机目视观感待确认 |

现行结构：`Stack { Row(边缘层 1vp 描边, hitTestBehavior(None)); Button(ImmersiveMaterial + lightEffect, 不画边) }`。描边必须放独立节点，压在 `systemMaterial` 的 Button 上会挡掉材质层。

**Issue #937 现行结构（内容流右下角唯一主操作载体）**：`PrimaryFab.ets` 是同一尺寸的 `Stack { Button(毛玻璃主体，唯一可点击) + 极低不透明度系统色表面层 + Canvas 圆环边缘高光层 + SymbolGlyph「+」(最上层，hitTestBehavior(None)) }`，位置即原设计位置（`Alignment.BottomEnd` + 原 `PRIMARY_FAB_BOTTOM_INSET_VP`），三个页面共用：
- 表面：透明 `backgroundColor` + `backgroundBlurStyle(BlurStyle.Thin)`（可透视毛玻璃）+ `shadow(ShadowStyle.OUTER_DEFAULT_XS)`；另加一层极低不透明度的系统色圆形表面（`sys.color` 前景色低透明度），让浅色/深色/纯色背景下都有稳定可见的体积感，不盖住模糊。**不再有写死的固定色 `border`**。
- 边缘高光：独立 `Canvas`（`hitTestBehavior(HitTestMode.None)`）在圆周描 1vp 圆环，`strokeStyle` 用 `createConicGradient` 生成的方向性渐变——左上方向高光带、沿圆周向两侧衰减、其余位置保留轻基础轮廓，高光与基础轮廓是同一套环形视觉；方向与强度取自固定的 UI 约定光源方向（左上），不是实时追光。原来的 `BlendMode.DIFFERENCE` 反色描边环已删除，不保留第二套边框，也不要把它称作「光感已复刻」；「+」符号从 Button 内部拆出、作为根 `Stack` 最后一层画在玻璃装饰层之上（`hitTestBehavior(HitTestMode.None)`），避免被表面层/高光层覆盖，唯一可点击的始终只有 `Button`。峰值方向与强度已支持入参：`lightDirectionTurns` / `lightIntensity`（默认保持静态约定值），入参变化时由 `@Monitor` 清理并重画同一个 56vp `Canvas`，不重建组件、不整页重绘。
- 按压：`stateEffect: false` + `.visualEffect(new hdsEffect.HdsEffectBuilder().pressShadow(...).buildEffect())`，按下 `BLEND_WHITE`、抬起/取消一起解除发光与底栏手势锁。
- **不挂 `systemMaterial`**（内容流不在官方生效范围，真机 `Material inactive: out of scope`）；**不接 `hdsEffect.pointLight`**（native 拒收、且 API 26 元数据里非公开接口，见上文）。
- 因此唯一仍缺的是原位置上的**沉浸材质跟手追光**与「由真实邻近光源驱动、随光源位置/强弱变化的动态边缘受光」，这两项按未完成如实保留：普通毛玻璃、方向性静态渐变高光与 pressShadow 都是真实可用的普通能力，但静态渐变高光只能叫玻璃光泽，不得写成已实现动态受光/实时反射；且整体观感**未经真机目视确认**（`hdc list targets` 为空）。动态受光的输入机制已就位（入参 + 重画），但当前三页附近没有可确定真实坐标与强度的发光组件，实际受光来源仍缺，先保持静态默认值；等出现真实配对光源时由调用方换算方向/强度传入即可接通。
上面 `Stack + 独立描边层 + Button(ImmersiveMaterial)` 是 Issue #811 的旧方案，已被 #905 取代。作品页 / 工作区 / 星图列表三个页面的主操作「+」在 API 23+ 窄屏与宽屏**都**用这个内容流右下角 FAB，不再有任何 MiniBar 分支。

**~~#905 复核更正（第二轮）~~：已撤销并回滚**。曾经把 API 23+ 窄屏的主操作「+」改由底部 `HdsTabs` MiniBar 承载（`isHdsMiniBarHostingPrimaryAction()` 互斥），该改造已被复核推翻——MiniBar 与页签栏同排，不是原右下角 FAB 的位置，为拿材质改位置属于偷换需求；`PrimaryActionChannel` / `miniBar` / `isHdsMiniBarHostingPrimaryAction()` 已从代码删除，三个页面恢复各自的内容流 `PrimaryFab`。`ui.primary_action_requested` 诊断事件（#906 引入）已迁到唯一真实点击入口 `PrimaryFab.handleClick()`（`origin=User`、`source=content_fab`，仅在未拖动且实际调用 `onTap` 时记录）。**未完成项保留**：原右下角独立 FAB 上的原生沉浸光感在公开 SDK 下仍无法实现（真机 `Material inactive: out of scope`；官方生效范围只有标题栏与合规底部 TabBar），不得用「搬到 MiniBar」或普通按钮宣称已补回。

**边缘层 `hdsEffect.pointLight(DEFAULT_FEATHERING_BORDER)` 与第七轮的点光源接入均未采纳**：本机 native 层对 `pointLight` 一律回 `Wrong argument type. int32 expected.`（见上表与对应说明），加上去只会每次刷一条错误而没有光，还会连累同一 `.visualEffect` 里的 `pressShadow`。Issue #937 复核确认：API 26 SDK 元数据（`hms/ets/api/device-define/api-version/UIDesignKit.json`）中 `pointLight` 及 `PointLightEffect` / `PointLightOptions` / 两个枚举全部 `publicApi: ""`（非公开接口），只有 `pressShadow` / `buildEffect` 是公开 API，与本机 native 行为一致——**属能力边界，FAB 重做不以它为前提，也不再做任何试验性接入**。

## 菜单 / 气泡菜单（bindMenu）

- Kit：`@kit.ArkUI`
- 接口：
  - 声明式：`.bindMenu(content: Array<MenuElement> | CustomBuilder, options?: MenuOptions)`
  - 程序化：`.bindMenu(isShow: boolean, content: Array<MenuElement> | CustomBuilder, options?: MenuOptions)`
- 最低 API：声明式 12（随 UI 组件基础能力），程序化重载 11
- SystemCapability：无独立声明（随 `@kit.ArkUI` 整体可用）
- 权限 / ACL：无
- fallback：无 fallback 路径（最低安装基线 API12 已覆盖声明式重载）
- 实现文件：`feature/starmap/ui/StarMapScene.ets`、`feature/starmap/ui/StarMapListScreen.ets`、`feature/project/ui/HomeScreen.ets`
- 用法约束（Issue #813）：
  - 递归 UI 里（星图 Scene 层层嵌套、圆形 Embed 带 `clip(true)`）**不要**用 `Column + Button + .position()` 自绘菜单：自绘弹层是 Scene 的普通子组件，会被祖先的 `clip(true)` 裁掉，也会因为用 Scene 局部坐标定位而越出屏幕。改用程序化 `bindMenu(isShow, ...)`，由系统定位并自动避让屏幕边缘。
  - **不要**用 `bindContextMenu(isShown, ...)`：官方文档说明程序化 `bindContextMenu` 不支持"长按后继续拖动"，会破坏"长按出菜单、继续移动拉线"的连线交互（见 #373）。`bindMenu` 只负责显示，长按与后续 Pan 仍由业务自己的 `GestureGroup` 状态机负责，两者互不冲突。
  - 菜单上下文（如"当前长按的是哪个节点"）通过组件字段传给无参 `CustomBuilder`，不要给 `@Builder` 加参数。
  - `bindMenu(isShow, ...)` 按**被绑定组件**定位。要让菜单贴着用户手指弹出，就在场景里放一个 1vp×1vp、`hitTestBehavior(HitTestMode.None)` 的透明锚点，长按时把锚点 `position()` 移到手指的屏幕坐标，再把 `bindMenu` 挂在这个锚点上。直接把 `bindMenu` 挂全屏背景层会让菜单相对整张场景定位，长按位置的信息就丢了（Issue #813 复审）。
  - 程序化 `bindMenu` 的 `isShow` **不会**自动双向回写。必须传 `MenuOptions.onDisappear` 把显示标记写回 `@State`，否则用户点菜单外部关掉菜单后状态仍是 `true`，下一次长按同一对象因为状态没变化而弹不出来。`onDisappear` 里不要无脑 reset 业务手势：长按后继续拖动（进 connect）和点"移动"（进 moveNode/moveEmbed）也会主动关菜单，只在业务仍处于"菜单待选"状态时才收尾（Issue #813 复审）。

## 触摸事件拦截（onTouchIntercept）

- Kit：`@kit.ArkUI`
- 接口：`.onTouchIntercept(callback: Callback<TouchEvent, HitTestMode>): T`，在命中测试阶段决定本组件的命中行为
- `TouchEvent.changedTouches: TouchObject[]`，`TouchObject.x / y` 是**相对本组件**的局部坐标
- 最低 API：12
- SystemCapability：`SystemCapability.ArkUI.ArkUI.Full`
- 权限 / ACL：无
- 用途：父组件按自己的几何形状动态决定"哪些区域我吃掉、哪些区域放给子组件"。
  典型场景是圆形子视图 —— 用若干根矩形窄条 + `clip(true)` 裁圆环是**不成立**的：裁出来的只是圆内几段直线，斜着点边框（例如 45°）既不在窄条上也不在标题条上，事件会掉进子视图，出现"几何算出来命中父级、实际却进了子级"的错位。
- **返回值语义**（SDK `enums.d.ts` 原文，务必区分）：
  - `Default`：当前组件**和它的子节点都参加**触摸测试，只挡兄弟节点，不影响祖先。父 Embed 用它等于"父和 child Scene 一起进手势竞争"，**不是**独占。
  - `Block`：当前组件参加，**同时挡掉子节点、兄弟节点和祖先节点**。产品语义定死"这块区域只归父组件"时必须用它。
  - `None`：当前组件和子节点都不参加，也不影响祖先 —— 事件继续往下/往外传。
  - 圆形 Embed 的实际分流：圆外 `None`、圆内 title/圆环 `Block`、圆内其余 `None`。
- 实现约束：`onTouchIntercept` 里 title/圆环的判定阈值必须 import
  `StarMapGeometry` 导出的 `EMBED_TITLE_HIT_HEIGHT` / `EMBED_BORDER_HIT_WIDTH`，不许在 UI 侧另抄一份。
  两份数值一旦不同，就会出现"手指按这一圈是父级边框、把线拖到同一点松手却判成子图内部"的
  命中与几何错位。这两个常量定义的是**交互热区**（圆环 12vp），不是视觉描边宽度（1/3vp）。
- 实现文件：`feature/starmap/ui/StarMapScene.ets`（Embed 圆形外壳的 title / 圆环分流）

## 自定义触摸热区（responseRegion）

- Kit：`@kit.ArkUI`
- 接口：`.responseRegion(value: Array<Rectangle> | Rectangle): T`
  - `Rectangle` 的 `x / y / width / height` 都是 `Length`，含义是**相对组件左上角**的坐标
  - **默认值就是整个组件**（`{x: 0, y: 0, width: '100%', height: '100%'}`）
- 最低 API：8
- SystemCapability：`SystemCapability.ArkUI.ArkUI.Full`
- 权限 / ACL：无
- 用途：非矩形（圆、圆角、多边形）子视图必须自己声明真实热区。`borderRadius` + `clip(true)`
  **只裁视觉、不裁热区**，不声明的话圆外四个方角依然可点，会出现"看起来是圆、实际整块方块都能选中"。
  官方文档同时说明：父组件 `clip(true)` 时子组件响应受**父组件触摸热区**限制，所以父级裁剪区也应当是真实形状。
- 实现方式：把圆按水平带切成若干矩形，每条带的半宽取该带**上下两条边所在高度处**圆半宽
  `sqrt(r² - dy²)` 的**较小值**（内切），不是带中心高度：
  - 取带中心会在极点附近外凸（直径 200、24 带时可达 28vp），等于"点在圆外却选中了子星图"，
    和不声明热区是同一类错位；
  - 取内切值保证永外不越界，代价是极点处半宽为 0 的最外侧一条被丢弃，圆帽高度 = 带高。
  - 带数按 `ceil(直径 / MAX_CIRCLE_RESPONSE_BAND_HEIGHT_VP)` 推导，**不写死带数**：
    写死会让圆帽高度 `直径/带数` 随 zoom 放大（3x、直径 600、32 带 → 上下各 18.75vp 看得见点不到）。
    固定最大带高后，0.3x / 1x / 3x 的圆帽都只有这一个固定上限。热区矩形数量随缩放线性增长是已知代价。
  - 纯几何在 `feature/starmap/platform/StarMapGeometry.ets` 的 `buildCircleResponseBands`（可测），UI 只做 `Rectangle` 转换。
- 实现文件：`feature/starmap/ui/StarMapScene.ets`（`buildEmbedCircleResponseRegions`）

## 自定义对话框（ComponentContent + openCustomDialog）

- Kit：`@kit.ArkUI`
- 接口：
  - `new ComponentContent<T>(uiContext: UIContext, builder: WrappedBuilder<[T]>, args: T)`
  - `UIContext.getPromptAction().openCustomDialog(content: ComponentContent<T>, options?: promptAction.BaseDialogOptions)`
  - `UIContext.getPromptAction().closeCustomDialog(content: ComponentContent<T>)`
- 最低 API：12（`stagemodelonly`；DevEco Previewer 不支持）
- SystemCapability：`SystemCapability.ArkUI.ArkUI.Full`
- 权限 / ACL：无
- fallback：无 fallback 路径（最低安装基线 API12 即为该 API 的 since 版本）
- 实现文件：`feature/starmap/ui/CreateTitleDialog.ets`（对话框 UI + `createTitleDialogContent` 工厂）、`feature/starmap/ui/StarMapScene.ets`（打开 / 关闭与生命周期清理）
- 用法约束（Issue #813）：
  - 递归 UI 里被祖先 `clip(true)` 裁掉、或需要脱离 Scene 局部坐标定位的对话框，挂到窗口层级：`getPromptAction().openCustomDialog(...)`，不要作为 Scene `Stack` 的普通子组件。
  - `wrapBuilder(...)` 收到的不接受 `this.xxx` 形式的 `@Builder` 成员方法引用（ArkTS 报 "The wrapBuilder's parameter should be '@Builder' function"）。要用**文件级** `@Builder function`，在函数体里转交给一个独立 `@Component` 组件，由组件自己持有输入状态；关闭时用同一个 `ComponentContent` 实例调 `closeCustomDialog`。
  - `promptAction.BaseDialogOptions` 没有 `backgroundColor` / `width` / `height` 字段：对话框内容完全跟随自定义组件样式，背景与遮罩分别由内容自身的 `backgroundColor` 和 `maskColor` 控制。

## 系统日期选择（UIContext.showDatePickerDialog）

- Kit：`@kit.ArkUI`（`UIContext.showDatePickerDialog(options: DatePickerDialogOptions)`；全局 `DatePickerDialog.show(options)` 自 API 18 起废弃，本工程不使用）
- 接口：
  - `UIContext.showDatePickerDialog(options: DatePickerDialogOptions): void`（since 10，`@stagemodelonly`）
  - `DatePickerDialogOptions.selected?: Date` 初始选中日期；`onDateAccept?: Callback<Date>` 用户确认后的回调（`onAccept` 已废弃）
- 最低 API：10（高于最低安装基线 12，实际始终可用）
- SystemCapability：`SystemCapability.ArkUI.ArkUI.Full`
- 权限 / ACL：无
- fallback：无 fallback 路径（最低安装基线 API12 已覆盖该接口）
- 实现文件：`feature/stats/ui/StatsScreen.ets`（统计页「自选」区间依次弹开始 / 结束日期选择）
- 语义边界：确认后的 `Date` 按用户所选本地日历日格式化成 `YYYY-MM-DD` 原样传给 Core，不在端侧把未来结束日改写成今天、不自行裁剪区间；Core 返回的错误在统计页内显示。

## 应用共享目录 / 捐献沙箱目录（shareFiles profile）

- Kit：无独立 Kit（`module.json5` 的 `shareFiles` 标签 + `resources/base/profile/share_files.json`）
- 配置项：
  - `module.json5`：`"shareFiles": "$profile:share_files"`
  - `share_files.scopes[].path` / `permission`（`r` 只读 / `r+w` 读写）—— 应用沙箱共享目录（API23+）
  - `share_files.sharingOSPath` / `sharingOSSubpath` / `sharingOSPermission` —— 捐献给操作系统的沙箱目录（API26+）
- 最低 API：23（共享目录 `scopes`）；26.0.0（捐献目录三个 `sharingOS*` 字段）
- SystemCapability：无独立 SystemCapability（模块级配置）
- 权限：无
- ACL：否
- 实现文件：`apps/harmony/entry/src/main/resources/base/profile/share_files.json`（工程资源，不在 `platform/` 下）

### 路径分流语义

shareFiles profile 的路径配置分两层：`scopes` 控制共享文件范围，`sharingOS*` 控制目录捐献。两者路径必须对齐。

- **scopes（API23+）**：profile 路径写 `/el2/base/files`。SDK 26.0.0 的 `modulecheck/shareFiles.json` schema 用 `^/(?:el1|el2|el3|el4|el5)/(?:base|distributedfiles|cloud)...` 校验 `scopes[].path`，第一级必须是 `el1~el5`，第二级必须是 `base`/`distributedfiles`/`cloud`。`/el2/base/files` 是 shareFiles profile 的合法逻辑路径（不是运行时沙箱绝对路径 `/data/app/el2/.../base/files`），系统在运行时自动映射到应用沙箱。写成 `/base/files` 不匹配 schema，PreBuild schema validate 会 BUILD FAILED。
- **sharingOSPath / sharingOSSubpath（API26+）**：`sharingOSPath` 必须和 `scopes` 中已配置的 path 对上（即 `/el2/base/files`），再用 `sharingOSSubpath` 选真正捐献给操作系统的子目录（如 `/diagnostics`）。这样 HarmonyOS 7 的文件管理器可以浏览 `diagnostics` 子目录，而不会暴露整个 `/el2/base/files`。
- **版本行为**：
  - HarmonyOS 7（API26+）：文件管理器可浏览 `diagnostics` 目录，用户直接看到诊断包文件。
  - API12~25：保持现有系统分享入口（ShareKit），不假装支持目录浏览。低版本系统忽略 `sharingOS*` 配置，不影响应用正常运行。

### 当前配置

```json
{
  "share_files": {
    "scopes": [
      { "path": "/el2/base/files", "permission": "r" }
    ],
    "sharingOSPath": "/el2/base/files",
    "sharingOSSubpath": "/diagnostics",
    "sharingOSPermission": "r"
  }
}
```

- `scopes` 声明 `/el2/base/files` 只读共享（profile 合法逻辑路径，系统运行时映射到应用沙箱）
- `sharingOSPath` = `/el2/base/files`（与 scope 对齐），`sharingOSSubpath` = `/diagnostics`（只捐献诊断子目录，不暴露整个 filesDir）
- `sharingOSPermission` = `r`（只读，是 scope permission 的子集）
- Issue #776 评论5848626733：`sharingOSSubpath` 从 `/share` 改为 `/diagnostics`，只把诊断目录捐给文件管理器；`permission` 从 `r+w` 改为 `r`，只读共享。

### 门禁

`tools/check_harmony_share_files.py`（自测 `tools/test_check_harmony_share_files.py`），在 harmony workflow 里跑。用 SDK 自带 schema 正则 + 官方路径限制同时校验：`scopes[].path` 必须匹配 `^/(?:el1|el2|el3|el4|el5)/(?:base|distributedfiles|cloud)...`，`sharingOSPath` 与 scope path 对齐，`sharingOSSubpath` 是 scope path 的子目录，`sharingOSPermission` 是 scope permission 子集。

## 平板专用横屏窗口政策（TabletWindowPolicy）

- Kit：`@kit.ArkUI`（`display` + `window` 模块）、`@ohos.deviceInfo`
- 接口：
  - `display.isFoldable(): boolean` — 判断设备是否可折叠（API10+）
  - `display.getDefaultDisplaySync(): Display` — 同步获取默认 Display 信息（宽高、密度等）
  - `window.Window.setPreferredOrientation(orientation: window.Orientation): Promise<void>` — 设置窗口首选方向（API9+）
  - `window.Window.setWindowLayoutFullScreen(isFullScreen: boolean): Promise<void>` — 设置窗口是否铺满屏幕（API12+）
  - `window.Window.setWindowSystemBarEnable(names: Array<'status' | 'navigation'>): Promise<void>` — 设置系统栏可见性（API12+）
  - `window.Window.getWindowStatus(): window.WindowStatusType` — 读当前窗口状态（API12+）
  - `window.Window.on('windowStatusChange', callback: Callback<window.WindowStatusType>)` / `off` — 全屏 ↔ 分屏/悬浮切换（API11+）
  - `deviceInfo.deviceType: string` — 设备类型（phone / tablet / wearable 等）
- 最低 API：12（`setWindowSystemBarEnable` / `setWindowLayoutFullScreen` / `getWindowStatus`）；11（`windowStatusChange`）；10（`display.isFoldable`）；9（`setPreferredOrientation`）
- SystemCapability：
  - `display.isFoldable` / `getWindowStatus` / `windowStatusChange`：`SystemCapability.Window.SessionManager`
  - `setPreferredOrientation` / `setWindowSystemBarEnable` / `setWindowLayoutFullScreen`：无独立 SystemCapability（随 `@kit.ArkUI` window 模块整体可用）
- 权限：无
- ACL：否
- fallback：读取设备特征失败时默认不强制（`isDedicatedLandscapeTablet()` 返回 false），不伪造状态；`windowStatusChange` 注册失败只记日志，不影响布局
- 实现文件：`window/TabletWindowPolicy.ets`（政策本体）、`ui/adaptive/AdaptiveContext.ets`（窗口状态监听与调用）
- 判定逻辑：
  1. `deviceInfo.deviceType === 'tablet'` 是普通平板候选
  2. `display.isFoldable() === false` 排除所有可折叠设备（阔折叠、普通折叠、三折叠）
  3. `display.getDefaultDisplaySync()` 宽高比 `max(w,h)/min(w,h) >= 1.25` 区分明显宽于正方形的常规平板
  4. 仅命中普通平板时：`setPreferredOrientation(LANDSCAPE)` + `setWindowLayoutFullScreen(true)` + `setWindowSystemBarEnable([])`
  5. 普通手机、阔直板、阔折叠、其他折叠形态不执行强制政策
- 设备能力只算一次并缓存；窗口尺寸变化不影响 `isDedicatedLandscapeTablet()` 判定
- 调用时序：`EntryAbility.onWindowStageCreate` / `onWindowStageRestore` 中 `loadContent` 之前，通过 `windowStage.getMainWindowSync()` 取得主窗口调用 `applyTabletWindowPolicy(mainWindow)`
- 窗口状态变化：`AdaptiveContext` 初始化时用 `getWindowStatus()` 记录当前状态，并监听 `windowStatusChange`。
  只有回到独占全屏（`WindowStatusType.FULL_SCREEN = 1`）才重新 `applyTabletWindowPolicy()`；
  进入分屏（`SPLIT_SCREEN = 5`）/ 悬浮（`FLOATING = 4`）/ 最大化（`MAXIMIZE = 2`）调用 `restoreWindowPolicy()` 恢复方向与系统栏。
  冷启动套用前也先读一次 `getWindowStatus()`：分屏 / 悬浮 / 最大化直接跳过，不把全屏政策硬套到非全屏窗口上；
  `UNDEFINED = 0`（窗口状态尚未就绪）与读取失败保守按全屏处理，不阻断原有启动路径。
  `WindowStatusType` 取值：UNDEFINED=0、FULL_SCREEN=1、MAXIMIZE=2、MINIMIZE=3、FLOATING=4、SPLIT_SCREEN=5。
  分屏 / 悬浮下的布局仍由 `windowSizeChange` 的真实 `windowRect` 驱动，不缩放、不虚构窗口尺寸。
  非专用横屏平板（阔折叠、阔直屏、手机）不调用这两个接口。
- 官方文档：
  - 华为旋转说明：https://developer.huawei.com/consumer/cn/doc/doccenter-capabilities/window-rotation
  - 官方沉浸式窗口：https://developer.huawei.com/consumer/cn/doc/doccenter-capabilities/immersive-window-feature
  - 官方自由窗口：https://developer.huawei.com/consumer/cn/doc/HarmonyOS-Guides/freeform-window
  - 官方折叠判别：https://developer.huawei.com/consumer/cn/doc/doccenter-dev-faq/faqs-purax-12

## 网络状态（hasDefaultNet / isDefaultNetMetered）

- Kit：`@kit.NetworkKit`（`connection` 模块）
- 接口：
  - `connection.hasDefaultNet(): Promise<boolean>` — 查询是否有默认激活的数据网络（API12+）
  - `connection.isDefaultNetMetered(): Promise<boolean>` — 查询默认网络是否按流量计费（API12+）
- 最低 API：12
- SystemCapability：`SystemCapability.Communication.NetManager.Core`
- 权限：`ohos.permission.GET_NETWORK_INFO`（system_grant，normal，`provisionEnable=false`，**不需要 ACL**）
- fallback：查询失败时保持默认值（在线、非计费），不阻塞初始化；不伪造网络状态
- 实现文件：`corebridge/NativeWriterCoreBridge.ets`（`initialize()` 方法中查询真实网络状态并传递给 Rust `writer_core_init_harmony`）
- 语义边界：
  - `is_connected`：`hasDefaultNet` 返回 `true` = 在线，`false` = 离线
  - `is_metered`：`isDefaultNetMetered` 返回 `true` = 计费网络（蜂窝/个人热点），`false` = 非计费网络（WiFi/以太网）
  - 无默认网络时 `isDefaultNetMetered` 不调用，`is_metered` 保持默认 `false`
- 说明：网络状态用于 Core 侧 `PlatformServices` 的 `NetworkState`，影响同步传输工厂是否允许后台同步（计费网络下可能限制大流量同步）。官方文档：https://developer.huawei.com/consumer/cn/doc/doccenter-references/api/capi-net-connection-h

## 短时任务（退后台保存）

- Kit：`@kit.BackgroundTasksKit`（`backgroundTaskManager` 模块）
- 接口：
  - `backgroundTaskManager.requestSuspendDelay(reason: string, callback: Callback<void>): DelaySuspendInfo` — 申请短时任务；`callback` 一般在实际超时前约 6 秒被调用（用于收尾）
  - `backgroundTaskManager.cancelSuspendDelay(requestId: number): void` — 保存事务结束后释放
  - `DelaySuspendInfo { requestId: number; actualDelayTime: number }` — 实际时长由系统决定（正常最长 3 分钟，低电量 1 分钟）
- 最低 API：9（`requestSuspendDelay` / `cancelSuspendDelay` 均 `@since 9`，经本机 SDK d.ts 确认；compatibleSdkVersion=12 始终满足）
- SystemCapability：`SystemCapability.ResourceSchedule.BackgroundTaskManager.TransientTask`
- 权限：无（d.ts 未声明 `@permission`；短时任务不需要权限，也不进 `module.json5` 的 `requestPermissions`）
- ACL：否
- fallback：平台不支持（`canIUse` 为 false）或申请短时任务抛错时，`runSave` 不调用保存 action、返回 false 并记录 `background_save_unavailable` 诊断，不伪造成功；保存 action 返回 false 时由 `WritingWorkflowCoordinator` 保留 dirty / `lastSaveFailed`。超时回调记录 `background_save_timeout`。诊断走 `diagnostics/HarmonyDiagnosticEvents.ets` 通道。
- 实现文件：`lifecycle/HarmonyBackgroundSaveTask.ets`（平台短时任务包装，UI 页面不直接调用后台任务接口）
- 调用方：`lifecycle/HarmonyLifecycle.ets` 的 `AutoSaveLifecycleObserver.onBackground()` —— 申请到 requestId 后才开始异步落盘，`try/finally` 释放；单一 in-flight 标志保证 `onWindowStageDestroy` + `onBackground` 连续触发只产生一次后台保存事务。前台 `setInterval` 定时保存不使用短时任务。
- 官方文档：https://developer.huawei.com/consumer/cn/doc/harmonyos-references-V14/js-apis-resourceschedule-backgroundtaskmanager-V14
