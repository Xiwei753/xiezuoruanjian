// HarmonyDiagnosticsCheckpoint — 后台诊断 checkpoint 管理器。
//
// 设计目标：平时持续维护 latest.zip，不等用户点导出。
// 初始化、运行态变化和结构化诊断事件触发一个合并的单次快照；后台不轮询。
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

import { common } from '@kit.AbilityKit'
import hilog from '@ohos.hilog'
import fs from '@ohos.file.fs'

import { HarmonyDiagnosticEvents } from './HarmonyDiagnosticEvents'
import { HarmonyDiagnosticsExporter } from './HarmonyDiagnosticsExporter'
import { HarmonyDiagnosticsRuntimeSnapshot } from './HarmonyDiagnosticsRuntimeSnapshot'
import { HarmonyThemeAdapter, ThemeObserver, ThemeConfig } from '../../ui/theme/HarmonyThemeAdapter'
import { NavigationChromeStore } from '../../app/navigation/NavigationChromeStore'
import { EditorRuntimeSnapshotStore, EditorRuntimeSnapshot, EditorSnapshotObserver } from '../../feature/editor/diagnostics/EditorRuntimeSnapshotStore'
import { ScreenRole } from '../../app/navigation/ScreenSemantics'
import { AdaptiveContext, AdaptiveObserver, AdaptiveState } from '../../ui/adaptive/AdaptiveContext'
import { HarmonyLifecycle, LifecycleObserver } from '../lifecycle/HarmonyLifecycle'

const LOG_TAG = 'HarmonyDiagnosticsCheckpoint'
const CHECKPOINT_COALESCE_MS = 250
// Rust writer opens, appends, flushes, and closes once per batch. Wait for a
// full quiet window after the last CLOSE_WRITE so adjacent batches publish as
// one archive instead of repeatedly compressing the same log stream.
const LOG_QUIET_WINDOW_MS = 1000
const DIAGNOSTICS_DIR = 'diagnostics'
const LATEST_ZIP = 'sujian-diagnostics-latest.zip'
const PREVIOUS_ZIP = 'sujian-diagnostics-previous.zip'
const LATEST_TMP = 'sujian-diagnostics-latest.tmp'
const STAGING_SUBDIR = '.staging/checkpoint'
const WATCH_CLOSE_WRITE = 0x8
const WATCH_MOVED_FROM = 0x40
const WATCH_MOVED_TO = 0x80
const WATCH_CREATE = 0x100
const WATCH_DELETE = 0x200
const WATCH_DELETE_SELF = 0x400
const WATCH_MOVE_SELF = 0x800
const LOG_DIRECTORY_EVENTS = WATCH_MOVED_FROM | WATCH_MOVED_TO | WATCH_CREATE | WATCH_DELETE |
  WATCH_DELETE_SELF | WATCH_MOVE_SELF
const LOG_FILENAME_PATTERN = /^sujian-current-[A-Za-z0-9._-]+\.log$/

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

class CheckpointBackgroundFinalizerObserver implements LifecycleObserver {
  private checkpoint: HarmonyDiagnosticsCheckpoint

  constructor(checkpoint: HarmonyDiagnosticsCheckpoint) {
    this.checkpoint = checkpoint
  }

  onBackgroundFinalize(): Promise<void> {
    return this.checkpoint.finalizeBackground()
  }
}

export class HarmonyDiagnosticsCheckpoint {
  private static instance: HarmonyDiagnosticsCheckpoint
  private context: common.UIAbilityContext | null = null
  private exporter: HarmonyDiagnosticsExporter
  private dirty: boolean = false
  private stateDirty: boolean = false
  private logDirty: boolean = false
  // Issue #773 评论 5846511131：版本号机制 — 导出过程中若 observer 又 markDirty()，
  // 最后 dirty = false 不会把新变化吃掉。tick 开始记版本号，成功后比较是否变化。
  private dirtyVersion: number = 0
  private stateDirtyVersion: number = 0
  private logDirtyVersion: number = 0
  private isGenerating: boolean = false
  private scheduledTimerId: number = -1
  private isForeground: boolean = true
  private backgroundFinalizationPending: boolean = false
  private backgroundTransitionVersion: number = 0
  private activeCheckpointPromise: Promise<void> | null = null
  private lastLogClosedAt: number = 0
  private lastArchiveCompletedAt: number = 0
  private backgroundFinalizerObserver: LifecycleObserver | null = null

