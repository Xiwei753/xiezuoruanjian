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
