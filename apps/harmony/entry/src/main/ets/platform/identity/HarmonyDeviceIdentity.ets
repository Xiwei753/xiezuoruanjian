import { common } from '@kit.AbilityKit'
import util from '@ohos.util'
import fs from '@ohos.file.fs'

/**
 * Harmony 设备标识 — :platform 层的平台能力封装。
 *
 * 在应用私有 filesDir 下持久化一份 device_id，进程首次启动时生成，
 * 后续读取复用。对应 Android 端的 AndroidDeviceIdentity。
 *
 * Issue #670 评论 5652119660 修复 2e：Harmony 端 diagnostics init 需要应用级 device id，
 * 不依赖系统设备标识 API（避免权限问题）。
 */
export class HarmonyDeviceIdentity {
  private static readonly DEVICE_ID_FILE_NAME = 'device_id'

  /**
   * 获取或创建设备 ID。首次调用生成 UUID v4 并持久化，后续调用读取复用。
   * 读取/写入失败时返回新生成的 UUID（不持久化），保证总有非空 ID。
   */
  static getOrCreateDeviceId(context: common.UIAbilityContext): string {
    const filesDir = context.filesDir
    const deviceIdPath = `${filesDir}/${HarmonyDeviceIdentity.DEVICE_ID_FILE_NAME}`

    // 尝试读取已有文件。
    try {
      if (fs.accessSync(deviceIdPath)) {
        const fd = fs.openSync(deviceIdPath, fs.OpenMode.READ_ONLY)
        try {
          const stat = fs.statSync(deviceIdPath)
          const buf = new ArrayBuffer(stat.size)
          fs.readSync(fd.fd, buf)
          const textDecoder = new util.TextDecoder('utf-8')
          const content = textDecoder.decodeWithStream(new Uint8Array(buf))
          if (content.trim().length > 0) {
            return content.trim()
          }
        } finally {
          fs.closeSync(fd)
        }
      }
    } catch (_) {
      // 读取失败，继续走生成路径。
    }

    // 生成新 UUID v4 — util.generateRandomUUID(isFast) 返回 UUID 字符串。
    const newId = util.generateRandomUUID(false)

    // 尝试持久化（失败不影响返回值）。
    try {
      const fd = fs.openSync(
        deviceIdPath,
        fs.OpenMode.CREATE | fs.OpenMode.WRITE_ONLY | fs.OpenMode.TRUNC
      )
      try {
        const textEncoder = new util.TextEncoder('utf-8')
        const encoded: Uint8Array = textEncoder.encodeInto(newId)
        fs.writeSync(fd.fd, encoded.buffer)
      } finally {
        fs.closeSync(fd)
      }
    } catch (_) {
      // 写入失败时仍返回新 ID，下次启动会重新生成。
    }

    return newId
  }
}
