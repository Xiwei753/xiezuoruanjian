import { IWriterCoreBridge } from './IWriterCoreBridge'
import { EditorVisualTransaction, EditorTransactionCause } from './dto/EditorDtos'
import { WindowCapabilities, LayoutContract, ScreenPolicy, ScreenRole } from './dto/PlatformDtos'
import { Project, Volume, ChapterMeta, ChapterContent, ChapterSaveReceipt, ProjectTree, WritingStats } from './dto/ProjectDtos'
import { AppSummary, ResultEnvelope, RecentEdit, ChangedEntity } from './dto/ResultEnvelope'
import { LocalSettings, SyncableSettings, ThemePaletteRecordDto, BuiltinThemeDto } from './dto/SettingsDtos'
import { StarMapMeta, StarMapGraph, StarMapLayout, StarMapEdgeRender, StarMapMotionPolicy } from './dto/StarMapDtos'
import { SyncConfig, SyncResult, SyncDiagnosticsResult } from './dto/SyncDtos'
import { NativeCoreModule } from './native/NativeCoreModule'
import { NativeAppStateBridge } from './native/NativeAppStateBridge'
import { NativeProjectBridge } from './native/NativeProjectBridge'
import { NativeChapterBridge } from './native/NativeChapterBridge'
import { NativeSettingsBridge } from './native/NativeSettingsBridge'
import { NativeSyncBridge } from './native/NativeSyncBridge'
import { NativeStatsBridge } from './native/NativeStatsBridge'
import { NativeStarMapBridge } from './native/NativeStarMapBridge'

export class NativeWriterCoreBridge implements IWriterCoreBridge {
  private static instance: NativeWriterCoreBridge
  private isInitialized: boolean = false
  private appDataRoot: string = ''
  private lastInitErrorCode: number = 0

  // ── 公共模块 & 分域 Bridge ──
  private core = new NativeCoreModule()
  private appStateBridge = new NativeAppStateBridge(this.core)
  private projectBridge = new NativeProjectBridge(this.core)
  private chapterBridge = new NativeChapterBridge(this.core)
  private settingsBridge = new NativeSettingsBridge(this.core)
  private syncBridge = new NativeSyncBridge(this.core)
  private statsBridge = new NativeStatsBridge(this.core)
  private starMapBridge = new NativeStarMapBridge(this.core)

  static getInstance(): NativeWriterCoreBridge {
    if (!NativeWriterCoreBridge.instance) {
      NativeWriterCoreBridge.instance = new NativeWriterCoreBridge()
    }
    return NativeWriterCoreBridge.instance
  }

