# HarmonyOS 平台能力矩阵

本文件记录素笺 HarmonyOS 客户端实际使用的每个系统能力：Kit、接口、最低 API、SystemCapability、权限、ACL 需求、fallback 策略和实现文件。新增系统能力前先查 HarmonyOS 官方 API 和本机 SDK d.ts（`/opt/devecostudio/sdk/default`），再在此登记。

实现文件路径相对于本文件所在目录 `entry/src/main/ets/platform/`（即下表与正文中的路径不以 `platform/` 开头，直接从子目录名写起）。

## 应用最低安装基线与版本分流

- **最低安装基线**：`compatibleSdkVersion = 12`（HarmonyOS NEXT 5.0.0(12)）。这是应用能安装运行的最低系统版本，由 `apps/harmony/build-profile.json5` 的 `compatibleSdkVersion` 表达，只反映真实需要支持的最低版本，不因开发机/测试机使用 API26 SDK 就一起抬到 API26。
- **编译目标**：`targetSdkVersion = 26`（HarmonyOS 7）。工程使用 API26 SDK 构建，`targetSdkVersion` 保持 API26，与 `compatibleSdkVersion` 不必为同一 API。
- **能力分流**：API20 / API23 / API26 等高版本能力不抬高安装下限，而是通过 `PlatformApiResolver`（`version/PlatformApiResolver.ets`）按运行时 API Level 分流到各 `impl/apiXX` facade：
  - `impl/api11`：普通系统分享（systemShare，since 11）
  - `impl/api12`：碰一碰分享（knockShare 基础重载，since 12）
  - `impl/api20`：握姿感知、防窥基础、隔空传送（gesturesShare，since 20）
  - `impl/api23`：防窥扩展（requestAntiPeepOptions，since 23）
  - API26 新能力继续单独使用
- **降级语义**：旧系统缺少某个高版本能力时，只降级该能力（`isSupported()` 返回 false、对应入口返回 false，不伪造状态），**不导致整个应用无法安装**。API12～25 的设备可以正常安装运行，只是按运行时版本禁用对应高版本能力。
- 稳定 facade（`*Service.ets`）不直接 import 版本敏感的高版本能力 Kit、不自行判断具体 API Level；Ability Context（`@kit.AbilityKit` 的 `common`）等基础平台类型可以保留。具体高版本系统调用和版本判断收进 `impl/apiXX`，由 `PlatformApiResolver` 按运行时 API Level 分流。

## 能力总览

