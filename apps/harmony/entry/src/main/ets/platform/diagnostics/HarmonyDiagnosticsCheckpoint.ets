// HarmonyDiagnosticsCheckpoint — 后台诊断 checkpoint 管理器。
//
// 设计目标：平时持续维护 latest.zip，不等用户点导出。
// 运行态发生变化时只标记 dirty；最多每 5 秒生成一次 checkpoint，
// 避免每条日志都重新压 zip。
//
// Issue #773 评论 5845033983：激活 checkpoint — 订阅三个 Store 的 observer，
// theme/navigation/editor 变化时自动 persistRuntimeSnapshot + markDirty。
// 三个 Store 不反向 import Checkpoint，避免循环依赖。Checkpoint 主动订阅 Store 的 observer。
//
// 目录结构：
//   context.filesDir/diagnostics/
//     live/
//       log/          ← Rust writer 持续 append 的日志
//       runtime/      ← 运行态 JSON 快照（双槽原子写入）
//     snapshots/
//       sujian-diagnostics-latest.zip      ← 当前最近一次完整包
//       sujian-diagnostics-previous.zip    ← 上一次完整包
//       sujian-diagnostics-latest.tmp      ← 正在生成的临时文件
//     .staging/
//       checkpoint/   ← 导出暂存目录，确认完整成功后再移动到 snapshots
//
// 双槽 + 原子替换：如果进程恰好死在打包中途，
// latest.zip 或 previous.zip 至少还有一份完整包。

import { common, ConfigurationConstant } from '@kit.AbilityKit'
import hilog from '@ohos.hilog'
import fs from '@ohos.file.fs'

import { HarmonyDiagnosticsExporter } from './HarmonyDiagnosticsExporter'
import { HarmonyDiagnosticsRuntimeSnapshot } from './HarmonyDiagnosticsRuntimeSnapshot'
import { HarmonyThemeAdapter, ThemeObserver, ThemeConfig } from '../../ui/theme/HarmonyThemeAdapter'
import { NavigationChromeStore } from '../../app/navigation/NavigationChromeStore'
import { EditorRuntimeSnapshotStore, EditorRuntimeSnapshot, EditorSnapshotObserver } from '../../feature/editor/diagnostics/EditorRuntimeSnapshotStore'
import { ScreenRole } from '../../corebridge/dto/PlatformDtos'
import { AdaptiveContext, AdaptiveObserver, AdaptiveState } from '../../ui/adaptive/AdaptiveContext'

const LOG_TAG = 'HarmonyDiagnosticsCheckpoint'
const CHECKPOINT_INTERVAL_MS = 5000  // 最多每5秒生成一次
const DIAGNOSTICS_DIR = 'diagnostics'
const LATEST_ZIP = 'sujian-diagnostics-latest.zip'
const PREVIOUS_ZIP = 'sujian-diagnostics-previous.zip'
const LATEST_TMP = 'sujian-diagnostics-latest.tmp'
const STAGING_SUBDIR = '.staging/checkpoint'

// Issue #773 评论 5845033983：ThemeObserver 实现 — theme 变化时 persist + markDirty。
class CheckpointThemeObserver implements ThemeObserver {
  private checkpoint: HarmonyDiagnosticsCheckpoint

  constructor(checkpoint: HarmonyDiagnosticsCheckpoint) {
    this.checkpoint = checkpoint
  }

  onThemeChanged(_theme: ThemeConfig): void {
    this.checkpoint.persistRuntimeSnapshot()
  }
}

// Issue #773 评论 5845033983：NavObserver — navigation 变化时 persist + markDirty。
type CheckpointNavObserver = (role: ScreenRole) => void

// Issue #773 评论 5845398115：AdaptiveObserver 实现 — 窗口尺寸/折叠/安全区变化时 persist + markDirty。
class CheckpointAdaptiveObserver implements AdaptiveObserver {
  private checkpoint: HarmonyDiagnosticsCheckpoint

  constructor(checkpoint: HarmonyDiagnosticsCheckpoint) {
    this.checkpoint = checkpoint
  }

  onWindowSizeChange(_state: AdaptiveState): void {
    this.checkpoint.persistRuntimeSnapshot()
  }
}

export class HarmonyDiagnosticsCheckpoint {
  private static instance: HarmonyDiagnosticsCheckpoint
  private context: common.UIAbilityContext | null = null
  private exporter: HarmonyDiagnosticsExporter
  private dirty: boolean = false
  private isGenerating: boolean = false
  private timerId: number = -1

  // Issue #773 评论 5845033983：三个 observer，在 destroy 中取消订阅。
  private themeObserver: CheckpointThemeObserver | null = null
  private navObserver: CheckpointNavObserver | null = null
  private editorObserver: EditorSnapshotObserver | null = null

  // Issue #773 评论 5845398115：生命周期幂等 + AdaptiveContext 订阅 + 日志变化检测。
  private initialized: boolean = false
  private adaptiveObserver: CheckpointAdaptiveObserver | null = null
  private lastLogSignature: string = ''

  private constructor() {
    this.exporter = new HarmonyDiagnosticsExporter()
  }

