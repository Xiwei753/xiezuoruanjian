# HarmonyOS 平台能力矩阵

本文件记录素笺 HarmonyOS 客户端实际使用的每个系统能力：Kit、接口、最低 API、SystemCapability、权限、ACL 需求、fallback 策略和实现文件。新增系统能力前先查 HarmonyOS 官方 API 和本机 SDK d.ts（`/opt/devecostudio/sdk/default`），再在此登记。

实现文件路径相对于本文件所在目录 `entry/src/main/ets/platform/`。

## 能力总览

| 能力 | Kit | 最低 API | SystemCapability | 权限 | ACL | 实现文件 |
|------|-----|---------|------------------|------|-----|---------|
| 握姿感知 | @kit.MultimodalAwarenessKit (motion) | 20 | SystemCapability.MultimodalAwareness.Motion | ohos.permission.DETECT_GESTURE | 否 | platform/awareness/impl/api20/GripPostureApi20.ets |
| 防窥屏 | @kit.DeviceSecurityKit (dlpAntiPeep) | 20（基础）/ 23（requestAntiPeepOptions） | SystemCapability.Security.DlpAntiPeep | ohos.permission.DLP_GET_HIDE_STATUS | 是 | platform/privacy/impl/api20/DlpAntiPeepApi20.ets, platform/privacy/impl/api23/DlpAntiPeepApi23.ets |
| 窗口隐私 | @kit.ArkUI (window) | — | SystemCapability.ArkUI.UIComponent | 无 | 否 | platform/privacy/WindowPrivacyService.ets |
| 应用接续 | continuable (module.json5) | — | — | 无 | 否 | platform/continuity/AppContinuationService.ets |
| 分享 | @kit.ShareKit | — | — | — | 否 | platform/share/SystemShareService.ets, platform/share/TapShareService.ets, platform/share/AirTransferService.ets |
| 手写笔 | @kit.MultimodalInputKit (inputDevice) | — | — | — | 否 | platform/input/StylusInputService.ets |

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
- 实现文件：`platform/awareness/impl/api20/GripPostureApi20.ets`
- 对外 facade：`platform/awareness/GripPostureService.ets`（通过 `PlatformApiResolver` 分流，不直接 import `@kit.MultimodalAwarenessKit`）

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
- 实现文件：`platform/privacy/impl/api20/DlpAntiPeepApi20.ets`

### API21 扩展

- 接口：`setAntiPeepMaskLayer(windowId)`
- 最低 API：21

### API23 扩展

- 接口：
  - `requestAntiPeepOptions(context)`
  - `publishAntiPeepInformation()`
- 枚举 `AntiPeepOptionsResult`：`SUCCESS = 0`，`FAIL = 1`，`ALREADY_ON = 2`
- 最低 API：23
- 实现文件：`platform/privacy/impl/api23/DlpAntiPeepApi23.ets`

### fallback

- API < 20 或能力不支持时，防窥状态固定为 `false`（安全侧默认），不伪造状态。
- `setEnabled` 在 API < 23 时无法打开系统设置页（`requestAntiPeepOptions` 不可用），应提示用户手动设置。
- 对外 facade：`platform/privacy/ShoulderSurfingService.ets`（通过 `PlatformApiResolver` 分流）

## 窗口隐私

- Kit：`@kit.ArkUI`（`window` 模块）
- 接口：
  - `window.getLastWindow()`
  - `win.setSnapshotSkip(enabled)`（截图/录屏跳过，用于隐私保护）
- SystemCapability：`SystemCapability.ArkUI.UIComponent`
- 权限：无
- ACL：否
- 实现文件：`platform/privacy/WindowPrivacyService.ets`

## 应用接续

- Kit：`@kit.AppKitcontinuation` / `continuable`
- 接口：`module.json5` 中 ability 声明 `"continuable": true`（系统接续）
- 权限：无
- ACL：否
- 实现文件：`platform/continuity/AppContinuationService.ets`

## 分享

- Kit：`@kit.ShareKit`
- 权限：—（由三后端汇总，具体权限以后端为准）
- ACL：否
- 实现文件：
  - `platform/share/SystemShareService.ets`（系统分享）
  - `platform/share/TapShareService.ets`（点按分享）
  - `platform/share/AirTransferService.ets`（隔空投送）

## 手写笔

- Kit：`@kit.MultimodalInputKit`（`inputDevice` 模块）
- 权限：—
- ACL：否
- 实现文件：`platform/input/StylusInputService.ets`