| 能力 | Kit | 最低 API | SystemCapability | 权限 | ACL | 实现文件 |
|------|-----|---------|------------------|------|-----|---------|
| 握姿感知 | @kit.MultimodalAwarenessKit (motion) | 20 | SystemCapability.MultimodalAwareness.Motion | ohos.permission.DETECT_GESTURE | 否 | awareness/impl/api20/GripPostureApi20.ets |
| 防窥屏 | @kit.DeviceSecurityKit (dlpAntiPeep) | 20（基础）/ 23（requestAntiPeepOptions） | SystemCapability.Security.DlpAntiPeep | ohos.permission.DLP_GET_HIDE_STATUS | 是 | privacy/impl/api20/DlpAntiPeepApi20.ets, privacy/impl/api23/DlpAntiPeepApi23.ets |
| 窗口隐私 | @kit.ArkUI (window) | 未限定独立 API（随 @kit.ArkUI window 模块） | 未限定独立 SystemCapability（随 @kit.ArkUI window 模块） | 无 | 否 | privacy/WindowPrivacyService.ets |
| 应用接续 | 无独立 Kit（Ability 生命周期 + module.json5 continuable 配置） | 未限定独立 API（Staged 模型 ability 级配置） | 无独立 SystemCapability（Ability 级配置） | 无 | 否 | continuity/AppContinuationService.ets |
| 普通系统分享 | @kit.ShareKit (systemShare) | 11 | SystemCapability.Collaboration.SystemShare | 无 | 否 | share/impl/api11/SystemShareApi11.ets, share/SystemShareService.ets |
| 碰一碰分享 | @kit.ShareKit (harmonyShare knockShare) | 12 | SystemCapability.Collaboration.HarmonyShare | 无 | 否 | share/impl/api12/KnockShareApi12.ets, share/TapShareService.ets |
| 隔空传送 | @kit.ShareKit (harmonyShare gesturesShare) | 20 | SystemCapability.Collaboration.HarmonyShare | 无 | 否 | share/impl/api20/GesturesShareApi20.ets, share/AirTransferService.ets |
| 手写笔 | @kit.ArkUI (组件 TouchEvent) | 未限定独立 API（基础输入事件，随 ArkUI） | 无独立 SystemCapability（基础输入事件） | 无独立权限（基础输入事件） | 否 | input/StylusInputService.ets |
| Native HiLog | @kit.BasicServicesKit (hilog) — C 接口 | 12 | SystemCapability.HiviewDFX.HiLog | 无 | 否 | diagnostics/HarmonyDiagnosticsExporter.ets（ArkTS 侧调用 getHilogSnapshot），cpp/corebridge/napi/napi_init.cpp（C++ 侧 OH_LOG_SetCallback） |
| Core File Kit (fileUri) | @kit.CoreFileKit (fileUri) | 12 | SystemCapability.FileManagement.File.FileUri | 无 | 否 | diagnostics/HarmonyDiagnosticsExporter.ets |
| 系统剪贴板 | @ohos.pasteboard | 12 | SystemCapability.MiscServices.Pasteboard | 无 | 否 | feature/settings/presentation/SettingsViewModel.ets |
| Application 颜色模式 | @kit.AbilityKit (ApplicationContext) | 11 | 无独立 SystemCapability（随 @kit.AbilityKit Application 生命周期） | 无 | 否 | ui/theme/HarmonyColorModeController.ets |
| HDS Tabs（悬浮页签） | @kit.UIDesignKit (HdsTabs) | 23 | SystemCapability.UIDesign.HDSComponent.Core | 无 | 否 | app/navigation/impl/api23/HdsPrimaryTabsApi23.ets, app/navigation/PrimaryTabShell.ets |
| TextController + LayoutManager | @kit.ArkUI (TextController / LayoutManager) | 12（getLayoutManager/getLineCount/getGlyphPositionAtCoordinate/getLineMetrics）/ 14（getRectsForRange） | 无独立 SystemCapability（随 @kit.ArkUI 整体可用） | 无 | 否 | feature/editor/ui/SujianEditor.ets, feature/editor/render/EditorRenderBackend.ets |
| StyledString / MutableStyledString | @kit.ArkUI (StyledString / MutableStyledString) | 12 | 无独立 SystemCapability（随 @kit.ArkUI 整体可用） | 无 | 否 | feature/editor/render/EditorTextStyleProjector.ets |
| ComponentObserver (inspector) | @kit.ArkUI (inspector) | 12（on('layout') 回调） | 无独立 SystemCapability（随 @kit.ArkUI 整体可用） | 无 | 否 | feature/editor/ui/SujianEditor.ets |
| 沉浸光感材质运行态 | @kit.ArkUI (uiMaterial) | 26 | SystemCapability.ArkUI.ArkUI.Full | 无 | 否 | material/impl/api26/HarmonyMaterialRuntimeApi26.ets |
| 应用共享目录 / 捐献沙箱目录 | 无独立 Kit（module.json5 shareFiles profile） | 23（共享目录 scopes）/ 26.0.0（捐献目录 sharingOS*） | 无独立 SystemCapability（模块级配置） | 无 | 否 | entry/src/main/resources/base/profile/share_files.json（工程资源，不在 platform/ 下） |

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
  - 需在签名 Profile 的 ACL 中声明，并在 `module.json5` 的 `requestPermissions` 登记。