  // Issue #773 评论 5845033983：三个 observer，在 destroy 中取消订阅。
  private themeObserver: CheckpointThemeObserver | null = null
  private navObserver: CheckpointNavObserver | null = null
  private editorObserver: EditorSnapshotObserver | null = null

  // Issue #773 评论 5845398115：生命周期幂等 + AdaptiveContext 订阅。
  private initialized: boolean = false
  private adaptiveObserver: CheckpointAdaptiveObserver | null = null

  // 目标目录出现前，只短暂监听当前最深的已存在父目录；目标 log 出现后只监听它。
  private logDirectoryWatcher: fs.Watcher | null = null
  private logDirectoryWatchLevel: number = -1

  private constructor() {
    this.exporter = new HarmonyDiagnosticsExporter()
  }

  static getInstance(): HarmonyDiagnosticsCheckpoint {
    if (!HarmonyDiagnosticsCheckpoint.instance) {
      HarmonyDiagnosticsCheckpoint.instance = new HarmonyDiagnosticsCheckpoint()
    }
    return HarmonyDiagnosticsCheckpoint.instance
  }

  // 初始化 — 注入 UIAbilityContext 并订阅运行态 observer。
  // Issue #773 评论 5845398115：生命周期幂等 — UIAbility 在同一进程销毁后重建时，
  // 不重复订阅 observer、不重复开 timer。destroy() 后 initialized 复位，允许下次 init。
  init(context: common.UIAbilityContext): void {
    if (this.initialized) {
      return
    }
    this.initialized = true
    this.isForeground = true
    this.backgroundFinalizationPending = false
    this.context = context
    this.exporter.setAbilityContext(context)
    this.attachLogWatchers()
    HarmonyDiagnosticEvents.setCheckpointDirtyHandler(() => this.markDirty())
    this.backgroundFinalizerObserver = new CheckpointBackgroundFinalizerObserver(this)
    HarmonyLifecycle.getInstance().addObserver(this.backgroundFinalizerObserver)

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

    // Produce an initial snapshot so latest.zip exists before the next state event.
    this.markDirty()
  }

  // 运行态变化合并触发一次 checkpoint。
  markDirty(): void {
    this.stateDirty = true
    this.dirty = true
    this.dirtyVersion += 1
    this.stateDirtyVersion += 1
    this.schedulePendingCheckpoint()
  }

  // 显式请求绕过静默窗口，立即归档调用时可见的最新日志。
  requestCheckpoint(): void {
    this.stateDirty = true
    this.dirty = true
    this.dirtyVersion += 1
    this.stateDirtyVersion += 1
    this.cancelScheduledCheckpoint()
    if (!this.initialized || this.context === null) {
      return
    }
    void this.runCheckpoint(true)
  }

  onForeground(): void {
    const wasForeground = this.isForeground
    this.isForeground = true
    this.backgroundTransitionVersion += 1
    this.backgroundFinalizationPending = false
    if (!wasForeground) {
      this.attachLogWatchers()
      // The watcher was paused after background save completion. Reconcile the
      // persisted log directory on return, including writes made while paused.
      this.markLogClosed()
    }
  }

  onBackground(): void {
    if (!this.isForeground) {
      return
    }
    this.isForeground = false
    this.backgroundTransitionVersion += 1
    this.backgroundFinalizationPending = true
    this.cancelScheduledCheckpoint()
    // Keep the directory watcher through the background save. The lifecycle
    // finalization callback stops it after all background saves have completed.
  }

  // Lifecycle awaits this after all background save Promises settle and before
  // its single suspend-delay request is released.
  async finalizeBackground(): Promise<void> {
    if (!this.initialized || !this.backgroundFinalizationPending || this.isForeground) {
      return
    }
    const transitionVersion = this.backgroundTransitionVersion
    this.stopLogWatchers()
    this.stateDirty = true
    this.dirty = true
    this.dirtyVersion += 1
    this.stateDirtyVersion += 1
    this.cancelScheduledCheckpoint()
    try {
      // exportTo() flushes Rust diagnostics before collecting and compressing
      // files. The lifecycle keeps the same suspend-delay request until this
      // final archive has either been published or cancelled as stale.
      await this.runCheckpoint(true, transitionVersion)
    } finally {
      if (this.backgroundTransitionVersion === transitionVersion) {
        this.backgroundFinalizationPending = false
      }
    }
  }