  static getInstance(): HarmonyDiagnosticsCheckpoint {
    if (!HarmonyDiagnosticsCheckpoint.instance) {
      HarmonyDiagnosticsCheckpoint.instance = new HarmonyDiagnosticsCheckpoint()
    }
    return HarmonyDiagnosticsCheckpoint.instance
  }

  // 初始化 — 注入 UIAbilityContext，订阅 observer，启动定时器。
  // Issue #773 评论 5845398115：生命周期幂等 — UIAbility 在同一进程销毁后重建时，
  // 不重复订阅 observer、不重复开 timer。destroy() 后 initialized 复位，允许下次 init。
  init(context: common.UIAbilityContext): void {
    if (this.initialized) {
      return
    }
    this.initialized = true
    this.context = context
    this.exporter.setAbilityContext(context)

    // Issue #773 评论 5845033983：订阅三个 Store 的 observer。
    // 三个 Store 不反向 import Checkpoint，避免循环依赖。Checkpoint 主动订阅。

    // Theme observer — theme 变化时 persistRuntimeSnapshot + markDirty
    this.themeObserver = new CheckpointThemeObserver(this)
    HarmonyThemeAdapter.getInstance().addObserver(this.themeObserver)

    // Nav observer — navigation 变化时 persistRuntimeSnapshot + markDirty
    this.navObserver = (_role: ScreenRole) => {
      this.persistRuntimeSnapshot()
    }
    NavigationChromeStore.getInstance().subscribe(this.navObserver)

    // Editor observer — editor 变化时 persistRuntimeSnapshot + markDirty
    this.editorObserver = (_snapshot: EditorRuntimeSnapshot | null) => {
      this.persistRuntimeSnapshot()
    }
    EditorRuntimeSnapshotStore.getInstance().addObserver(this.editorObserver)

    // Issue #773 评论 5845398115：订阅 AdaptiveContext — 窗口尺寸/折叠/安全区变化时 persist + markDirty。
    this.adaptiveObserver = new CheckpointAdaptiveObserver(this)
    AdaptiveContext.getInstance().addObserver(this.adaptiveObserver)

    this.startTimer()
  }

  // 标记 dirty — 运行态变化时调用，只设置 dirty=true。
  // 实际的 checkpoint 生成由定时器触发，最多每 5 秒一次。
  markDirty(): void {
    this.dirty = true
  }

  // Issue #773 评论 5845398115：立即排一轮 checkpoint，不等 5 秒 timer。
  // 首次启动立即调用，消除"首启 5 秒空窗期内闪退没有 latest.zip"的问题。
  requestCheckpoint(): void {
    this.markDirty()
    this.onTimerTick()
  }

  // Issue #773 评论 5845033983：persistRuntimeSnapshot — 把运行态 JSON 写到 live/runtime/ 目录，
  // 然后标记 dirty，让定时器在下一个 tick 生成 checkpoint。
  persistRuntimeSnapshot(): void {
    if (this.context === null) {
      return
    }
    HarmonyDiagnosticsRuntimeSnapshot.getInstance().persistAll(this.context)
    this.markDirty()
  }

  // 启动内部定时器，每 CHECKPOINT_INTERVAL_MS 检查一次 dirty 状态。
  private startTimer(): void {
    this.timerId = setInterval(() => {
      this.onTimerTick()
    }, CHECKPOINT_INTERVAL_MS)
  }

  // 定时器回调 — 检查 dirty 状态，必要时触发 checkpoint 生成。
  // Issue #773 评论 5845033983：tick 不仅看 Store dirty，还比较 live/log 当前日志文件的 size/mtime。
  // Rust live log 持续 append 不触发 Store dirty；窗口尺寸/折叠/safe area 变化由 AdaptiveObserver 触发。
  // 日志变了就生成新 checkpoint，保证 latest.zip 最多落后几秒。
  // Issue #773 评论 5845876043：候选签名只在成功发布后提交，失败时保留重试条件。
  private async onTimerTick(): Promise<void> {
    if (this.isGenerating) {
      return
    }
    const signature = this.getLogSignature()
    const logChanged = signature !== this.lastLogSignature
    if (!this.dirty && !logChanged) {
      return
    }
    const published = await this.generateCheckpoint()
    if (published) {
      this.lastLogSignature = signature
      this.dirty = false
    } else {
      // 保留重试条件，下一 tick 继续尝试
      this.dirty = true
    }
  }

  // Issue #773 评论 5845876043：getLogSignature 只计算签名，不修改 lastLogSignature。
  // 签名的提交（lastLogSignature 赋值）推迟到 onTimerTick 确认发布成功之后。
  private getLogSignature(): string {
    if (this.context === null) {
      return ''
    }
    const logDir = `${this.context.filesDir}/${DIAGNOSTICS_DIR}/live/log`
    return this.computeLogSignature(logDir)
  }