- SystemCapability：`SystemCapability.Security.DlpAntiPeep`

### API20 基础

- 接口：
  - `isDlpAntiPeepSwitchOn()`
  - `on('dlpAntiPeep', callback)`
  - `off('dlpAntiPeep', callback?)`
  - `getDlpAntiPeepInfo()`
  - `passDlpAntiPeepInfo()`
- 枚举 `DlpAntiPeepStatus`：`PASS = 0`，`HIDE = 1`
- 最低 API：20
- 实现文件：`privacy/impl/api20/DlpAntiPeepApi20.ets`

### API21 扩展

- 接口：`setAntiPeepMaskLayer(windowId)`
- 最低 API：21

### API23 扩展

- 接口：
  - `requestAntiPeepOptions(context)`
  - `publishAntiPeepInformation()`
- 枚举 `AntiPeepOptionsResult`：`SUCCESS = 0`，`FAIL = 1`，`ALREADY_ON = 2`
- 最低 API：23
- 实现文件：`privacy/impl/api23/DlpAntiPeepApi23.ets`

### fallback

- API < 20 或能力不支持时，防窥状态为 `unknown`（未解析），不伪造为 `false`（安全侧默认）也不伪造为 `true`。`ShoulderSurfingService.isUnknown()` 返回 true，`isSafe()`/`isPeeping()` 均不成立。
- 状态语义：`PASS` → safe（无窥视）；`HIDE` → peeping（被窥视）；API 调用失败 / 未解析 / 不支持 → unknown。
- `requestEnable()` 在 API < 23 时无法打开系统设置页（`requestAntiPeepOptions` 不可用），返回当前真实开关状态，不伪造；`isEnabled()` 永远查询系统真实开关，不读本地缓存假装启用。
- 官方当前限制：DlpAntiPeep 目前只支持 Phone 设备。官方 2026-09-04 最新最佳实践：https://developer.huawei.com/consumer/en/doc/best-practices/bpta-antipeep-protection
- 对外 facade：`privacy/ShoulderSurfingService.ets`（不直接 import `@kit.DeviceSecurityKit`；仅 import `@kit.AbilityKit` 的 `common` 用于 context 类型；不认识 API Level 数字，版本判断在 `impl/apiXX` facade）

## 窗口隐私

- Kit：`@kit.ArkUI`（`window` 模块）
- 接口：
  - `window.getLastWindow()`
  - `win.setSnapshotSkip(enabled)`（截图/录屏跳过，用于隐私保护）
- 最低 API：未限定独立 API（`setSnapshotSkip` 随 `@kit.ArkUI` `window` 模块整体可用，官方未为该方法单独声明起始 API Level）
- SystemCapability：未限定独立 SystemCapability（随 `@kit.ArkUI` `window` 模块，无独立 syscap 声明）
- 权限：无
- ACL：否
- 实现文件：`privacy/WindowPrivacyService.ets`

## 应用接续

- Kit：无独立 Kit。接续主链走 Ability 生命周期（`EntryAbility.onContinue` / `onCreate` / `onNewWant`）+ `module.json5` 中 ability 声明 `"continuable": true`，不创造独立的接续 Kit 名（仓库无此 Kit/import）。
- 接口：
  - `module.json5` 中 ability 声明 `"continuable": true`（系统接续开关）
  - 源设备 `EntryAbility.onContinue` 调 `AppContinuationService.getCurrentPayload()` 写出 payload
  - 目标设备 `EntryAbility.onCreate` / `onNewWant` 在 `launchReason === CONTINUATION` 时调 `restoreFromWant()` 还原 payload