  // Issue #773 评论 5845033983：persistRuntimeSnapshot — 把运行态 JSON 写到 live/runtime/ 目录，
  // 然后标记 dirty，合并触发一次 checkpoint。
  persistRuntimeSnapshot(): void {
    if (this.context === null) {
      return
    }
    HarmonyDiagnosticsRuntimeSnapshot.getInstance().persistAll(this.context)
    this.markDirty()
  }

  // 普通运行态事件用一次性 debounce；日志事件会重置计时器，形成真正的静默窗口。
  private scheduleCheckpoint(resetExisting: boolean = false, delayMs: number = CHECKPOINT_COALESCE_MS): void {
    if (!this.initialized || this.context === null || !this.dirty || this.isGenerating ||
      !this.isForeground || (!this.stateDirty && !this.logDirty)) {
      return
    }
    if (this.scheduledTimerId !== -1) {
      if (!resetExisting) {
        return
      }
      this.cancelScheduledCheckpoint()
    }
    this.scheduledTimerId = setTimeout(() => {
      this.scheduledTimerId = -1
      void this.runScheduledCheckpoint()
    }, Math.max(0, delayMs))
  }

  // CLOSE_WRITE 是一个 batch 已落盘的边界，才从这里启动真正的静默窗口。
  private markLogClosed(): void {
    this.logDirty = true
    this.dirty = true
    this.dirtyVersion += 1
    this.logDirtyVersion += 1
    this.lastLogClosedAt = Date.now()
    if (this.isForeground) {
      this.schedulePendingCheckpoint(true)
    }
  }

  private schedulePendingCheckpoint(resetExisting: boolean = false): void {
    if (!this.isForeground) {
      return
    }
    const delay = this.logDirty ? this.getLogQuietDelay() : CHECKPOINT_COALESCE_MS
    this.scheduleCheckpoint(resetExisting, delay)
  }

  private getLogQuietDelay(): number {
    const quietDeadline = Math.max(
      this.lastLogClosedAt + LOG_QUIET_WINDOW_MS,
      this.lastArchiveCompletedAt + LOG_QUIET_WINDOW_MS
    )
    return Math.max(0, quietDeadline - Date.now())
  }

  private cancelScheduledCheckpoint(): void {
    if (this.scheduledTimerId !== -1) {
      clearTimeout(this.scheduledTimerId)
      this.scheduledTimerId = -1
    }
  }

  // 单次归档失败后保留 dirty，但不自行重试；归档期间新增变化保留版本，
  // 归档结束后再等待静默窗口，避免紧接着反复压 ZIP。
  private async runScheduledCheckpoint(): Promise<void> {
    if (!this.isForeground || this.isGenerating || !this.dirty) {
      return
    }
    if (!this.stateDirty && !this.logDirty) {
      return
    }
    if (this.logDirty && this.isForeground) {
      const quietDelay = this.getLogQuietDelay()
      if (quietDelay > 0) {
        this.scheduleCheckpoint(true, quietDelay)
        return
      }
    }
    await this.runCheckpoint(false)
  }

  private async runCheckpoint(manual: boolean, backgroundTransitionVersion: number = -1): Promise<void> {
    if (backgroundTransitionVersion >= 0 && !this.isCurrentBackgroundFinalization(backgroundTransitionVersion)) {
      return
    }
    const active = this.activeCheckpointPromise
    if (active !== null) {
      if (!manual) {
        return
      }
      await active
      if (backgroundTransitionVersion >= 0 && !this.isCurrentBackgroundFinalization(backgroundTransitionVersion)) {
        return
      }
      await this.runCheckpoint(true, backgroundTransitionVersion)
      return
    }
    const checkpointPromise = this.runCheckpointBody(backgroundTransitionVersion)
    this.activeCheckpointPromise = checkpointPromise
    try {
      await checkpointPromise
    } finally {
      if (this.activeCheckpointPromise === checkpointPromise) {
        this.activeCheckpointPromise = null
      }
    }
  }

