import { IWriterCoreBridge } from './IWriterCoreBridge'
import {
  WorkspaceSummary,
  Project,
  Volume,
  ChapterMeta,
  ChapterContent,
  ChapterSaveReceipt,
  StarMapMeta,
  StarMapGraph,
  StarMapLayout,
  StarMapEdgeRender,
  StarMapMotionPolicy,
  LocalSettings,
  SyncableSettings,
  ResultEnvelope,
  ProjectTree,
  WritingStats,
  SyncConfig,
  SyncResult,
  SyncDiagnosticsResult,
  RecentEdit,
  ChangedEntity,
  WindowMetrics,
  LayoutPlan,
  ScreenPolicy,
  ScreenRole,
  ShellMode
} from '../model/CoreDtos'
import { NativeCoreModule } from './NativeCoreModule'
import { NativeWorkspaceBridge } from './NativeWorkspaceBridge'
import { NativeProjectBridge } from './NativeProjectBridge'
import { NativeChapterBridge } from './NativeChapterBridge'
import { NativeSettingsBridge } from './NativeSettingsBridge'
import { NativeSyncBridge } from './NativeSyncBridge'
import { NativeStatsBridge } from './NativeStatsBridge'
import { NativeStarMapBridge } from './NativeStarMapBridge'
import { NativeLayoutBridge } from './NativeLayoutBridge'

export class NativeWriterCoreBridge implements IWriterCoreBridge {
  private static instance: NativeWriterCoreBridge
  private isInitialized: boolean = false
  private workspacePath: string = ''
  private lastInitErrorCode: number = 0

  // ── 公共模块 & 分域 Bridge ──
  private core = new NativeCoreModule()
  private workspaceBridge = new NativeWorkspaceBridge(this.core)
  private projectBridge = new NativeProjectBridge(this.core)
  private chapterBridge = new NativeChapterBridge(this.core)
  private settingsBridge = new NativeSettingsBridge(this.core)
  private syncBridge = new NativeSyncBridge(this.core)
  private statsBridge = new NativeStatsBridge(this.core)
  private starMapBridge = new NativeStarMapBridge(this.core)
  private layoutBridge = new NativeLayoutBridge(this.core)

  static getInstance(): NativeWriterCoreBridge {
    if (!NativeWriterCoreBridge.instance) {
      NativeWriterCoreBridge.instance = new NativeWriterCoreBridge()
    }
    return NativeWriterCoreBridge.instance
  }

  async initialize(workspacePath: string): Promise<boolean> {
    if (this.isInitialized) {
      return true
    }
    try {
      const result: number = this.core.getNativeModule().nativeInit(workspacePath) as number
      if (result === 0) {
        this.isInitialized = true
        this.workspacePath = workspacePath
        this.statsBridge.setInitialized(true)
        return true
      }
      this.lastInitErrorCode = result
      console.error(`nativeInit failed with code: ${result}, path: ${workspacePath}`)
      return false
    } catch (e) {
      this.lastInitErrorCode = -999
      this.core.markNativeUnavailable()
      console.error(`nativeInit exception: ${e}`)
      return false
    }
  }

  getLastInitErrorCode(): number {
    return this.lastInitErrorCode
  }

  async getLastError(): Promise<string> {
    try {
      const err: string = this.core.getNativeModule().nativeGetLastError() as string
      return err
    } catch (e) {
      return String(e)
    }
  }

  // ── Workspace ──

  async listWorkspaces(): Promise<ResultEnvelope<WorkspaceSummary[]>> {
    return this.workspaceBridge.listWorkspaces()
  }

  async openWorkspace(path: string): Promise<ResultEnvelope<WorkspaceSummary>> {
    return this.workspaceBridge.openWorkspace(path)
  }

  async validateWorkspace(path: string): Promise<ResultEnvelope<boolean>> {
    return this.workspaceBridge.validateWorkspace(path)
  }

  async getWorkspaceState(): Promise<ResultEnvelope<WorkspaceSummary>> {
    return this.workspaceBridge.getWorkspaceState()
  }

  // ── Project ──

  async listProjects(): Promise<ResultEnvelope<Project[]>> {
    return this.projectBridge.listProjects()
  }

  async getProjectTree(projectId: string): Promise<ResultEnvelope<ProjectTree>> {
    return this.projectBridge.getProjectTree(projectId)
  }

  async createProject(name: string): Promise<ResultEnvelope<Project>> {
    return this.projectBridge.createProject(name)
  }

  async renameProject(projectId: string, newName: string): Promise<ResultEnvelope<boolean>> {
    return this.projectBridge.renameProject(projectId, newName)
  }

