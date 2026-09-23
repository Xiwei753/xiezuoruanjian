# HarmonyOS 平台能力矩阵

本文件记录素笺 HarmonyOS 客户端实际使用的每个系统能力：Kit、接口、最低 API、SystemCapability、权限、ACL 需求、fallback 策略和实现文件。新增系统能力前先查 HarmonyOS 官方 API 和本机 SDK d.ts（`/opt/devecostudio/sdk/default`），再在此登记。

实现文件路径相对于本文件所在目录 `entry/src/main/ets/platform/`（即下表与正文中的路径不以 `platform/` 开头，直接从子目录名写起）。

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
  - `event.sourceTool` / `SourceTool` 枚举识别笔类工具（`SourceTool.Pen`）
  - 坐标来自 `event.touches[0].x / y`
  - 压力和倾斜来自事件级 `event.pressure / event.tiltX / event.tiltY`（BaseEvent 字段）
  - 时间戳 `event.timestamp`
- 最低 API：未限定独立 API（ArkUI 组件触摸事件，随 ArkUI 整体可用，无独立起始 API Level 声明）
- SystemCapability：无独立 SystemCapability（基础输入事件）
- 权限：无独立权限（基础输入事件）
- ACL：否
- 实现文件：`input/StylusInputService.ets`
- 说明：ArkUI 原始输入只在 `platform/input` 归一化，不把平台事件类型传进 Core。`StylusInputService` 消费 ArkUI 组件 `.onTouch` 传进来的全局 `TouchEvent`，不再依赖 `@kit.InputKit` 的 `TouchEvent / ToolType`。
