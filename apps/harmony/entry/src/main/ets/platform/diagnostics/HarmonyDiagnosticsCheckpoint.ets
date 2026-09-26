// HarmonyDiagnosticsCheckpoint — 后台诊断 checkpoint 管理器。
//
// 设计目标：平时持续维护 latest.zip，不等用户点导出。
// 运行态发生变化时只标记 dirty；最多每 5 秒生成一次 checkpoint，
// 避免每条日志都重新压 zip。
//
// 目录结构：
//   context.filesDir/diagnostics/
//     live/
//       log/          ← Rust writer 持续 append 的日志
//       runtime/      ← 运行态 JSON 快照
//     snapshots/
//       sujian-diagnostics-latest.zip      ← 当前最近一次完整包
//       sujian-diagnostics-previous.zip    ← 上一次完整包
//       sujian-diagnostics-latest.tmp      ← 正在生成的临时文件
//
// 双槽 + 原子替换：如果进程恰好死在打包中途，
// latest.zip 或 previous.zip 至少还有一份完整包。

import { common } from '@kit.AbilityKit'
import hilog from '@ohos.hilog'
import fs from '@ohos.file.fs'
import { HarmonyDiagnosticsExporter } from './HarmonyDiagnosticsExporter'

const LOG_TAG = 'HarmonyDiagnosticsCheckpoint'
const CHECKPOINT_INTERVAL_MS = 5000  // 最多每5秒生成一次
const DIAGNOSTICS_DIR = 'diagnostics'
const LATEST_ZIP = 'sujian-diagnostics-latest.zip'
const PREVIOUS_ZIP = 'sujian-diagnostics-previous.zip'
const LATEST_TMP = 'sujian-diagnostics-latest.tmp'

export class HarmonyDiagnosticsCheckpoint {
  private static instance: HarmonyDiagnosticsCheckpoint
  private context: common.UIAbilityContext | null = null
  private exporter: HarmonyDiagnosticsExporter
  private dirty: boolean = false
  private isGenerating: boolean = false
  private timerId: number = -1

  private constructor() {
    this.exporter = new HarmonyDiagnosticsExporter()
  }

  static getInstance(): HarmonyDiagnosticsCheckpoint {
    if (!HarmonyDiagnosticsCheckpoint.instance) {
      HarmonyDiagnosticsCheckpoint.instance = new HarmonyDiagnosticsCheckpoint()
    }
    return HarmonyDiagnosticsCheckpoint.instance
  }

  // 初始化 — 注入 UIAbilityContext，启动定时器。
  init(context: common.UIAbilityContext): void {
    this.context = context
    this.exporter.setAbilityContext(context)
    this.startTimer()
  }

  // 标记 dirty — 运行态变化时调用，只设置 dirty=true。
  // 实际的 checkpoint 生成由定时器触发，最多每 5 秒一次。
  markDirty(): void {
    this.dirty = true
  }

  // 原子写入运行态 JSON 到 live/runtime/ 目录。
  // 先写 .tmp 文件，然后 rename 为最终文件名（原子覆盖）。
  // 文件名如 theme_runtime_sanitized.json、ui_runtime_sanitized.json、editor_runtime_sanitized.json。
  writeRuntimeJson(filename: string, jsonContent: string): void {
    if (this.context === null) {
      hilog.warn(0x0000, LOG_TAG, 'context is null; cannot write runtime json')
      return
    }
    const runtimeDir = `${this.context.filesDir}/${DIAGNOSTICS_DIR}/live/runtime`
    this.ensureDir(runtimeDir)
    const tmpPath = `${runtimeDir}/${filename}.tmp`
    const finalPath = `${runtimeDir}/${filename}`
    try {
      // 写入 .tmp 文件
      const file = fs.openSync(tmpPath, fs.OpenMode.CREATE | fs.OpenMode.WRITE_ONLY | fs.OpenMode.TRUNC)
      try {
        fs.writeSync(file.fd, jsonContent)
      } finally {
        fs.closeSync(file)
      }
      // rename 为最终文件名（原子覆盖）
      // HarmonyOS fs.renameSync 目标已存在时会抛异常，所以先删除
      this.safeDelete(finalPath)
      fs.renameSync(tmpPath, finalPath)
    } catch (e) {
      hilog.error(0x0000, LOG_TAG, 'writeRuntimeJson failed: %{public}s', String(e))
    }
  }