  async initialize(appDataRoot: string): Promise<boolean> {
    if (this.isInitialized) {
      return true
    }
    try {
      const result: number = this.core.getNativeModule().nativeInit(appDataRoot) as number
      if (result === 0) {
        this.isInitialized = true
        this.appDataRoot = appDataRoot
        this.statsBridge.setInitialized(true)
        return true
      }
      this.lastInitErrorCode = result
      console.error(`nativeInit failed with code: ${result}, path: ${appDataRoot}`)
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

  // ── App State ──

  async listAppSummaries(): Promise<ResultEnvelope<AppSummary[]>> {
    return this.appStateBridge.listAppSummaries()
  }

  async getAppState(): Promise<ResultEnvelope<AppSummary>> {
    return this.appStateBridge.getAppState()
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

  async listPaletteRecords(): Promise<ResultEnvelope<ThemePaletteRecordDto[]>> {
    return this.settingsBridge.listPaletteRecords()
  }

  async loadPaletteRecord(deviceId: string, fingerprint: string): Promise<ResultEnvelope<ThemePaletteRecordDto>> {
    return this.settingsBridge.loadPaletteRecord(deviceId, fingerprint)
  }

  async deletePaletteRecord(deviceId: string, fingerprint: string): Promise<ResultEnvelope<boolean>> {
    return this.settingsBridge.deletePaletteRecord(deviceId, fingerprint)
  }

  async listBuiltinThemes(): Promise<ResultEnvelope<BuiltinThemeDto[]>> {
    return this.settingsBridge.listBuiltinThemes()
  }

  // ── Sync（作品级，per-project:<projectId>） ──

  async loadSyncConfig(projectId: string): Promise<ResultEnvelope<SyncConfig>> {
    return this.syncBridge.loadSyncConfig(projectId)
  }

  async saveSyncConfig(projectId: string, config: SyncConfig): Promise<ResultEnvelope<boolean>> {
    return this.syncBridge.saveSyncConfig(projectId, config)
  }

  async syncDryRun(projectId: string): Promise<ResultEnvelope<SyncResult>> {
    return this.syncBridge.syncDryRun(projectId)
  }

  async performSync(projectId: string): Promise<ResultEnvelope<SyncResult>> {
    return this.syncBridge.performSync(projectId)
  }

  async syncDiagnostics(projectId: string): Promise<ResultEnvelope<SyncDiagnosticsResult>> {
    return this.syncBridge.syncDiagnostics(projectId)
  }

  // ── 应用级同步通道（Issue #600 评论 #3 问题四） ──
  // 应用级 Git 仓库根 = app_data_root，同步设置/全局星图/主题调色板。
  // AppSyncProfile 与 ProjectSyncProfile(projectId) 在 API/数据模型里明确区分。

  async loadAppSyncConfig(): Promise<ResultEnvelope<SyncConfig>> {
    return this.syncBridge.loadAppSyncConfig()
  }

  async saveAppSyncConfig(config: SyncConfig): Promise<ResultEnvelope<boolean>> {
    return this.syncBridge.saveAppSyncConfig(config)
  }

  async performAppSync(): Promise<ResultEnvelope<SyncResult>> {
    return this.syncBridge.performAppSync()
  }

  async appSyncDiagnostics(): Promise<ResultEnvelope<SyncDiagnosticsResult>> {
    return this.syncBridge.appSyncDiagnostics()
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

  // ── Layout Contract（#610） ──

  async resolveLayout(capabilities: WindowCapabilities): Promise<ResultEnvelope<LayoutContract>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeResolveLayout(JSON.stringify(capabilities))
      return this.core.parseEnvelope<LayoutContract>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', messageKey: 'error.native_error', messageArgs: { 'detail': String(e) } as Record<string, string>, rawError: String(e), warnings: [] as string[], changedPaths: [] as string[], changedEntities: [] as ChangedEntity[] }
    }
  }

  // ── Screen Contract（#610：不随壳层变化） ──

  async resolveScreenPolicy(screenRole: ScreenRole): Promise<ResultEnvelope<ScreenPolicy>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeResolveScreenPolicy(JSON.stringify(screenRole))
      return this.core.parseEnvelope<ScreenPolicy>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', messageKey: 'error.native_error', messageArgs: { 'detail': String(e) } as Record<string, string>, rawError: String(e), warnings: [] as string[], changedPaths: [] as string[], changedEntities: [] as ChangedEntity[] }
    }
  }

  // ── Editor Animation ──
  // 鸿蒙端动画暂未实现，返回 NOT_IMPLEMENTED。
  // 等 NAPI 层接入 writer_core_editor_visual_transaction 后替换为真实调用。

  async editorVisualTransaction(
    oldText: string,
    newText: string,
    oldCursorIndex: number,
    newCursorIndex: number,
    cause: EditorTransactionCause,
    maxAnimatedChars: number,
    animationDurationMs: number
  ): Promise<ResultEnvelope<EditorVisualTransaction | null>> {
    return {
      success: false,
      errorCode: 'NOT_IMPLEMENTED',
      messageKey: 'error.not_implemented',
      messageArgs: { 'feature': 'editor_visual_transaction' } as Record<string, string>,
      warnings: [] as string[],
      changedPaths: [] as string[],
      changedEntities: [] as ChangedEntity[]
    }
  }
}