- 最低 API：未限定独立 API（Staged 模型 ability 级配置，随 `@kit.AbilityKit` Ability 生命周期，无独立起始 API Level 声明）
- SystemCapability：无独立 SystemCapability（Ability 级配置）
- 权限：无
- ACL：否
- 实现文件：`continuity/AppContinuationService.ets`

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
- 最低 API：23（本项目使用条件：API23+ 时启用，API < 23 fallback 到普通 Navigation/NavDestination）
- SystemCapability：`SystemCapability.UIDesign.HDSComponent.Core`
- 权限：无
- ACL：否
- fallback：API < 23 使用普通 `Navigation` / `NavDestination`
- 实现文件：
  - `app/navigation/impl/api23/HdsPrimaryTabsApi23.ets`（HdsNavigation 替代 Navigation）
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
  - `hdsEffect.pointLight`（`@kit.UIDesignKit`，20.0.0，`SystemCapability.UIDesign.HDSComponent.Core`）：语义是光源照亮**周围**组件，本就不是单组件按压反馈；且本机 ArkTS 侧构造成功、能力检查通过（`apiLevel=26 hdsCore=true effect=true`），native 层每次报 `HDS_hdsbase: [42]Wrong argument type. int32 expected.`，enum 成员/字面量 int/只留两字段/去掉 options 全被拒。
  - `hdsEffect.pressShadow(PressShadowType)`：只支持 Button，官方定义为「按压交互时自动计算背景色变化」的视效，`BLEND_GRADIENT` 是**叠白**（中心 85% 白、边缘 100% 白），实测是由内向外递增的整面高光，不是跟随手指的定点光；白底上还会削顶到 255 什么也看不出来。官方示例要求 Button 配 `stateEffect: false`，否则内建压暗与按压阴影互盖。
  - `CommonMethod.lightUpEffect(0..1)`（common.d.ts:21813-21845）：只有整体亮度一个参数，**无位置参数**。
  - `HdsVisualComponent`：只有 `DUAL_EDGE_FLOW_LIGHT_WITH_BACKGROUND_MASK` 一个场景，无悬浮按钮场景。HDS 组件库里没有任何悬浮按钮类组件。
- 页面内悬浮按钮的现行做法：`ui/components/PrimaryFab.ets` 用通用属性 `backgroundBlurStyle(BlurStyle.Thin)` + `shadow(ShadowStyle.OUTER_DEFAULT_XS)` + Button 内建 `stateEffect`，与底栏胶囊的观感对齐（`barBackgroundBlurStyle` 就是 Tabs 对 `backgroundBlurStyle` 的专有封装）。内容流拿不到材质，悬浮感靠模糊 + 投影，不靠光感。
  - 阴影档位实机灰度落差（Pocket 2，量按钮右边缘相对背景）：不设 0 级 / `OUTER_FLOATING_MD` 35 级 / `OUTER_FLOATING_SM` 14 级 / `OUTER_DEFAULT_XS` 3 级。`OUTER_DEFAULT_*` 与 `OUTER_FLOATING_*` 是两套并行档位，**不能按名字里的「SM」推断轻重**。
  - 别再设 `border`：深色模式下受光组件自带的 border 会覆盖点光源效果；FAB 的边界靠背景模糊与背景色差自然形成。