  // 启动内部定时器，每 CHECKPOINT_INTERVAL_MS 检查一次 dirty 状态。
  private startTimer(): void {
    this.timerId = setInterval(() => {
      this.onTimerTick()
    }, CHECKPOINT_INTERVAL_MS)
  }

  // 定时器回调 — 检查 dirty 状态，必要时触发 checkpoint 生成。
  private async onTimerTick(): Promise<void> {
    if (!this.dirty || this.isGenerating) {
      return
    }
    this.dirty = false
    await this.generateCheckpoint()
  }

  // 生成 checkpoint — 复用 HarmonyDiagnosticsExporter.export() 生成带时间戳的 zip，
  // 然后通过双槽 + 原子替换流程维护 latest.zip / previous.zip。
  private async generateCheckpoint(): Promise<void> {
    if (this.context === null) {
      hilog.warn(0x0000, LOG_TAG, 'context is null; cannot generate checkpoint')
      return
    }
    this.isGenerating = true
    try {
      // 1. 调用 exporter.export() 生成带时间戳的 zip
      //    export() 内部会：flush 日志 → 收集附件 → 调用 Rust export → 返回 zip 路径
      const timestampedZipPath = await this.exporter.export()
      if (timestampedZipPath === null) {
        hilog.warn(0x0000, LOG_TAG, 'export returned null; skipping checkpoint')
        return
      }

      // 2. 确认带时间戳的 zip 文件存在
      if (!this.fileExists(timestampedZipPath)) {
        hilog.warn(0x0000, LOG_TAG, 'timestamped zip does not exist: %{public}s', timestampedZipPath)
        return
      }

      // 3. 准备路径
      const snapshotsDir = `${this.context.filesDir}/${DIAGNOSTICS_DIR}/snapshots`
      const tmpPath = `${snapshotsDir}/${LATEST_TMP}`
      const latestPath = `${snapshotsDir}/${LATEST_ZIP}`
      const previousPath = `${snapshotsDir}/${PREVIOUS_ZIP}`

      // 4. 将带时间戳的 zip rename 为 latest.tmp
      this.safeDelete(tmpPath)
      fs.renameSync(timestampedZipPath, tmpPath)

      // 5. 原子替换：旧 latest.zip → previous.zip, latest.tmp → latest.zip
      this.atomicReplace(tmpPath, latestPath, previousPath)

      hilog.info(0x0000, LOG_TAG, 'checkpoint generated successfully')
    } catch (e) {
      hilog.error(0x0000, LOG_TAG, 'generateCheckpoint failed: %{public}s', String(e))
    } finally {
      this.isGenerating = false
    }
  }

  // 双槽 + 原子替换：
  // 1. 如果 previous.zip 存在，删除
  // 2. 如果 latest.zip 存在，rename → previous.zip
  // 3. latest.tmp → latest.zip
  // 如果进程恰好死在打包中途，latest.zip 或 previous.zip 至少还有一份完整包。
  private atomicReplace(tmpPath: string, latestPath: string, previousPath: string): boolean {
    try {
      // 1. 如果 previous.zip 存在，删除
      this.safeDelete(previousPath)
      // 2. 如果 latest.zip 存在，rename → previous.zip
      if (this.fileExists(latestPath)) {
        fs.renameSync(latestPath, previousPath)
      }
      // 3. latest.tmp → latest.zip
      fs.renameSync(tmpPath, latestPath)
      return true
    } catch (e) {
      hilog.error(0x0000, LOG_TAG, 'atomicReplace failed: %{public}s', String(e))
      return false
    }
  }

  // 安全删除文件 — 如果文件不存在，不抛异常。
  private safeDelete(filePath: string): void {
    try {
      fs.unlinkSync(filePath)
    } catch (_) {
      // 文件不存在是正常情况
    }
  }

  // 检查文件是否存在。
  private fileExists(filePath: string): boolean {
    try {
      return fs.accessSync(filePath)
    } catch (_) {
      return false
    }
  }

  // 确保目录存在 — 如果不存在则创建（含父目录）。
  private ensureDir(dirPath: string): void {
    try {
      if (!fs.accessSync(dirPath)) {
        fs.mkdirSync(dirPath, true)
      }
    } catch (_) {
      // 目录创建失败不影响后续操作
    }
  }

  // 销毁 — 停止定时器，释放资源。
  destroy(): void {
    if (this.timerId !== -1) {
      clearInterval(this.timerId)
      this.timerId = -1
    }
  }
}
