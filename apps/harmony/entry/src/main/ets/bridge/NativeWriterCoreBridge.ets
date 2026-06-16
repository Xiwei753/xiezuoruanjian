import { IWriterCoreBridge } from './IWriterCoreBridge'
import {
  Workspace,
  WorkspaceState,
  Project,
  Volume,
  Chapter,
  ChapterData,
  SaveReceipt,
  StarMapMeta,
  StarMapGraph,
  LocalSettings,
  SyncableSettings,
  EffectiveSettings,
  ResultEnvelope,
  ProjectTree,
  VolumeTree,
  WritingStats,
  SyncConfig,
  SyncReport,
  SyncDiagnostics,
  SyncResult,
  RecentEdit,
  ErrorCode,
  WindowMetrics,
  LayoutPlan
} from '../model/CoreDtos'

import nativeModule from 'libwriter_core.so'

interface ChapterLocation {
  projectId: string
  volumeId: string
}

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
  private chapterIndex: Map<string, ChapterLocation> = new Map()
  private volumeProjectIndex: Map<string, string> = new Map()
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
        errorCode: obj.errorCode as ErrorCode | undefined,
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

  private resolveChapter(chapterId: string): ChapterLocation | null {
    const loc = this.chapterIndex.get(chapterId)
    if (loc) {
      return loc
    }
    return null
  }

  private invalidateChapterIndex(volumeId?: string): void {
    if (volumeId) {
      const entriesToRemove: string[] = []
      this.chapterIndex.forEach((loc, chId) => {
        if (loc.volumeId === volumeId) {
          entriesToRemove.push(chId)
        }
      })
      for (const chId of entriesToRemove) {
        this.chapterIndex.delete(chId)
      }
    } else {
      this.chapterIndex.clear()
    }
  }

  private invalidateVolumeProjectIndex(projectId?: string): void {
    if (projectId) {
      const entriesToRemove: string[] = []
      this.volumeProjectIndex.forEach((pid, volId) => {
        if (pid === projectId) {
          entriesToRemove.push(volId)
        }
      })
      for (const volId of entriesToRemove) {
        this.volumeProjectIndex.delete(volId)
      }
    } else {
      this.volumeProjectIndex.clear()
    }
  }

  private async ensureChapterIndex(): Promise<void> {
    if (this.chapterIndex.size > 0) {
      return
    }
    const projectsResult = await this.listProjects()
    if (!projectsResult.success || !projectsResult.data) {
      return
    }
    for (const project of projectsResult.data) {
      const volumesResult = await this.listVolumes(project.id)
      if (!volumesResult.success || !volumesResult.data) {
        continue
      }
      for (const vol of volumesResult.data) {
        this.volumeProjectIndex.set(vol.id, project.id)
        const jsonStr: string = this.getNativeModule().nativeListChapters(project.id, vol.id)
        const chaptersResult = this.parseEnvelope<Chapter[]>(jsonStr)
        if (chaptersResult.success && chaptersResult.data) {
          for (const ch of chaptersResult.data) {
            this.chapterIndex.set(ch.id, { projectId: project.id, volumeId: vol.id })
          }
        }
      }
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

  async listWorkspaces(): Promise<ResultEnvelope<Workspace[]>> {
    return {
      success: true,
      data: [{
        path: this.workspacePath,
        name: '我的写作空间',
        isValid: true
      }]
    }
  }

  async openWorkspace(path: string): Promise<ResultEnvelope<WorkspaceState>> {
    if (!this.isInitialized) {
      const ok = await this.initialize(path)
      if (!ok) {
        return { success: false, errorCode: 'NATIVE_BRIDGE_NOT_LOADED', userMessage: '初始化失败' }
      }
    }

    const projectsResult = await this.listProjects()
    const recentResult = await this.getRecentEdits()
    const validResult = await this.validateWorkspace(path)

    return {
      success: true,
      data: {
        workspace: {
          path: this.workspacePath,
          name: '我的写作空间',
          isValid: validResult.success && validResult.data === true
        },
        projects: projectsResult.success ? (projectsResult.data || []) : [],
        recentEdits: recentResult.success ? (recentResult.data || []) : []
      }
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

  async getWorkspaceState(): Promise<ResultEnvelope<WorkspaceState>> {
    return this.openWorkspace(this.workspacePath)
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
      const result = this.parseEnvelope<ProjectTree>(jsonStr)
      if (result.success && result.data) {
        const tree = result.data as ProjectTree
        for (const vt of tree.volumes) {
          this.volumeProjectIndex.set(vt.volume.id, projectId)
          for (const ch of vt.chapters) {
            this.chapterIndex.set(ch.id, { projectId: projectId, volumeId: vt.volume.id })
          }
        }
      }
      return result
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
      const result = this.parseEnvelope<boolean>(jsonStr)
      if (result.success) {
        this.invalidateVolumeProjectIndex(projectId)
        this.invalidateChapterIndex()
      }
      return result
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
      const result = this.parseEnvelope<Volume[]>(jsonStr)
      if (result.success && result.data) {
        for (const vol of result.data) {
          this.volumeProjectIndex.set(vol.id, projectId)
        }
      }
      return result
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async createVolume(projectId: string, name: string): Promise<ResultEnvelope<Volume>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeCreateVolume(projectId, name)
      const result = this.parseEnvelope<Volume>(jsonStr)
      if (result.success && result.data) {
        // 新卷加入后，该项目下的卷列表已变化，清理旧索引并注册新卷
        this.invalidateVolumeProjectIndex(projectId)
        this.volumeProjectIndex.set(result.data.id, projectId)
      }
      return result
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
      const result = this.parseEnvelope<boolean>(jsonStr)
      if (result.success) {
        this.volumeProjectIndex.delete(volumeId)
        this.invalidateChapterIndex(volumeId)
      }
      return result
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async reorderVolumes(projectId: string, orderedVolumeIds: string[]): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeReorderVolumes(projectId, JSON.stringify(orderedVolumeIds))
      const result = this.parseEnvelope<boolean>(jsonStr)
      if (result.success) {
        this.invalidateVolumeProjectIndex(projectId)
        this.invalidateChapterIndex()
      }
      return result
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  // ── Chapter ──

  async listChapters(volumeId: string): Promise<ResultEnvelope<Chapter[]>> {
    const projectId = this.volumeProjectIndex.get(volumeId) || ''
    try {
      const jsonStr: string = this.getNativeModule().nativeListChapters(projectId, volumeId)
      const result = this.parseEnvelope<Chapter[]>(jsonStr)
      if (result.success && result.data) {
        for (const ch of result.data) {
          this.chapterIndex.set(ch.id, { projectId: projectId, volumeId: volumeId })
        }
      }
      return result
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async loadChapter(chapterId: string): Promise<ResultEnvelope<ChapterData>> {
    await this.ensureChapterIndex()
    let loc = this.resolveChapter(chapterId)
    if (!loc) {
      // 索引未命中，清理缓存并重试一次
      this.invalidateChapterIndex()
      await this.ensureChapterIndex()
      loc = this.resolveChapter(chapterId)
    }
    if (!loc) {
      return { success: false, errorCode: 'CHAPTER_NOT_FOUND', userMessage: `章节 ${chapterId} 未找到` }
    }
    try {
      const jsonStr: string = this.getNativeModule().nativeOpenChapter(loc.projectId, loc.volumeId, chapterId)
      return this.parseEnvelope<ChapterData>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async saveChapter(chapterId: string, text: string): Promise<ResultEnvelope<SaveReceipt>> {
    await this.ensureChapterIndex()
    let loc = this.resolveChapter(chapterId)
    if (!loc) {
      // 索引未命中，清理缓存并重试一次
      this.invalidateChapterIndex()
      await this.ensureChapterIndex()
      loc = this.resolveChapter(chapterId)
    }
    if (!loc) {
      return { success: false, errorCode: 'CHAPTER_NOT_FOUND', userMessage: `章节 ${chapterId} 未找到` }
    }
    try {
      const jsonStr: string = this.getNativeModule().nativeSaveChapter(loc.projectId, loc.volumeId, chapterId, text)
      return this.parseEnvelope<SaveReceipt>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async clearChapter(chapterId: string): Promise<ResultEnvelope<SaveReceipt>> {
    await this.ensureChapterIndex()
    let loc = this.resolveChapter(chapterId)
    if (!loc) {
      // 索引未命中，清理缓存并重试一次
      this.invalidateChapterIndex()
      await this.ensureChapterIndex()
      loc = this.resolveChapter(chapterId)
    }
    if (!loc) {
      return { success: false, errorCode: 'CHAPTER_NOT_FOUND', userMessage: `章节 ${chapterId} 未找到` }
    }
    try {
      const jsonStr: string = this.getNativeModule().nativeClearChapter(loc.projectId, loc.volumeId, chapterId)
      return this.parseEnvelope<SaveReceipt>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async createChapter(volumeId: string, name: string): Promise<ResultEnvelope<Chapter>> {
    const projectId = this.volumeProjectIndex.get(volumeId) || ''
    try {
      const jsonStr: string = this.getNativeModule().nativeCreateChapter(projectId, volumeId, name)
      const result = this.parseEnvelope<Chapter>(jsonStr)
      if (result.success && result.data) {
        // 新章节加入后，该 volume 下的章节列表已变化，清理旧索引并注册新章节
        this.invalidateChapterIndex(volumeId)
        this.chapterIndex.set(result.data.id, { projectId: projectId, volumeId: volumeId })
      }
      return result
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async renameChapter(chapterId: string, newName: string): Promise<ResultEnvelope<boolean>> {
    await this.ensureChapterIndex()
    let loc = this.resolveChapter(chapterId)
    if (!loc) {
      // 索引未命中，清理缓存并重试一次
      this.invalidateChapterIndex()
      await this.ensureChapterIndex()
      loc = this.resolveChapter(chapterId)
    }
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
    await this.ensureChapterIndex()
    let loc = this.resolveChapter(chapterId)
    if (!loc) {
      // 索引未命中，清理缓存并重试一次
      this.invalidateChapterIndex()
      await this.ensureChapterIndex()
      loc = this.resolveChapter(chapterId)
    }
    if (!loc) {
      return { success: false, errorCode: 'CHAPTER_NOT_FOUND', userMessage: `章节 ${chapterId} 未找到` }
    }
    try {
      const jsonStr: string = this.getNativeModule().nativeDeleteChapter(loc.projectId, loc.volumeId, chapterId)
      const result = this.parseEnvelope<boolean>(jsonStr)
      if (result.success) {
        this.chapterIndex.delete(chapterId)
      }
      return result
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async reorderChapters(volumeId: string, orderedChapterIds: string[]): Promise<ResultEnvelope<boolean>> {
    const projectId = this.volumeProjectIndex.get(volumeId) || ''
    try {
      const jsonStr: string = this.getNativeModule().nativeReorderChapters(projectId, volumeId, JSON.stringify(orderedChapterIds))
      const result = this.parseEnvelope<boolean>(jsonStr)
      if (result.success) {
        this.invalidateChapterIndex(volumeId)
      }
      return result
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

  async getEffectiveSettings(): Promise<ResultEnvelope<EffectiveSettings>> {
    const local = await this.getLocalSettings()
    if (local.success && local.data) {
      const data: EffectiveSettings = {
        fontSize: local.data.fontSize,
        lineHeight: local.data.lineHeight,
        fontFamily: local.data.fontFamily,
        theme: local.data.theme,
        autoSave: local.data.autoSave,
        autoSaveInterval: local.data.autoSaveInterval,
        autoIndent: local.data.autoIndent,
        showWordCount: local.data.showWordCount,
        showLineNumbers: local.data.showLineNumbers,
        wordWrap: local.data.wordWrap,
        spellCheck: local.data.spellCheck,
        lastWorkspacePath: local.data.lastWorkspacePath,
        lastProjectId: local.data.lastProjectId,
        lastChapterId: local.data.lastChapterId,
        editorWidth: local.data.editorWidth,
        uiLanguage: local.data.uiLanguage,
        isSynced: false
      }
      return { success: true, data: data }
    }
    return this.notImplemented()
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

  async syncDryRun(): Promise<ResultEnvelope<SyncReport>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeSyncDryRun()
      return this.parseEnvelope<SyncReport>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async syncDiagnostics(): Promise<ResultEnvelope<SyncDiagnostics>> {
    try {
      const jsonStr: string = this.getNativeModule().nativeSyncDiagnostics()
      return this.parseEnvelope<SyncDiagnostics>(jsonStr)
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

  async processWritingEvent(charsAdded: number, durationSeconds: number): Promise<ResultEnvelope<boolean>> {
    try {
      const eventJson = JSON.stringify({
        charsAdded: charsAdded,
        durationSeconds: durationSeconds,
        platform: 'harmony',
        sessionId: `hm-${Date.now()}`
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
}
