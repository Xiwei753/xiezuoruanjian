// HarmonyDiagnosticsCheckpoint — 运行态快照与归档元数据管理器。
//
// Issue #1017 职责重划：
// - 正常导航/光标/输入/窗口/主题变化只更新内存状态，并 debounce 一次小型运行态
//   JSON 快照；不再由每条结构化事件或每个 CLOSE_WRITE 触发整包 ZIP。
// - 归档只由显式导出、真实故障收尾和后台收尾发起；文件枚举、读取、脱敏、
//   压缩和 latest/previous 发布全部在 DiagnosticsArchiveWorker 执行。
// - 本类只维护最近归档时间/路径/计数/最近故障 eventKey，并通过 Worker 写入
//   snapshots/archive-state.json；ArkUI 线程不做整包 I/O。
//
// 目录结构：
//   context.filesDir/diagnostics/
//     live/log/          ← Rust writer 持续 append 的日志
//     live/runtime/      ← 运行态 JSON 快照（Worker 双槽原子写入）
//     snapshots/
//       sujian-diagnostics-latest.zip
//       sujian-diagnostics-previous.zip
//       archive-state.json

import { common } from '@kit.AbilityKit'
import hilog from '@ohos.hilog'

import { HarmonyDiagnosticsExporter } from './HarmonyDiagnosticsExporter'
import { HarmonyDiagnosticsRuntimeSnapshot } from './HarmonyDiagnosticsRuntimeSnapshot'
import { HarmonyThemeAdapter, ThemeObserver, ThemeConfig } from '../../ui/theme/HarmonyThemeAdapter'
import { NavigationChromeStore } from '../../app/navigation/NavigationChromeStore'
import {
  EditorRuntimeSnapshotStore,
  EditorRuntimeSnapshot,
  EditorSnapshotObserver
} from '../../feature/editor/diagnostics/EditorRuntimeSnapshotStore'
import { ScreenRole } from '../../app/navigation/ScreenSemantics'
import { AdaptiveContext, AdaptiveObserver, AdaptiveState } from '../../ui/adaptive/AdaptiveContext'
import { HarmonyLifecycle, LifecycleObserver } from '../lifecycle/HarmonyLifecycle'

const LOG_TAG = 'HarmonyDiagnosticsCheckpoint'
const DIAGNOSTICS_DIR = 'diagnostics'
const SNAPSHOT_DEBOUNCE_MS = 400

// archive-state.json 只保存稳定元数据，不包含日志内容、路径以外的用户数据。
interface ArchiveStateDto {
  schemaVersion: number
  lastArchiveAtMs: number
  lastArchivePath: string
  archiveCount: number
  lastFaultEventKey: string
}

class CheckpointThemeObserver implements ThemeObserver {
  private checkpoint: HarmonyDiagnosticsCheckpoint

  constructor(checkpoint: HarmonyDiagnosticsCheckpoint) {
    this.checkpoint = checkpoint
  }

  onThemeChanged(_theme: ThemeConfig): void {
    this.checkpoint.persistRuntimeSnapshot()
  }
}

