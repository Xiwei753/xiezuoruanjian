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
  ErrorCode,
  WindowMetrics,
  LayoutPlan,
  ScreenPolicy,
  ScreenRole,
  ShellMode
} from '../model/CoreDtos'

import nativeModule from 'libwriter_core.so'

interface NativeJsonEnvelope {
  success: boolean
  data?: Record<string, Object>
  errorCode?: string
  userMessage?: string
}

export class NativeWriterCoreBridge implements IWriterCoreBridge {
  private static instance: NativeWriterCoreBridge
  private isInitialized: boolean = false
  private workspacePath: string = ''
  private nativeModuleAvailable: boolean = true

  /**
   * 防御性获取 native module。
   * 如果 nativeModule 不可用（加载失败或运行时异常），返回错误 ResultEnvelope。
   * 调用方应在 try-catch 中使用此方法，或用 checkNativeError() 包装。
   */
  private getNativeModule(): Record<string, Function> {
    if (!this.nativeModuleAvailable) {
      throw new Error('Native module is not available (previously failed to load)')
    }
    // nativeModule 是顶层 import 的模块对象，ArkTS 不支持运行时 require/import
    // 如果顶层 import 失败，整个模块不会加载，所以这里主要防御运行时调用异常
    if (nativeModule === null || nativeModule === undefined) {
      this.nativeModuleAvailable = false
      throw new Error('Native module is null or undefined')
    }
    return nativeModule as Record<string, Function>
  }

  static getInstance(): NativeWriterCoreBridge {
    if (!NativeWriterCoreBridge.instance) {
      NativeWriterCoreBridge.instance = new NativeWriterCoreBridge()
    }
    return NativeWriterCoreBridge.instance
  }

  private parseEnvelope<T>(jsonStr: string): ResultEnvelope<T> {
    try {
      const obj: NativeJsonEnvelope = JSON.parse(jsonStr) as NativeJsonEnvelope
      const result: ResultEnvelope<T> = {
        success: obj.success,
        data: obj.data as T | undefined,
        errorCode: obj.errorCode as string | undefined,
        userMessage: obj.userMessage
      }
      return result
    } catch (e) {
      const errResult: ResultEnvelope<T> = {
        success: false,
        errorCode: 'PARSE_ERROR',
        userMessage: `JSON parse error: ${e}`
      }
      return errResult
    }
  }

  private notImplemented<T>(): ResultEnvelope<T> {
    return {
      success: false,
      errorCode: 'NOT_IMPLEMENTED',
      userMessage: 'Native bridge not implemented yet'
    }
  }

  /**
   * 通过 Core FFI 的 resolveChapterLocation 查找 chapterId 所在的 projectId 和 volumeId。
   * 不再使用遍历项目树的临时方案。
   */
  private async resolveChapterLocation(chapterId: string): Promise<{ projectId: string, volumeId: string } | null> {
    try {
      const jsonStr: string = this.getNativeModule().nativeResolveChapterLocation(chapterId)
      const result = this.parseEnvelope<{ projectId: string, volumeId: string, chapterId: string }>(jsonStr)
      if (result.success && result.data) {
        return { projectId: result.data.projectId, volumeId: result.data.volumeId }
      }
      return null
    } catch (e) {
      return null
    }
  }

  /**
   * 通过 Core FFI 的 resolveVolumeLocation 查找 volumeId 所属的 projectId。
   * 不再使用遍历项目树的临时方案。
   */
  private async resolveVolumeProjectId(volumeId: string): Promise<string | null> {
    try {
      const jsonStr: string = this.getNativeModule().nativeResolveVolumeLocation(volumeId)
      const result = this.parseEnvelope<{ projectId: string, volumeId: string }>(jsonStr)
      if (result.success && result.data) {
        return result.data.projectId
      }
      return null
    } catch (e) {
      return null
    }
  }

  private lastInitErrorCode: number = 0

  async initialize(workspacePath: string): Promise<boolean> {
    if (this.isInitialized) {
      return true
    }
    try {
      const result: number = this.getNativeModule().nativeInit(workspacePath) as number
      if (result === 0) {
        this.isInitialized = true
        this.workspacePath = workspacePath
        return true
      }
      this.lastInitErrorCode = result
      console.error(`nativeInit failed with code: ${result}, path: ${workspacePath}`)
      return false
    } catch (e) {
      this.lastInitErrorCode = -999
      this.nativeModuleAvailable = false
      console.error(`nativeInit exception: ${e}`)
      return false
    }
  }