  // Issue #773 评论 5845398115：计算 live/log 目录下所有文件的总 size + 最大 mtime 作为签名。
  // 签名变化说明 Rust 又写了日志，需要生成新 checkpoint。
  // Issue #773 评论 5845876043：本方法只计算签名，不修改 lastLogSignature；
  // 签名提交由 onTimerTick 在确认发布成功后执行。
  private computeLogSignature(logDir: string): string {
    try {
      if (!fs.accessSync(logDir)) {
        return ''
      }
      const entries = fs.listFileSync(logDir)
      let totalSize: number = 0
      let maxMtime: number = 0
      for (let i = 0; i < entries.length; i++) {
        const fullPath = `${logDir}/${entries[i]}`
        const stat = fs.statSync(fullPath)
        totalSize += stat.size
        if (stat.mtime > maxMtime) {
          maxMtime = stat.mtime
        }
      }
      return `${totalSize}:${maxMtime}`
    } catch (_) {
      return ''
    }
  }

  // 生成 checkpoint — 复用 HarmonyDiagnosticsExporter.exportTo() 生成 zip，
  // 然后通过双槽 + 原子替换流程维护 latest.zip / previous.zip。
  // Issue #773 评论 5845033983：先导出到 staging 目录，确认完整成功后再移动到 snapshots。
  // Issue #773 评论 5845876043：改为 Promise<boolean>，所有失败分支返回 false；
  // 只有 atomicReplace 成功后才清 staging、打印成功并返回 true。
  private async generateCheckpoint(): Promise<boolean> {
    if (this.context === null) {
      hilog.warn(0x0000, LOG_TAG, 'context is null; cannot generate checkpoint')
      return false
    }
    this.isGenerating = true
    try {
      // 1. 准备 staging 目录
      const stagingDir = `${this.context.filesDir}/${DIAGNOSTICS_DIR}/${STAGING_SUBDIR}`
      this.ensureDir(stagingDir)

      // 2. 调用 exporter.exportTo(stagingDir) 导出到 staging 目录
      const timestampedZipPath = await this.exporter.exportTo(stagingDir)
      if (timestampedZipPath === null) {
        hilog.warn(0x0000, LOG_TAG, 'exportTo returned null; skipping checkpoint')
        return false
      }

      // 3. 确认 zip 文件存在
      if (!this.fileExists(timestampedZipPath)) {
        hilog.warn(0x0000, LOG_TAG, 'timestamped zip does not exist: %{public}s', timestampedZipPath)
        return false
      }

      // 4. 准备 snapshots 路径
      const snapshotsDir = `${this.context.filesDir}/${DIAGNOSTICS_DIR}/snapshots`
      this.ensureDir(snapshotsDir)
      const tmpPath = `${snapshotsDir}/${LATEST_TMP}`
      const latestPath = `${snapshotsDir}/${LATEST_ZIP}`
      const previousPath = `${snapshotsDir}/${PREVIOUS_ZIP}`

      // 5. 将带时间戳的 zip rename 为 latest.tmp
      this.safeDelete(tmpPath)
      fs.renameSync(timestampedZipPath, tmpPath)

      // 6. 原子替换：旧 latest.zip → previous.zip, latest.tmp → latest.zip
      if (!this.atomicReplace(tmpPath, latestPath, previousPath)) {
        hilog.error(0x0000, LOG_TAG, 'atomicReplace failed; checkpoint not published')
        return false
      }

      // 7. 清理 staging 目录
      this.cleanStagingDir(stagingDir)

      hilog.info(0x0000, LOG_TAG, 'checkpoint generated successfully')
      return true
    } catch (e) {
      hilog.error(0x0000, LOG_TAG, 'generateCheckpoint failed: %{public}s', String(e))
      return false
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

  // 清理 staging 目录中的残留文件。
  private cleanStagingDir(stagingDir: string): void {
    try {
      if (!fs.accessSync(stagingDir)) {
        return
      }
      const entries = fs.listFileSync(stagingDir)
      for (const entry of entries) {
        this.safeDelete(`${stagingDir}/${entry}`)
      }
    } catch (_) {
      // 清理失败不影响主流程
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

  // 销毁 — 停止定时器，取消所有 observer 订阅，释放资源。
  destroy(): void {
    if (this.timerId !== -1) {
      clearInterval(this.timerId)
      this.timerId = -1
    }
    // Issue #773 评论 5845033983：取消三个 Store 的 observer 订阅。
    if (this.themeObserver !== null) {
      HarmonyThemeAdapter.getInstance().removeObserver(this.themeObserver)
      this.themeObserver = null
    }
    if (this.navObserver !== null) {
      NavigationChromeStore.getInstance().unsubscribe(this.navObserver)
      this.navObserver = null
    }
    if (this.editorObserver !== null) {
      EditorRuntimeSnapshotStore.getInstance().removeObserver(this.editorObserver)
      this.editorObserver = null
    }
    // Issue #773 评论 5845398115：取消 AdaptiveContext 订阅。
    if (this.adaptiveObserver !== null) {
      AdaptiveContext.getInstance().removeObserver(this.adaptiveObserver)
      this.adaptiveObserver = null
    }
    // Issue #773 评论 5845398115：生命周期幂等 — 复位 initialized，允许下次 init。
    this.initialized = false
    this.lastLogSignature = ''
  }
}
