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
  ErrorCode
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
        const jsonStr: string = nativeModule.nativeListChapters(project.id, vol.id)
        const chaptersResult = this.parseEnvelope<Chapter[]>(jsonStr)
        if (chaptersResult.success && chaptersResult.data) {
          for (const ch of chaptersResult.data) {
            this.chapterIndex.set(ch.id, { projectId: project.id, volumeId: vol.id })
          }
        }
      }
    }
  }

  async initialize(workspacePath: string): Promise<boolean> {
    if (this.isInitialized) {
      return true
    }
    try {
      const result: number = nativeModule.nativeInit(workspacePath) as number
      if (result === 0) {
        this.isInitialized = true
        this.workspacePath = workspacePath
        return true
      }
      console.error(`nativeInit failed with code: ${result}`)
      return false
    } catch (e) {
      console.error(`nativeInit exception: ${e}`)
      return false
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
      const jsonStr: string = nativeModule.nativeValidateWorkspace()
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
      const jsonStr: string = nativeModule.nativeListProjects()
      return this.parseEnvelope<Project[]>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async getProjectTree(projectId: string): Promise<ResultEnvelope<ProjectTree>> {
    try {
      const jsonStr: string = nativeModule.nativeGetProjectTree(projectId)
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
      const jsonStr: string = nativeModule.nativeCreateProject(name)
      return this.parseEnvelope<Project>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async renameProject(projectId: string, newName: string): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = nativeModule.nativeRenameProject(projectId, newName)
      return this.parseEnvelope<boolean>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async deleteProject(projectId: string): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = nativeModule.nativeDeleteProject(projectId)
      return this.parseEnvelope<boolean>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async getProjectStats(projectId: string): Promise<ResultEnvelope<WritingStats>> {
    try {
      const jsonStr: string = nativeModule.nativeGetProjectStats(projectId)
      return this.parseEnvelope<WritingStats>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  // ── Volume ──

  async listVolumes(projectId: string): Promise<ResultEnvelope<Volume[]>> {
    try {
      const jsonStr: string = nativeModule.nativeListVolumes(projectId)
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
      const jsonStr: string = nativeModule.nativeCreateVolume(projectId, name)
      const result = this.parseEnvelope<Volume>(jsonStr)
      if (result.success && result.data) {
        this.volumeProjectIndex.set(result.data.id, projectId)
      }
      return result
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async renameVolume(projectId: string, volumeId: string, newName: string): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = nativeModule.nativeRenameVolume(projectId, volumeId, newName)
      return this.parseEnvelope<boolean>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async deleteVolume(projectId: string, volumeId: string): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = nativeModule.nativeDeleteVolume(projectId, volumeId)
      return this.parseEnvelope<boolean>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async reorderVolumes(projectId: string, orderedVolumeIds: string[]): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = nativeModule.nativeReorderVolumes(projectId, JSON.stringify(orderedVolumeIds))
      return this.parseEnvelope<boolean>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  // ── Chapter ──

  async listChapters(volumeId: string): Promise<ResultEnvelope<Chapter[]>> {
    const projectId = this.volumeProjectIndex.get(volumeId) || ''
    try {
      const jsonStr: string = nativeModule.nativeListChapters(projectId, volumeId)
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
    const loc = this.resolveChapter(chapterId)
    if (!loc) {
      return { success: false, errorCode: 'CHAPTER_NOT_FOUND', userMessage: `章节 ${chapterId} 未找到` }
    }
    try {
      const jsonStr: string = nativeModule.nativeOpenChapter(loc.projectId, loc.volumeId, chapterId)
      return this.parseEnvelope<ChapterData>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async saveChapter(chapterId: string, text: string): Promise<ResultEnvelope<SaveReceipt>> {
    await this.ensureChapterIndex()
    const loc = this.resolveChapter(chapterId)
    if (!loc) {
      return { success: false, errorCode: 'CHAPTER_NOT_FOUND', userMessage: `章节 ${chapterId} 未找到` }
    }
    try {
      const jsonStr: string = nativeModule.nativeSaveChapter(loc.projectId, loc.volumeId, chapterId, text)
      return this.parseEnvelope<SaveReceipt>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async clearChapter(chapterId: string): Promise<ResultEnvelope<SaveReceipt>> {
    await this.ensureChapterIndex()
    const loc = this.resolveChapter(chapterId)
    if (!loc) {
      return { success: false, errorCode: 'CHAPTER_NOT_FOUND', userMessage: `章节 ${chapterId} 未找到` }
    }
    try {
      const jsonStr: string = nativeModule.nativeClearChapter(loc.projectId, loc.volumeId, chapterId)
      return this.parseEnvelope<SaveReceipt>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async createChapter(volumeId: string, name: string): Promise<ResultEnvelope<Chapter>> {
    const projectId = this.volumeProjectIndex.get(volumeId) || ''
    try {
      const jsonStr: string = nativeModule.nativeCreateChapter(projectId, volumeId, name)
      const result = this.parseEnvelope<Chapter>(jsonStr)
      if (result.success && result.data) {
        this.chapterIndex.set(result.data.id, { projectId: projectId, volumeId: volumeId })
      }
      return result
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async renameChapter(chapterId: string, newName: string): Promise<ResultEnvelope<boolean>> {
    await this.ensureChapterIndex()
    const loc = this.resolveChapter(chapterId)
    if (!loc) {
      return { success: false, errorCode: 'CHAPTER_NOT_FOUND', userMessage: `章节 ${chapterId} 未找到` }
    }
    try {
      const jsonStr: string = nativeModule.nativeRenameChapter(loc.projectId, loc.volumeId, chapterId, newName)
      return this.parseEnvelope<boolean>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async deleteChapter(chapterId: string): Promise<ResultEnvelope<boolean>> {
    await this.ensureChapterIndex()
    const loc = this.resolveChapter(chapterId)
    if (!loc) {
      return { success: false, errorCode: 'CHAPTER_NOT_FOUND', userMessage: `章节 ${chapterId} 未找到` }
    }
    try {
      const jsonStr: string = nativeModule.nativeDeleteChapter(loc.projectId, loc.volumeId, chapterId)
      return this.parseEnvelope<boolean>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async reorderChapters(volumeId: string, orderedChapterIds: string[]): Promise<ResultEnvelope<boolean>> {
    const projectId = this.volumeProjectIndex.get(volumeId) || ''
    try {
      const jsonStr: string = nativeModule.nativeReorderChapters(projectId, volumeId, JSON.stringify(orderedChapterIds))
      return this.parseEnvelope<boolean>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  // ── StarMap ──

  async listStarMaps(): Promise<ResultEnvelope<StarMapMeta[]>> {
    try {
      const jsonStr: string = nativeModule.nativeListStarMaps()
      return this.parseEnvelope<StarMapMeta[]>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async listStarMapsForProject(projectId: string): Promise<ResultEnvelope<StarMapMeta[]>> {
    try {
      const jsonStr: string = nativeModule.nativeListStarMapsForProject(projectId)
      return this.parseEnvelope<StarMapMeta[]>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async getStarMap(starmapId: string): Promise<ResultEnvelope<StarMapMeta>> {
    try {
      const jsonStr: string = nativeModule.nativeGetStarMap(starmapId)
      return this.parseEnvelope<StarMapMeta>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async getStarMapGraph(starmapId: string): Promise<ResultEnvelope<StarMapGraph>> {
    try {
      const jsonStr: string = nativeModule.nativeGetStarMapGraph(starmapId)
      return this.parseEnvelope<StarMapGraph>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async createStarMap(title: string, description: string): Promise<ResultEnvelope<StarMapMeta>> {
    try {
      const jsonStr: string = nativeModule.nativeCreateStarMap(title, description)
      return this.parseEnvelope<StarMapMeta>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async deleteStarMap(starmapId: string): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = nativeModule.nativeDeleteStarMap(starmapId)
      return this.parseEnvelope<boolean>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async renameStarMap(starmapId: string, newTitle: string): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = nativeModule.nativeRenameStarMap(starmapId, newTitle)
      return this.parseEnvelope<boolean>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  // ── Settings ──

  async getLocalSettings(): Promise<ResultEnvelope<LocalSettings>> {
    try {
      const jsonStr: string = nativeModule.nativeLoadLocalSettings()
      return this.parseEnvelope<LocalSettings>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async saveLocalSettings(settings: LocalSettings): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = nativeModule.nativeSaveLocalSettings(JSON.stringify(settings))
      return this.parseEnvelope<boolean>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async getSyncableSettings(): Promise<ResultEnvelope<SyncableSettings>> {
    try {
      const jsonStr: string = nativeModule.nativeLoadSyncableSettings()
      return this.parseEnvelope<SyncableSettings>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async saveSyncableSettings(settings: SyncableSettings): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = nativeModule.nativeSaveSyncableSettings(JSON.stringify(settings))
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
      const jsonStr: string = nativeModule.nativeLoadSyncConfig()
      return this.parseEnvelope<SyncConfig>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async saveSyncConfig(config: SyncConfig): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = nativeModule.nativeSaveSyncConfig(JSON.stringify(config))
      return this.parseEnvelope<boolean>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async syncDryRun(): Promise<ResultEnvelope<SyncReport>> {
    try {
      const jsonStr: string = nativeModule.nativeSyncDryRun()
      return this.parseEnvelope<SyncReport>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async syncDiagnostics(): Promise<ResultEnvelope<SyncDiagnostics>> {
    try {
      const jsonStr: string = nativeModule.nativeSyncDiagnostics()
      return this.parseEnvelope<SyncDiagnostics>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async performSync(): Promise<ResultEnvelope<SyncResult>> {
    try {
      const jsonStr: string = nativeModule.nativePerformSync()
      return this.parseEnvelope<SyncResult>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  // ── Stats ──

  async getWritingStats(): Promise<ResultEnvelope<WritingStats>> {
    try {
      const jsonStr: string = nativeModule.nativeGetWritingStats()
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
      return nativeModule.nativeCalculateWordCount(text)
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
      const jsonStr: string = nativeModule.nativeProcessWritingEvent(eventJson)
      return this.parseEnvelope<boolean>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async getRecentEdits(): Promise<ResultEnvelope<RecentEdit[]>> {
    try {
      const jsonStr: string = nativeModule.nativeGetRecentEdits()
      return this.parseEnvelope<RecentEdit[]>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  // ── Native Status ──

  async getNativeLoadStatus(): Promise<ResultEnvelope<string>> {
    try {
      const status: string = nativeModule.nativeGetLoadStatus() as string
      return { success: true, data: status }
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async isWorkspaceValid(): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = nativeModule.nativeValidateWorkspace() as string
      return this.parseEnvelope<boolean>(jsonStr)
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }

  async isAiAvailable(): Promise<ResultEnvelope<boolean>> {
    try {
      const available: number = nativeModule.nativeIsAiAvailable() as number
      return { success: true, data: available === 1 }
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e) }
    }
  }
}
