// NativeInitParams — Core 初始化所需的真实目录与设备信息。
//
// 此前 ArkTS 只传 appDataRoot，files_dir / cache_dir / device_id / app_version /
// locale / timezone 全部传空串，Rust 端把空串当合法路径，导致 PlatformInit.app_data_dir
// 与 Core 配置契约不一致、同步配置落到错误目录。
//
// 这里由初始化调用方收集真实值：
// - appDataRoot / filesDir：Core 数据根（filesDir/sujian-data），二者必须是同一目录。
// - cacheDir：真实的 UIAbilityContext.cacheDir。
// - deviceId / appVersion / locale / timezone：复用 HarmonyDeviceIdentity /
//   bundleManager / DeviceInfoSnapshot 的统一数据源。

import { common } from '@kit.AbilityKit'
import bundleManager from '@ohos.bundle.bundleManager'
import { HarmonyDeviceIdentity } from '../identity/HarmonyDeviceIdentity'
import { getDeviceInfoSnapshot } from '../device/DeviceInfoSnapshot'
import { NetworkSnapshot } from '../network/NetworkStatusService'

export interface NativeInitParams {
  appDataRoot: string
  filesDir: string
  cacheDir: string
  deviceId: string
  appVersion: string
  locale: string
  timezone: string
  networkKnown: boolean
  isConnected: boolean
  isMetered: boolean
}

/**
 * 从能力上下文与网络快照构造 Core 初始化参数。
 * @param context UIAbilityContext，提供 filesDir / cacheDir。
 * @param appDataRoot Core 数据根（filesDir/sujian-data）。
 * @param network 当前网络快照；known=false 表示查询失败/未知。
 */
export function collectNativeInitParams(
  context: common.UIAbilityContext,
  appDataRoot: string,
  network: NetworkSnapshot
): NativeInitParams {
  const deviceSnapshot = getDeviceInfoSnapshot()
  let appVersion = 'unknown'
  try {
    const info = bundleManager.getBundleInfoForSelfSync(
      bundleManager.BundleFlag.GET_BUNDLE_INFO_DEFAULT
    )
    appVersion = info.versionName
  } catch (_e) {
    appVersion = 'unknown'
  }
  return {
    appDataRoot: appDataRoot,
    // PlatformInit.app_data_dir 必须与 Core 的 app_data_root 指向同一数据根。
    filesDir: appDataRoot,
    cacheDir: context.cacheDir,
    deviceId: HarmonyDeviceIdentity.getOrCreateDeviceId(context),
    appVersion: appVersion,
    locale: deviceSnapshot.locale,
    timezone: deviceSnapshot.timezone,
    networkKnown: network.known,
    isConnected: network.isConnected,
    isMetered: network.isMetered
  }
}
