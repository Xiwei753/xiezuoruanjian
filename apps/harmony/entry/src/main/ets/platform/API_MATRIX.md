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
| 分享 | @kit.ShareKit (systemShare / harmonyShare) | 未限定独立 API（随 @kit.ShareKit） | 未限定独立 SystemCapability | 无独立权限（systemShare/harmonyShare 无 ohos.permission.* 声明） | 否 | share/SystemShareService.ets, share/TapShareService.ets, share/AirTransferService.ets |
| 手写笔 | @kit.InputKit (TouchEvent / ToolType) | 未限定独立 API（基础输入事件，随 ArkUI） | 无独立 SystemCapability（基础输入事件） | 无独立权限（基础输入事件） | 否 | input/StylusInputService.ets |

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

- API < 20 或能力不支持时，防窥状态固定为 `false`（安全侧默认），不伪造状态。
- `requestEnable()` 在 API < 23 时无法打开系统设置页（`requestAntiPeepOptions` 不可用），应提示用户手动设置；`isEnabled()` 永远查询系统真实开关，不读本地缓存假装启用。
- 对外 facade：`privacy/ShoulderSurfingService.ets`（通过 `PlatformApiResolver` 分流，不直接 import `@kit.DeviceSecurityKit`；仅 import `@kit.AbilityKit` 的 `common` 用于 context 类型）

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

- Kit：`@kit.ShareKit`（`systemShare` 模块用于普通系统分享；`harmonyShare` 模块用于碰一碰 `knockShare` 与隔空传送 `gesturesShare`）
- 接口：
  - `systemShare.SharedData` / `systemShare.ShareController.show(context)`（普通系统分享）
  - `harmonyShare.on('knockShare', callback)` / `harmonyShare.off('knockShare', callback)`（碰一碰分享）
  - `harmonyShare.on('gesturesShare', { windowId }, callback)` / `harmonyShare.off('gesturesShare', { windowId }, callback)`（隔空传送）
- 最低 API：未限定独立 API（随 `@kit.ShareKit`，官方未为 systemShare/harmonyShare 模块单独声明起始 API Level）
- SystemCapability：未限定独立 SystemCapability
- 权限：无独立权限（`systemShare` / `harmonyShare` 模块无 `ohos.permission.*` 声明，由系统分享面板承载）
- ACL：否
- 实现文件：
  - `share/SystemShareService.ets`（系统分享）
  - `share/TapShareService.ets`（碰一碰分享）
  - `share/AirTransferService.ets`（隔空投送）

## 手写笔

- Kit：`@kit.InputKit`（`TouchEvent` / `ToolType`）
- 接口：
  - `TouchEvent`（`touches[].force` 压力 / `tiltX` / `tiltY` 倾斜 / `toolType` 工具类型）
  - `ToolType.TIP` / `ToolType.ERASER`（识别笔尖/橡皮擦）
- 最低 API：未限定独立 API（`TouchEvent` / `ToolType` 是基础输入事件，随 ArkUI 触摸事件分发，无独立起始 API Level 声明）
- SystemCapability：无独立 SystemCapability（基础输入事件）
- 权限：无独立权限（基础输入事件）
- ACL：否
- 实现文件：`input/StylusInputService.ets`