- 参数要点（照官方《组件适配沉浸光感》Button 一节）：材质样式取薄档 `ULTRA_THIN` / `THIN`；`materialColor` 必须带透明度，不透明纯色会把材质滤镜完全挡住；开了材质后不要再设 `backgroundColor` / 背景模糊 / `border`，它们会盖在材质层之上；THIN/ULTRA_THIN 时 `fontColor` 要用系统可反色资源（如 `sys.color.icon_primary`）才跟随反色；开了 `lightEffect` 后按钮默认点击态/悬浮态反馈由材质接管。
- 应用级开关：`entry/src/main/module.json5` 已配 `ohos.arkui.UIMaterial.state = "enable"`；ENABLE 下 Button 不会默认开启，必须显式传 `systemMaterial`。
- 说明：声明位于本机 SDK `openharmony/ets/api/@ohos.arkui.uiMaterial.d.ts`（`ImmersiveStyle { ULTRA_THIN=0, THIN=1, REGULAR=2 }`）与 `hms/ets/api/@hms.hds.hdsMaterial.d.ets`（`MaterialType { NONE=0, ADAPTIVE=100, IMMERSIVE=101 }`、`MaterialLevel { EXQUISITE=0, GENTLE=1, SMOOTH=2, ADAPTIVE=10 }`）。官方文档 docId：`开发指南/ArkUI_方舟UI框架/UI开发_ArkTS声明式开发范式/沉浸光感/沉浸光感开发指导/组件适配沉浸光感/arkts-immersive-light-sense-component-adaptation`、`…/沉浸光感常见问题/arkts-immersive-light-sense-faq`、`开发指南/UI_Design_Kit_UI设计套件/沉浸光感/ui-design-hds-component-material`、`FAQ/UI框架/UI界面/HarmonyOS下HdsNavigation与HdsTabs实现滚动模糊及沉浸光感材质效果的解决方案/faqs-arkui-1095`

### stackBuilder 自绘区的两个坑（`HdsAppDestinationApi23.ets` 实测）

- **HDS 返回按钮不会因为 stackBuilder 存在就自动避让**。它画在标题栏自己的层上，直接压住自绘标题（实测返回按钮面占 x 50~174px，星图详情页标题被完全盖住）。非根页必须自己让出槽位：`.padding({ left: isRoot ? 16 : 60, right: 16 })`（16 内边距 + 40 返回按钮 + 4 间距）。
- **返回按钮自带 label**：HDS 会把目标页标题当返回按钮文案画在圆形按钮里，标题一长就被裁成一两个字（实测星图详情页返回按钮里露出「1」）。置 `content.backIcon = { label: '' }` 清掉。
- stackBuilder 是自绘区域，**不吃 HDS 给 `mainTitle` 准备的内边距**，左右 16vp 要自己补，否则标题顶到屏幕左边缘（实测左边界 53px → 3px）。
- `stackBuilder` 的类型是 `CustomBuilder`，**里面的组件调用必须写在 `@Builder` 里**。直接在箭头函数体里写 `SomeComponent({...})` 会在运行时抛 `TypeError: class constructor cannot called without 'new'`（ArkTS 把它当普通函数调用了）。这条对所有 `CustomBuilder` 字段通用（`@BuilderParam`、`stackBuilder`、类型为 `CustomBuilder` 的属性）。

### 页面内容流里的材质与阴影（实测）

- 材质挂到内容流的 Button 上，**hilog 会打 `Material inactive: out of scope`，但材质仍然有响应**。不要只凭这条日志判定材质没生效：作品页 FAB 实测静止 0 级 → 按住 109 级。判断必须以像素测量为准。
- 深色模式纯黑底上，材质静止态几乎全透明（0 级，与背景同色），圆盘边界看不见；这是材质本身的特性，不是属性被谁删了。
- **材质阴影与自定义 shadow 在深色纯黑底上都不显**：材质背景层透明，`shadow` 按组件自身背景形状外投，背景透明则无物可投。
- 官方对替换关系的明确表述只覆盖阴影：「沉浸式系统材质默认自带阴影效果（`applyShadow` 为 true），优先于 `shadow` 通用属性，此时自定义的 `shadow` 设置不会生效。如需使用自定义阴影，将 `applyShadow` 置为 false 后再设置 `shadow`。」**`border` 没有对应的替换通道**，自定义 `border` 会盖在材质层之上。
- `materialColor` 的官方语义：「对所有档位的算力设备均生效。在高算力和中算力设备上，该参数为材质滤镜再混合一层纯色效果；在低算力设备上，该参数作为背景色 backgroundColor 属性值。」给它不透明纯色会遮挡材质滤镜（实测页头胶囊传 `#33FFFFFF` 糊成一片发灰起雾的乳白，与底栏的通透感差 3 倍以上）。

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