  async deleteProject(projectId: string): Promise<ResultEnvelope<boolean>> {
    return this.projectBridge.deleteProject(projectId)
  }

  async getProjectStats(projectId: string): Promise<ResultEnvelope<WritingStats>> {
    return this.projectBridge.getProjectStats(projectId)
  }

  // ── Volume ──

  async listVolumes(projectId: string): Promise<ResultEnvelope<Volume[]>> {
    return this.projectBridge.listVolumes(projectId)
  }

  async createVolume(projectId: string, name: string): Promise<ResultEnvelope<Volume>> {
    return this.projectBridge.createVolume(projectId, name)
  }

  async renameVolume(projectId: string, volumeId: string, newName: string): Promise<ResultEnvelope<boolean>> {
    return this.projectBridge.renameVolume(projectId, volumeId, newName)
  }

  async deleteVolume(projectId: string, volumeId: string): Promise<ResultEnvelope<boolean>> {
    return this.projectBridge.deleteVolume(projectId, volumeId)
  }

  async reorderVolumes(projectId: string, orderedVolumeIds: string[]): Promise<ResultEnvelope<boolean>> {
    return this.projectBridge.reorderVolumes(projectId, orderedVolumeIds)
  }

  // ── Chapter ──

  async listChapters(volumeId: string): Promise<ResultEnvelope<ChapterMeta[]>> {
    return this.chapterBridge.listChapters(volumeId)
  }

  async loadChapter(chapterId: string): Promise<ResultEnvelope<ChapterContent>> {
    return this.chapterBridge.loadChapter(chapterId)
  }

  async saveChapter(chapterId: string, text: string): Promise<ResultEnvelope<ChapterSaveReceipt>> {
    return this.chapterBridge.saveChapter(chapterId, text)
  }

  async clearChapter(chapterId: string): Promise<ResultEnvelope<ChapterSaveReceipt>> {
    return this.chapterBridge.clearChapter(chapterId)
  }

  async createChapter(volumeId: string, name: string): Promise<ResultEnvelope<ChapterMeta>> {
    return this.chapterBridge.createChapter(volumeId, name)
  }

  async renameChapter(chapterId: string, newName: string): Promise<ResultEnvelope<boolean>> {
    return this.chapterBridge.renameChapter(chapterId, newName)
  }

  async deleteChapter(chapterId: string): Promise<ResultEnvelope<boolean>> {
    return this.chapterBridge.deleteChapter(chapterId)
  }

  async reorderChapters(volumeId: string, orderedChapterIds: string[]): Promise<ResultEnvelope<boolean>> {
    return this.chapterBridge.reorderChapters(volumeId, orderedChapterIds)
  }

  // ── StarMap ──

  async listStarMaps(): Promise<ResultEnvelope<StarMapMeta[]>> {
    return this.starMapBridge.listStarMaps()
  }

  async listStarMapsForProject(projectId: string): Promise<ResultEnvelope<StarMapMeta[]>> {
    return this.starMapBridge.listStarMapsForProject(projectId)
  }

  async getStarMap(starmapId: string): Promise<ResultEnvelope<StarMapMeta>> {
    return this.starMapBridge.getStarMap(starmapId)
  }

  async getStarMapGraph(starmapId: string): Promise<ResultEnvelope<StarMapGraph>> {
    return this.starMapBridge.getStarMapGraph(starmapId)
  }

  async createStarMap(title: string, description: string): Promise<ResultEnvelope<StarMapMeta>> {
    return this.starMapBridge.createStarMap(title, description)
  }

  async deleteStarMap(starmapId: string): Promise<ResultEnvelope<boolean>> {
    return this.starMapBridge.deleteStarMap(starmapId)
  }

  async renameStarMap(starmapId: string, newTitle: string): Promise<ResultEnvelope<boolean>> {
    return this.starMapBridge.renameStarMap(starmapId, newTitle)
  }

  async saveStarMapLayout(starmapId: string, layout: StarMapLayout): Promise<ResultEnvelope<boolean>> {
    return this.starMapBridge.saveStarMapLayout(starmapId, layout)
  }

  async getStarMapLayout(starmapId: string): Promise<ResultEnvelope<StarMapLayout>> {
    return this.starMapBridge.getStarMapLayout(starmapId)
  }

  async getStarMapMotionPolicy(): Promise<ResultEnvelope<StarMapMotionPolicy>> {
    return this.starMapBridge.getStarMapMotionPolicy()
  }