  private async runCheckpointBody(backgroundTransitionVersion: number): Promise<void> {
    const versionAtStart = this.dirtyVersion
    const stateVersionAtStart = this.stateDirtyVersion
    const logVersionAtStart = this.logDirtyVersion
    const published = await this.generateCheckpoint(backgroundTransitionVersion)
    this.lastArchiveCompletedAt = Date.now()
    const changedDuringArchive = this.dirtyVersion !== versionAtStart

    if (published) {
      if (this.stateDirtyVersion === stateVersionAtStart) {
        this.stateDirty = false
      }
      if (this.logDirtyVersion === logVersionAtStart) {
        this.logDirty = false
      }
    }
    this.dirty = this.stateDirty || this.logDirty

    // 新写入在归档期间到达时只保留待处理状态；从归档完成起至少再等一个静默窗。
    if (changedDuringArchive && this.dirty) {
      if (this.stateDirty || (this.logDirty && this.isForeground)) {
        this.schedulePendingCheckpoint(true)
      }
    }
  }

  private isCurrentBackgroundFinalization(transitionVersion: number): boolean {
    return this.backgroundFinalizationPending && !this.isForeground &&
      this.backgroundTransitionVersion === transitionVersion
  }

  // 生成 checkpoint — 复用 HarmonyDiagnosticsExporter.exportTo() 生成 zip，
  // 然后通过双槽 + 原子替换流程维护 latest.zip / previous.zip。
  // Issue #773 评论 5845033983：先导出到 staging 目录，确认完整成功后再移动到 snapshots。
  // Issue #773 评论 5845876043：改为 Promise<boolean>，所有失败分支返回 false；
  // 只有 atomicReplace 成功后才清 staging、打印成功并返回 true。
  private async generateCheckpoint(backgroundTransitionVersion: number = -1): Promise<boolean> {
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
      if (backgroundTransitionVersion >= 0 &&
        !this.isCurrentBackgroundFinalization(backgroundTransitionVersion)) {
        this.safeDelete(timestampedZipPath)
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
  // Issue #773 评论 5846511131：只有 latest 真存在时才轮换 previous，
  // 避免连续两次 tmp → latest 失败时把最后一份 previous 也删掉。
  // 1. 如果 latest.zip 存在，删旧 previous.zip，然后 latest.zip → previous.zip
  // 2. latest.tmp → latest.zip；这一步失败时把刚轮换走的 previous 恢复成 latest
  //    （评论 5846308072：tmp 发布失败后恢复 previous，别让两份完整包同时消失）
  // 如果进程恰好死在打包中途，latest.zip 或 previous.zip 至少还有一份完整包。
  private atomicReplace(tmpPath: string, latestPath: string, previousPath: string): boolean {
    const hadLatest = this.fileExists(latestPath)
    if (hadLatest) {
      try {
        this.safeDelete(previousPath)
        fs.renameSync(latestPath, previousPath)
      } catch (e) {
        // 轮换这一步失败时 latest 自己还在，保留旧包，不再冒险覆盖
        hilog.error(0x0000, LOG_TAG, 'atomicReplace rotate latest→previous failed: %{public}s', String(e))
        return false
      }
    }
    try {
      fs.renameSync(tmpPath, latestPath)
      return true
    } catch (e) {
      this.restorePreviousAsLatest(hadLatest, latestPath, previousPath)
      hilog.error(0x0000, LOG_TAG, 'atomicReplace failed: %{public}s', String(e))
      return false
    }
  }

  // Issue #773 评论 5846308072（本轮 5846511131 重申）：tmp → latest 失败后，
  // 把刚轮换到 previous 的旧包恢复成 latest，避免重试再次失败后两份都不在。
  private restorePreviousAsLatest(hadLatest: boolean, latestPath: string, previousPath: string): void {
    if (!hadLatest || this.fileExists(latestPath) || !this.fileExists(previousPath)) {
      return
    }
    try {
      fs.renameSync(previousPath, latestPath)
      hilog.info(0x0000, LOG_TAG, 'restored previous zip back to latest after failed publish')
    } catch (e) {
      hilog.error(0x0000, LOG_TAG, 'restore previous→latest failed: %{public}s', String(e))
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
      if (fs.accessSync(filePath)) {
        fs.unlinkSync(filePath)
      }
    } catch (_) {
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

  private getLogWatchPaths(): Array<string> {
    if (this.context === null) {
      return []
    }
    const filesDir = this.context.filesDir
    const diagnosticsDir = `${filesDir}/${DIAGNOSTICS_DIR}`
    const liveDir = `${diagnosticsDir}/live`
    return [filesDir, diagnosticsDir, liveDir, `${liveDir}/log`]
  }

  // log 尚未创建时只看当前最深的已存在父目录；新子目录出现后切换到它。
  private attachLogWatchers(): void {
    const paths = this.getLogWatchPaths()
    let level: number = -1
    for (let index = paths.length - 1; index >= 0; index -= 1) {
      try {
        if (fs.statSync(paths[index]).isDirectory()) {
          level = index
          break
        }
      } catch (_) {
      }
    }

    if (level === -1) {
      this.stopLogWatchers()
      return
    }
    if (this.logDirectoryWatchLevel !== level) {
      this.stopLogDirectoryWatcher()
    }
    if (this.logDirectoryWatcher === null) {
      try {
        const watcherEvents = level === 3 ? LOG_DIRECTORY_EVENTS | WATCH_CLOSE_WRITE : LOG_DIRECTORY_EVENTS
        const watcher = fs.createWatcher(paths[level], watcherEvents, (event) => {
          this.onLogDirectoryEvent(level, event.fileName, event.event)
        })
        watcher.start()
        this.logDirectoryWatcher = watcher
        this.logDirectoryWatchLevel = level
      } catch (e) {
        hilog.warn(0x0000, LOG_TAG, 'cannot watch diagnostics directory %{public}s: %{public}s', paths[level], String(e))
        this.logDirectoryWatcher = null
        this.logDirectoryWatchLevel = -1
      }
    }
  }

  private onLogDirectoryEvent(level: number, fileName: string, eventMask: number): void {
    if (!this.initialized || this.context === null ||
      (!this.isForeground && !this.backgroundFinalizationPending)) {
      return
    }

    const entryEvents = WATCH_MOVED_FROM | WATCH_MOVED_TO | WATCH_CREATE | WATCH_DELETE
    if (level === 3) {
      if ((eventMask & WATCH_CLOSE_WRITE) !== 0 && LOG_FILENAME_PATTERN.test(fileName)) {
        // Rust closes each append batch before emitting IN_CLOSE_WRITE.
        this.markLogClosed()
      }
      if ((eventMask & (WATCH_DELETE_SELF | WATCH_MOVE_SELF)) !== 0) {
        this.reattachLogWatchers()
      }
      return
    }

    const paths = this.getLogWatchPaths()
    const childPath = paths[level + 1]
    const childChanged = (eventMask & entryEvents) !== 0 && this.matchesPathEvent(fileName, childPath)
    const selfMovedOrDeleted = (eventMask & (WATCH_DELETE_SELF | WATCH_MOVE_SELF)) !== 0
    if (childChanged) {
      this.attachLogWatchers()
    } else if (selfMovedOrDeleted) {
      this.reattachLogWatchers()
    }
  }

  private matchesPathEvent(fileName: string, path: string): boolean {
    const pathParts = path.split('/')
    const basename = pathParts[pathParts.length - 1]
    return fileName === path || fileName === basename || fileName.endsWith(`/${basename}`)
  }

  private reattachLogWatchers(): void {
    this.stopLogDirectoryWatcher()
    this.attachLogWatchers()
  }

  private stopLogWatchers(): void {
    this.stopLogDirectoryWatcher()
  }

  private stopLogDirectoryWatcher(): void {
    if (this.logDirectoryWatcher !== null) {
      try {
        this.logDirectoryWatcher.stop()
      } catch (_) {
      }
    }
    this.logDirectoryWatcher = null
    this.logDirectoryWatchLevel = -1
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

  // 销毁 — 取消尚未触发的一次性调度，取消所有 observer 订阅。
  destroy(): void {
    this.cancelScheduledCheckpoint()
    this.stopLogWatchers()
    HarmonyDiagnosticEvents.setCheckpointDirtyHandler(null)
    if (this.backgroundFinalizerObserver !== null) {
      HarmonyLifecycle.getInstance().removeObserver(this.backgroundFinalizerObserver)
      this.backgroundFinalizerObserver = null
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
  }
}
