// DeviceInfoSnapshot — 统一设备信息快照。
//
// 当前 HarmonyDiagnosticsExporter.ets 和 SettingsViewModel.ets 各自维护一套设备信息字段，
// 且两边都使用了 SDK 中不存在的 softwareVersion / apiVersion。
// 本文件提供统一的 DeviceInfoSnapshot interface 和 getDeviceInfoSnapshot() 函数，
// 使用正确的 SDK 字段（displayVersion / sdkApiVersion / osFullName），
// 消除重复代码和字段名错误。

import deviceInfo from '@ohos.deviceInfo'
import i18n from '@ohos.i18n'

/**
 * 设备信息快照 — 包含 HarmonyOS 设备的完整硬件/软件标识信息。
 *
 * 字段说明：
 * - platform: 固定值 'harmony'，标识平台类型
 * - deviceType: 设备类型（phone / tablet / wearable / tv 等）
 * - manufacture: 设备制造商
 * - brand: 设备品牌
 * - productModel: 产品型号
 * - productSeries: 产品系列
 * - displayVersion: 系统显示版本号（替代不存在的 softwareVersion）
 * - osFullName: 系统完整名称（如 "HarmonyOS NEXT"）
 * - sdkApiVersion: SDK API 版本号（替代不存在的 apiVersion）
 * - buildType: 构建类型（release / debug 等）
 * - buildVersion: 构建版本号
 * - securityPatchTag: 安全补丁标签
 * - abiList: 支持的 ABI 列表
 * - locale: 系统语言区域
 * - timezone: 系统时区
 */
export interface DeviceInfoSnapshot {
  platform: string
  deviceType: string
  manufacture: string
  brand: string
  productModel: string
  productSeries: string
  displayVersion: string
  osFullName: string
  sdkApiVersion: number
  buildType: string
  buildVersion: number
  securityPatchTag: string
  abiList: string
  locale: string
  timezone: string
}

/**
 * 收集当前设备的完整信息快照。
 *
 * 使用 @ohos.deviceInfo 和 @ohos.i18n 收集设备信息，
 * 返回 DeviceInfoSnapshot 对象。
 *
 * 注意：
 * - 使用 displayVersion 替代不存在的 softwareVersion
 * - 使用 sdkApiVersion 替代不存在的 apiVersion
 * - 包含 osFullName 字段
 */
export function getDeviceInfoSnapshot(): DeviceInfoSnapshot {
  return {
    platform: 'harmony',
    deviceType: deviceInfo.deviceType,
    manufacture: deviceInfo.manufacture,
    brand: deviceInfo.brand,
    productModel: deviceInfo.productModel,
    productSeries: deviceInfo.productSeries,
    displayVersion: deviceInfo.displayVersion,
    osFullName: deviceInfo.osFullName,
    sdkApiVersion: deviceInfo.sdkApiVersion,
    buildType: deviceInfo.buildType,
    buildVersion: deviceInfo.buildVersion,
    securityPatchTag: deviceInfo.securityPatchTag,
    abiList: deviceInfo.abiList,
    locale: i18n.System.getSystemLanguage(),
    timezone: i18n.getTimeZone().getID()
  }
}