  async computeStarMapEdgeRenders(graph: StarMapGraph): Promise<ResultEnvelope<StarMapEdgeRender[]>> {
    return this.starMapBridge.computeStarMapEdgeRenders(graph)
  }

  // ── Settings ──

  async getLocalSettings(): Promise<ResultEnvelope<LocalSettings>> {
    return this.settingsBridge.getLocalSettings()
  }

  async saveLocalSettings(settings: LocalSettings): Promise<ResultEnvelope<boolean>> {
    return this.settingsBridge.saveLocalSettings(settings)
  }

  async getSyncableSettings(): Promise<ResultEnvelope<SyncableSettings>> {
    return this.settingsBridge.getSyncableSettings()
  }

  async saveSyncableSettings(settings: SyncableSettings): Promise<ResultEnvelope<boolean>> {
    return this.settingsBridge.saveSyncableSettings(settings)
  }

  // ── Sync ──

  async loadSyncConfig(): Promise<ResultEnvelope<SyncConfig>> {
    return this.syncBridge.loadSyncConfig()
  }

  async saveSyncConfig(config: SyncConfig): Promise<ResultEnvelope<boolean>> {
    return this.syncBridge.saveSyncConfig(config)
  }

  async syncDryRun(): Promise<ResultEnvelope<SyncResult>> {
    return this.syncBridge.syncDryRun()
  }

  async performSync(): Promise<ResultEnvelope<SyncResult>> {
    return this.syncBridge.performSync()
  }

  async syncDiagnostics(): Promise<ResultEnvelope<SyncDiagnosticsResult>> {
    return this.syncBridge.syncDiagnostics()
  }

  // ── Stats ──

  async getWritingStats(): Promise<ResultEnvelope<WritingStats>> {
    return this.statsBridge.getWritingStats()
  }

  async calculateWordCount(text: string): Promise<number> {
    return this.statsBridge.calculateWordCount(text)
  }

  async processWritingEvent(deviceId: string, platform: string, projectId: string, volumeId: string,
    chapterId: string, oldText: string, newText: string, durationSeconds: number, sessionId: string): Promise<ResultEnvelope<boolean>> {
    return this.statsBridge.processWritingEvent(deviceId, platform, projectId, volumeId, chapterId, oldText, newText, durationSeconds, sessionId)
  }

  async getRecentEdits(): Promise<ResultEnvelope<RecentEdit[]>> {
    return this.statsBridge.getRecentEdits()
  }

  // ── Native Status ──

  async getNativeLoadStatus(): Promise<ResultEnvelope<string>> {
    try {
      const status: string = this.core.getNativeModule().nativeGetLoadStatus() as string
      const result: ResultEnvelope<string> = {
        success: true,
        data: status,
        warnings: [] as string[],
        changedPaths: [] as string[],
        changedEntities: [] as ChangedEntity[]
      }
      return result
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', messageKey: 'error.native_error', messageArgs: { 'detail': String(e) } as Record<string, string>, rawError: String(e), warnings: [] as string[], changedPaths: [] as string[], changedEntities: [] as ChangedEntity[] }
    }
  }

  async isWorkspaceValid(): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeValidateWorkspace() as string
      return this.core.parseEnvelope<boolean>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', messageKey: 'error.native_error', messageArgs: { 'detail': String(e) } as Record<string, string>, rawError: String(e), warnings: [] as string[], changedPaths: [] as string[], changedEntities: [] as ChangedEntity[] }
    }
  }

  async isAiAvailable(): Promise<ResultEnvelope<boolean>> {
    try {
      const available: number = this.core.getNativeModule().nativeIsAiAvailable() as number
      const result: ResultEnvelope<boolean> = {
        success: true,
        data: available === 1,
        warnings: [] as string[],
        changedPaths: [] as string[],
        changedEntities: [] as ChangedEntity[]
      }
      return result
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', messageKey: 'error.native_error', messageArgs: { 'detail': String(e) } as Record<string, string>, rawError: String(e), warnings: [] as string[], changedPaths: [] as string[], changedEntities: [] as ChangedEntity[] }
    }
  }

  // ── Layout Policy ──

  async resolveLayout(metrics: WindowMetrics): Promise<ResultEnvelope<LayoutPlan>> {
    return this.layoutBridge.resolveLayout(metrics)
  }

  // ── Screen Policy ──

  async resolveScreenPolicy(screenRole: ScreenRole, shellMode: ShellMode): Promise<ResultEnvelope<ScreenPolicy>> {
    return this.layoutBridge.resolveScreenPolicy(screenRole, shellMode)
  }
}