type CheckpointNavObserver = (role: ScreenRole) => void

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
  private initialized: boolean = false
  private isForeground: boolean = true
  private backgroundFinalizationPending: boolean = false
  private backgroundTransitionVersion: number = 0
  private snapshotTimerId: number = -1
  private snapshotPending: boolean = false
  private lastArchiveAtMs: number = 0
  private lastArchivePath: string = ''
  private archiveCount: number = 0
  private lastFaultEventKey: string = ''
  private backgroundFinalizerObserver: LifecycleObserver | null = null
  private themeObserver: CheckpointThemeObserver | null = null
  private navObserver: CheckpointNavObserver | null = null
  private editorObserver: EditorSnapshotObserver | null = null
  private adaptiveObserver: CheckpointAdaptiveObserver | null = null

  private constructor() {
    this.exporter = new HarmonyDiagnosticsExporter()
  }

  static getInstance(): HarmonyDiagnosticsCheckpoint {
    if (!HarmonyDiagnosticsCheckpoint.instance) {
      HarmonyDiagnosticsCheckpoint.instance = new HarmonyDiagnosticsCheckpoint()
    }
    return HarmonyDiagnosticsCheckpoint.instance
  }

  init(context: common.UIAbilityContext): void {
    if (this.initialized) {
      return
    }
    this.initialized = true
    this.isForeground = true
    this.backgroundFinalizationPending = false
    this.context = context
    this.exporter.setAbilityContext(context)

    this.backgroundFinalizerObserver = new CheckpointBackgroundFinalizerObserver(this)
    HarmonyLifecycle.getInstance().addObserver(this.backgroundFinalizerObserver)

    this.themeObserver = new CheckpointThemeObserver(this)
    HarmonyThemeAdapter.getInstance().addObserver(this.themeObserver)

    this.navObserver = (_role: ScreenRole): void => {
      this.persistRuntimeSnapshot()
    }
    NavigationChromeStore.getInstance().subscribe(this.navObserver)

    this.editorObserver = (_snapshot: EditorRuntimeSnapshot | null): void => {
      this.persistRuntimeSnapshot()
    }
    EditorRuntimeSnapshotStore.getInstance().addObserver(this.editorObserver)

    this.adaptiveObserver = new CheckpointAdaptiveObserver(this)
    AdaptiveContext.getInstance().addObserver(this.adaptiveObserver)

    // 首帧只落运行态快照；不在这里强制生成 latest.zip，避免启动即压缩整包。
    this.persistRuntimeSnapshot()
  }

  // 兼容旧调用点：运行态变化只安排轻量快照，不再触发归档。
  markDirty(): void {
    this.persistRuntimeSnapshot()
  }

  // 显式归档入口：仅由用户导出/后台收尾等明确动作调用。
  requestCheckpoint(): void {
    void this.runArchive('checkpoint', '')
  }

  onForeground(): void {
    const wasForeground = this.isForeground
    this.isForeground = true
    this.backgroundTransitionVersion += 1
    this.backgroundFinalizationPending = false
    if (!wasForeground) {
      this.persistRuntimeSnapshot()
    }
  }

  onBackground(): void {
    if (!this.isForeground) {
      return
    }
    this.isForeground = false
    this.backgroundTransitionVersion += 1
    this.backgroundFinalizationPending = true
    this.cancelSnapshotTimer()
    // 后台立即落一份小型运行态快照；真正的 ZIP 等所有后台保存完成后再做。
    void this.persistRuntimeSnapshotNow()
  }

  // Lifecycle 在所有后台保存完成后、释放 suspend-delay 之前 await 本方法。
  // 这里只请求一次 checkpoint 包，不依赖前台或日志 watcher。
  async finalizeBackground(): Promise<void> {
    if (!this.initialized || !this.backgroundFinalizationPending || this.isForeground) {
      return
    }
    const transitionVersion = this.backgroundTransitionVersion
    try {
      await this.persistRuntimeSnapshotNow()
      await this.runArchive('checkpoint', '')
    } finally {
      if (this.backgroundTransitionVersion === transitionVersion) {
        this.backgroundFinalizationPending = false
      }
    }
  }

  // 高频入口只做 debounce；实际 JSON 构建仍在 ArkUI 线程（轻量），文件写入在 Worker。
  persistRuntimeSnapshot(): void {
    if (!this.initialized || this.context === null) {
      return
    }
    this.snapshotPending = true
    if (this.snapshotTimerId !== -1) {
      return
    }
    this.snapshotTimerId = setTimeout(() => {
      this.snapshotTimerId = -1
      if (!this.snapshotPending) {
        return
      }
      this.snapshotPending = false
      void this.persistRuntimeSnapshotNow()
    }, SNAPSHOT_DEBOUNCE_MS)
  }

  private async persistRuntimeSnapshotNow(): Promise<void> {
    if (this.context === null) {
      return
    }
    this.cancelSnapshotTimer()
    this.snapshotPending = false
    await HarmonyDiagnosticsRuntimeSnapshot.getInstance().persistAll(
      this.context, this.buildArchiveStateJson()
    )
  }

  private buildArchiveStateJson(): string {
    const state: ArchiveStateDto = {
      schemaVersion: 1,
      lastArchiveAtMs: this.lastArchiveAtMs,
      lastArchivePath: this.lastArchivePath,
      archiveCount: this.archiveCount,
      lastFaultEventKey: this.lastFaultEventKey
    }
    return JSON.stringify(state, null, 2)
  }

  private async runArchive(kind: 'checkpoint' | 'fault', eventKey: string): Promise<void> {
    if (!this.initialized || this.context === null) {
      return
    }
    try {
      const archivePath = await this.exporter.exportTo(this.snapshotsDir(), kind, eventKey)
      // Ability 可能在归档期间销毁并终止 Worker；此时不要再更新元数据或重新拉起 Worker。
      if (!this.initialized || this.context === null) {
        return
      }
      if (archivePath === null) {
        hilog.warn(0x0000, LOG_TAG, 'checkpoint archive failed kind=%{public}s', kind)
        return
      }
      this.lastArchiveAtMs = Date.now()
      this.lastArchivePath = archivePath
      this.archiveCount += 1
      if (kind === 'fault') {
        this.lastFaultEventKey = eventKey
      }
      await this.persistRuntimeSnapshotNow()
    } catch (e) {
      hilog.error(0x0000, LOG_TAG, 'checkpoint archive exception kind=%{public}s reason=%{public}s',
        kind, e instanceof Error ? e.name : 'unknown')
    }
  }

  private snapshotsDir(): string {
    return this.context === null ? '' : `${this.context.filesDir}/${DIAGNOSTICS_DIR}/snapshots`
  }

  private cancelSnapshotTimer(): void {
    if (this.snapshotTimerId !== -1) {
      clearTimeout(this.snapshotTimerId)
      this.snapshotTimerId = -1
    }
  }

  destroy(): void {
    this.cancelSnapshotTimer()
    this.snapshotPending = false
    if (this.backgroundFinalizerObserver !== null) {
      HarmonyLifecycle.getInstance().removeObserver(this.backgroundFinalizerObserver)
      this.backgroundFinalizerObserver = null
    }
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
    if (this.adaptiveObserver !== null) {
      AdaptiveContext.getInstance().removeObserver(this.adaptiveObserver)
      this.adaptiveObserver = null
    }
    this.initialized = false
    this.context = null
  }
}