  getLastInitErrorCode(): number {
    return this.lastInitErrorCode
  }

  async getLastError(): Promise<string> {
    try {
      const err: string = this.getNativeModule().nativeGetLastError() as string
      return err
    } catch (e) {
      return String(e)
    }
  }

  // ── Workspace ──

  async listWorkspaces(): Promise<ResultEnvelope<WorkspaceSummary[]>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeListWorkspaces()
      return this.parseEnvelope<WorkspaceSummary[]>(jsonStr)
    } catch (e) {
      return this.notImplemented<WorkspaceSummary[]>()
    }
  }

  async openWorkspace(path: string): Promise<ResultEnvelope<WorkspaceSummary>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeOpenWorkspace(path)
      return this.parseEnvelope<WorkspaceSummary>(jsonStr)
    } catch (e) {
      return this.notImplemented<WorkspaceSummary>()
    }
  }

  async validateWorkspace(path: string): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeValidateWorkspace()
      return this.parseEnvelope<boolean>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async getWorkspaceState(): Promise<ResultEnvelope<WorkspaceSummary>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeGetWorkspaceState()
      return this.parseEnvelope<WorkspaceSummary>(jsonStr)
    } catch (e) {
      return this.notImplemented<WorkspaceSummary>()
    }
  }

  // ── Project ──

  async listProjects(): Promise<ResultEnvelope<Project[]>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeListProjects()
      return this.parseEnvelope<Project[]>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async getProjectTree(projectId: string): Promise<ResultEnvelope<ProjectTree>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeGetProjectTree(projectId)
      return this.parseEnvelope<ProjectTree>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async createProject(name: string): Promise<ResultEnvelope<Project>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeCreateProject(name)
      return this.parseEnvelope<Project>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async renameProject(projectId: string, newName: string): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeRenameProject(projectId, newName)
      return this.parseEnvelope<boolean>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async deleteProject(projectId: string): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeDeleteProject(projectId)
      return this.parseEnvelope<boolean>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async getProjectStats(projectId: string): Promise<ResultEnvelope<WritingStats>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeGetProjectStats(projectId)
      return this.parseEnvelope<WritingStats>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  // ── Volume ──

  async listVolumes(projectId: string): Promise<ResultEnvelope<Volume[]>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeListVolumes(projectId)
      return this.parseEnvelope<Volume[]>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async createVolume(projectId: string, name: string): Promise<ResultEnvelope<Volume>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeCreateVolume(projectId, name)
      return this.parseEnvelope<Volume>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async renameVolume(projectId: string, volumeId: string, newName: string): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeRenameVolume(projectId, volumeId, newName)
      return this.parseEnvelope<boolean>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async deleteVolume(projectId: string, volumeId: string): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeDeleteVolume(projectId, volumeId)
      return this.parseEnvelope<boolean>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async reorderVolumes(projectId: string, orderedVolumeIds: string[]): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeReorderVolumes(projectId, JSON.stringify(orderedVolumeIds))
      return this.parseEnvelope<boolean>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  // ── Chapter ──

  async listChapters(volumeId: string): Promise<ResultEnvelope<ChapterMeta[]>> {
    const projectId = await this.resolveVolumeProjectId(volumeId)
    if (!projectId) {
      return { success: false, errorCode: 'VOLUME_NOT_FOUND', userMessage: `卷 ${volumeId} 未找到对应项目` }
    }
    try {
      const jsonStr: string = this.getNativeModule().nativeListChapters(projectId, volumeId)
      return this.parseEnvelope<ChapterMeta[]>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async loadChapter(chapterId: string): Promise<ResultEnvelope<ChapterContent>> {
    const loc = await this.resolveChapterLocation(chapterId)
    if (!loc) {
      return { success: false, errorCode: 'CHAPTER_NOT_FOUND', userMessage: `章节 ${chapterId} 未找到` }
    }
    try {
      const jsonStr: string = this.getNativeModule().nativeOpenChapter(loc.projectId, loc.volumeId, chapterId)
      return this.parseEnvelope<ChapterContent>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async saveChapter(chapterId: string, text: string): Promise<ResultEnvelope<ChapterSaveReceipt>> {
    const loc = await this.resolveChapterLocation(chapterId)
    if (!loc) {
      return { success: false, errorCode: 'CHAPTER_NOT_FOUND', userMessage: `章节 ${chapterId} 未找到` }
    }
    try {
      const jsonStr: string = this.getNativeModule().nativeSaveChapter(loc.projectId, loc.volumeId, chapterId, text)
      return this.parseEnvelope<ChapterSaveReceipt>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async clearChapter(chapterId: string): Promise<ResultEnvelope<ChapterSaveReceipt>> {
    const loc = await this.resolveChapterLocation(chapterId)
    if (!loc) {
      return { success: false, errorCode: 'CHAPTER_NOT_FOUND', userMessage: `章节 ${chapterId} 未找到` }
    }
    try {
      const jsonStr: string = this.getNativeModule().nativeClearChapter(loc.projectId, loc.volumeId, chapterId)
      return this.parseEnvelope<ChapterSaveReceipt>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async createChapter(volumeId: string, name: string): Promise<ResultEnvelope<ChapterMeta>> {
    const projectId = await this.resolveVolumeProjectId(volumeId)
    if (!projectId) {
      return { success: false, errorCode: 'VOLUME_NOT_FOUND', userMessage: `卷 ${volumeId} 未找到对应项目` }
    }
    try {
      const jsonStr: string = this.getNativeModule().nativeCreateChapter(projectId, volumeId, name)
      return this.parseEnvelope<ChapterMeta>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async renameChapter(chapterId: string, newName: string): Promise<ResultEnvelope<boolean>> {
    const loc = await this.resolveChapterLocation(chapterId)
    if (!loc) {
      return { success: false, errorCode: 'CHAPTER_NOT_FOUND', userMessage: `章节 ${chapterId} 未找到` }
    }
    try {
      const jsonStr: string = this.getNativeModule().nativeRenameChapter(loc.projectId, loc.volumeId, chapterId, newName)
      return this.parseEnvelope<boolean>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async deleteChapter(chapterId: string): Promise<ResultEnvelope<boolean>> {
    const loc = await this.resolveChapterLocation(chapterId)
    if (!loc) {
      return { success: false, errorCode: 'CHAPTER_NOT_FOUND', userMessage: `章节 ${chapterId} 未找到` }
    }
    try {
      const jsonStr: string = this.getNativeModule().nativeDeleteChapter(loc.projectId, loc.volumeId, chapterId)
      return this.parseEnvelope<boolean>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async reorderChapters(volumeId: string, orderedChapterIds: string[]): Promise<ResultEnvelope<boolean>> {
    const projectId = await this.resolveVolumeProjectId(volumeId)
    if (!projectId) {
      return { success: false, errorCode: 'VOLUME_NOT_FOUND', userMessage: `卷 ${volumeId} 未找到对应项目` }
    }
    try {
      const jsonStr: string = this.getNativeModule().nativeReorderChapters(projectId, volumeId, JSON.stringify(orderedChapterIds))
      return this.parseEnvelope<boolean>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  // ── StarMap ──

  async listStarMaps(): Promise<ResultEnvelope<StarMapMeta[]>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeListStarMaps()
      return this.parseEnvelope<StarMapMeta[]>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async listStarMapsForProject(projectId: string): Promise<ResultEnvelope<StarMapMeta[]>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeListStarMapsForProject(projectId)
      return this.parseEnvelope<StarMapMeta[]>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async getStarMap(starmapId: string): Promise<ResultEnvelope<StarMapMeta>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeGetStarMap(starmapId)
      return this.parseEnvelope<StarMapMeta>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async getStarMapGraph(starmapId: string): Promise<ResultEnvelope<StarMapGraph>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeGetStarMapGraph(starmapId)
      return this.parseEnvelope<StarMapGraph>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async createStarMap(title: string, description: string): Promise<ResultEnvelope<StarMapMeta>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeCreateStarMap(title, description)
      return this.parseEnvelope<StarMapMeta>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async deleteStarMap(starmapId: string): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeDeleteStarMap(starmapId)
      return this.parseEnvelope<boolean>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async renameStarMap(starmapId: string, newTitle: string): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeRenameStarMap(starmapId, newTitle)
      return this.parseEnvelope<boolean>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  // ── StarMap Layout & Motion ──

  async saveStarMapLayout(starmapId: string, layout: StarMapLayout): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeSaveStarMapLayout(starmapId, JSON.stringify(layout))
      return this.parseEnvelope<boolean>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async getStarMapLayout(starmapId: string): Promise<ResultEnvelope<StarMapLayout>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeGetStarMapLayout(starmapId)
      return this.parseEnvelope<StarMapLayout>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async getStarMapMotionPolicy(): Promise<ResultEnvelope<StarMapMotionPolicy>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeGetStarMapMotionPolicy()
      return this.parseEnvelope<StarMapMotionPolicy>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async computeStarMapEdgeRenders(graph: StarMapGraph): Promise<ResultEnvelope<StarMapEdgeRender[]>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeComputeStarMapEdgeRenders(JSON.stringify(graph))
      return this.parseEnvelope<StarMapEdgeRender[]>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  // ── Settings ──

  async getLocalSettings(): Promise<ResultEnvelope<LocalSettings>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeLoadLocalSettings()
      return this.parseEnvelope<LocalSettings>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async saveLocalSettings(settings: LocalSettings): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeSaveLocalSettings(JSON.stringify(settings))
      return this.parseEnvelope<boolean>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async getSyncableSettings(): Promise<ResultEnvelope<SyncableSettings>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeLoadSyncableSettings()
      return this.parseEnvelope<SyncableSettings>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async saveSyncableSettings(settings: SyncableSettings): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeSaveSyncableSettings(JSON.stringify(settings))
      return this.parseEnvelope<boolean>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }


  // ── Sync ──

  async loadSyncConfig(): Promise<ResultEnvelope<SyncConfig>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeLoadSyncConfig()
      return this.parseEnvelope<SyncConfig>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async saveSyncConfig(config: SyncConfig): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeSaveSyncConfig(JSON.stringify(config))
      return this.parseEnvelope<boolean>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async syncDryRun(): Promise<ResultEnvelope<SyncResult>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeSyncDryRun()
      return this.parseEnvelope<SyncResult>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }


  async performSync(): Promise<ResultEnvelope<SyncResult>> {
    try {
      const jsonStr: string = this.getNativeModule().nativePerformSync()
      return this.parseEnvelope<SyncResult>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async syncDiagnostics(): Promise<ResultEnvelope<SyncDiagnosticsResult>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeSyncDiagnostics()
      return this.parseEnvelope<SyncDiagnosticsResult>(jsonStr)
    } catch (err) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: `syncDiagnostics failed: ${err}` }
    }
  }

  // ── Stats ──

  async getWritingStats(): Promise<ResultEnvelope<WritingStats>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeGetWritingStats()
      return this.parseEnvelope<WritingStats>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async calculateWordCount(text: string): Promise<number> {
    if (!this.isInitialized) {
      return -1
    }
    try {
      return this.getNativeModule().nativeCalculateWordCount(text)
    } catch (e) {
      return -1
    }
  }

  async processWritingEvent(deviceId: string, platform: string, projectId: string, volumeId: string,
    chapterId: string, oldText: string, newText: string, durationSeconds: number, sessionId: string): Promise<ResultEnvelope<boolean>> {
    try {
      const eventJson = JSON.stringify({
        deviceId: deviceId,
        platform: platform,
        projectId: projectId,
        volumeId: volumeId,
        chapterId: chapterId,
        oldText: oldText,
        newText: newText,
        durationSeconds: durationSeconds,
        sessionId: sessionId
      })
      const jsonStr: string = this.getNativeModule().nativeProcessWritingEvent(eventJson)
      return this.parseEnvelope<boolean>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async getRecentEdits(): Promise<ResultEnvelope<RecentEdit[]>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeGetRecentEdits()
      return this.parseEnvelope<RecentEdit[]>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  // ── Native Status ──

  async getNativeLoadStatus(): Promise<ResultEnvelope<string>> {
    try {
      const status: string = this.getNativeModule().nativeGetLoadStatus() as string
      return { success: true, data: status }
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async isWorkspaceValid(): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeValidateWorkspace() as string
      return this.parseEnvelope<boolean>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async isAiAvailable(): Promise<ResultEnvelope<boolean>> {
    try {
      const available: number = this.getNativeModule().nativeIsAiAvailable() as number
      return { success: true, data: available === 1 }
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  // ── Layout Policy ──

  async resolveLayout(metrics: WindowMetrics): Promise<ResultEnvelope<LayoutPlan>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeResolveLayout(JSON.stringify(metrics))
      return this.parseEnvelope<LayoutPlan>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  // ── Screen Policy ──

  async resolveScreenPolicy(screenRole: ScreenRole, shellMode: ShellMode): Promise<ResultEnvelope<ScreenPolicy>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeResolveScreenPolicy(
        JSON.stringify(screenRole),
        JSON.stringify(shellMode)
      )
      return this.parseEnvelope<ScreenPolicy>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }
}
