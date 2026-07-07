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
  StarMapNode,
  StarMapEdge,
  StarMapEmbed,
  StarMapLink,
  StarMapViewport,
  StarMapLayout,
  StarMapLayoutNode,
  StarMapEdgeRender,
  StarMapMotionPolicy,
  LocalSettings,
  SyncableSettings,
  ResultEnvelope,
  ProjectTree,
  VolumeTree,
  ProjectStats,
  WritingStats,
  SyncConfig,
  SyncResult,
  SyncDiagnosticsResult,
  RecentEdit,
  SyncState,
  SyncConflict,
  ChangedEntity,
  ErrorCode,
  WindowMetrics,
  LayoutPlan,
  ScreenPolicy,
  ActionSlot,
  ScreenRole,
  ActionRole,
  ActionPlacement,
  ShellMode
} from '../model/CoreDtos'
import { resolveLayoutFromMetrics } from '../common/LayoutPolicyHelper'

export class MockWriterCoreBridge implements IWriterCoreBridge {
  private currentWorkspacePath: string = '/mock/workspace'
  private mockProjects: Project[] = []

  private mockVolumes: Map<string, Volume[]> = new Map<string, Volume[]>()

  private mockChapters: Map<string, ChapterMeta[]> = new Map<string, ChapterMeta[]>()

  private mockChapterContents: Map<string, string> = new Map<string, string>()

  private mockStarMaps: StarMapMeta[] = []

  private mockStarMapViewport: StarMapViewport = {
    scale: 1,
    offsetX: 0,
    offsetY: 0,
    width: 800,
    height: 600
  } as StarMapViewport

  private mockStarMapLayout: StarMapLayout = {
    kind: 'Freeform',
    nodes: [] as StarMapLayoutNode[]
  } as StarMapLayout

  private mockStarMapGraph: StarMapGraph = {
    schemaVersion: 1,
    id: '',
    starmapId: '',
    title: '',
    nodes: [] as StarMapNode[],
    edges: [] as StarMapEdge[],
    embeds: [] as StarMapEmbed[],
    links: [] as StarMapLink[],
    layout: this.mockStarMapLayout,
    createdAt: 0,
    updatedAt: 0
  }

  private mockSettings: LocalSettings = {
    themeMode: 'system',
    locale: 'zh-CN',
    autoSaveEnabled: true,
    editorFontSize: 16,
    editorLineSpacingMultiplier: 1.8,
    windowWidth: 720,
    windowHeight: 1280,
    autoSaveDelayMs: 3000,
    autoIndentEnabled: true,
    autoIndentWidth: 2,
    editorTypingAnimationEnabled: true,
    editorSmoothCursorEnabled: true,
    editorTypingAnimationDurationMs: 100,
    editorSmoothCursorDurationMs: 80,
    aiEnabled: false,
    statsDeviceId: undefined,
    desktopSidebarWidth: 280,
    desktopEditorWidth: 820
  } as LocalSettings

  private mockSyncConfig: SyncConfig = {
    enabled: false,
    backendType: 'github',
    remoteUrl: '',
    transport: 'https',
    branch: 'main',
    autoSync: false,
    syncIntervalSeconds: 300,
    username: '',
    androidHasInternetPermission: false,
    androidHasAccessNetworkStatePermission: false
  } as SyncConfig

  private delay(ms: number): Promise<void> {
    return new Promise<void>(resolve => setTimeout(resolve, ms))
  }

  private success<T>(data: T): ResultEnvelope<T> {
    const result: ResultEnvelope<T> = {
      success: true,
      data,
      warnings: [] as string[],
      changedPaths: [] as string[],
      changedEntities: [] as ChangedEntity[]
    }
    return result
  }

  private error<T>(errorCode: ErrorCode, userMessage: string): ResultEnvelope<T> {
    const result: ResultEnvelope<T> = {
      success: false,
      errorCode: errorCode,
      messageKey: 'error.other',
      rawError: userMessage,
      warnings: [] as string[],
      changedPaths: [] as string[],
      changedEntities: [] as ChangedEntity[]
    }
    return result
  }

  // Workspace methods
  async listWorkspaces(): Promise<ResultEnvelope<WorkspaceSummary[]>> {
    await this.delay(100)
    return this.success([
      {
        path: '/mock/workspace',
        isValid: true,
        projects: this.mockProjects,
        recentEdits: [] as RecentEdit[]
      } as WorkspaceSummary
    ] as WorkspaceSummary[])
  }

  async openWorkspace(path: string): Promise<ResultEnvelope<WorkspaceSummary>> {
    await this.delay(200)
    this.currentWorkspacePath = path
    const summary: WorkspaceSummary = {
      path,
      isValid: true,
      projects: this.mockProjects,
      recentEdits: [] as RecentEdit[]
    }
    return this.success(summary)
  }

  async validateWorkspace(path: string): Promise<ResultEnvelope<boolean>> {
    await this.delay(100)
    return this.success(true)
  }

  async getWorkspaceState(): Promise<ResultEnvelope<WorkspaceSummary>> {
    return this.openWorkspace(this.currentWorkspacePath)
  }

  // Project methods
  async listProjects(): Promise<ResultEnvelope<Project[]>> {
    await this.delay(100)
    return this.success(this.mockProjects)
  }

  async getProjectTree(projectId: string): Promise<ResultEnvelope<ProjectTree>> {
    await this.delay(150)
    const project = this.mockProjects.find(p => p.id === projectId)
    if (!project) {
      return this.error('PROJECT_NOT_FOUND', '项目不存在')
    }

    const volumes = this.mockVolumes.get(projectId) || [] as Volume[]
    const volumeTrees: VolumeTree[] = volumes.map((vol: Volume): VolumeTree => {
      const vt: VolumeTree = {
        volume: vol,
        chapters: this.mockChapters.get(vol.id) || [] as ChapterMeta[]
      }
      return vt
    })

    const stats: ProjectStats = {
      totalWordCount: 0,
      volumeCount: volumes.length,
      chapterCount: volumeTrees.reduce((sum: number, vt: VolumeTree) => sum + vt.chapters.length, 0)
    }
    const tree: ProjectTree = {
      project,
      volumes: volumeTrees,
      stats
    }
    return this.success(tree)
  }

  async createProject(name: string): Promise<ResultEnvelope<Project>> {
    await this.delay(200)
    const newProject: Project = {
      id: `proj-${Date.now()}`,
      title: name,
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString()
    }
    this.mockProjects.push(newProject)
    this.mockVolumes.set(newProject.id, [] as Volume[])
    return this.success(newProject)
  }

  async renameProject(projectId: string, newName: string): Promise<ResultEnvelope<boolean>> {
    await this.delay(150)
    const project = this.mockProjects.find(p => p.id === projectId)
    if (!project) {
      return this.error('PROJECT_NOT_FOUND', '项目不存在')
    }
    project.title = newName
    project.updatedAt = new Date().toISOString()
    return this.success(true)
  }

  async deleteProject(projectId: string): Promise<ResultEnvelope<boolean>> {
    await this.delay(150)
    const index = this.mockProjects.findIndex(p => p.id === projectId)
    if (index === -1) {
      return this.error('PROJECT_NOT_FOUND', '项目不存在')
    }
    this.mockProjects.splice(index, 1)
    this.mockVolumes.delete(projectId)
    return this.success(true)
  }

  async getProjectStats(projectId: string): Promise<ResultEnvelope<WritingStats>> {
    await this.delay(100)
    const stats: WritingStats = {
      totalChars: 25600,
      totalWords: 18200,
      totalChapters: 8,
      todayChars: 1200,
      todayDuration: 45,
      averageSpeed: 800
    }
    return this.success(stats)
  }

  // Volume methods
  async listVolumes(projectId: string): Promise<ResultEnvelope<Volume[]>> {
    await this.delay(100)
    return this.success(this.mockVolumes.get(projectId) || [] as Volume[])
  }

  async createVolume(projectId: string, name: string): Promise<ResultEnvelope<Volume>> {
    await this.delay(150)
    const volumes = this.mockVolumes.get(projectId) || [] as Volume[]
    const newVolume: Volume = {
      id: `vol-${Date.now()}`,
      title: name,
      order: volumes.length + 1,
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString()
    }
    volumes.push(newVolume)
    this.mockVolumes.set(projectId, volumes)
    return this.success(newVolume)
  }

  async renameVolume(projectId: string, volumeId: string, newName: string): Promise<ResultEnvelope<boolean>> {
    await this.delay(150)
    const volumes = this.mockVolumes.get(projectId) || [] as Volume[]
    const volume = volumes.find(v => v.id === volumeId)
    if (!volume) {
      return this.error('VOLUME_NOT_FOUND', '卷不存在')
    }
    volume.title = newName
    volume.updatedAt = new Date().toISOString()
    return this.success(true)
  }

  async deleteVolume(projectId: string, volumeId: string): Promise<ResultEnvelope<boolean>> {
    await this.delay(150)
    const volumes = this.mockVolumes.get(projectId) || [] as Volume[]
    const index = volumes.findIndex(v => v.id === volumeId)
    if (index === -1) {
      return this.error('VOLUME_NOT_FOUND', '卷不存在')
    }
    volumes.splice(index, 1)
    this.mockChapters.delete(volumeId)
    return this.success(true)
  }

  async reorderVolumes(projectId: string, orderedVolumeIds: string[]): Promise<ResultEnvelope<boolean>> {
    await this.delay(150)
    const volumes = this.mockVolumes.get(projectId) || [] as Volume[]
    orderedVolumeIds.forEach((id, index) => {
      const volume = volumes.find(v => v.id === id)
      if (volume) {
        volume.order = index + 1
      }
    })
    return this.success(true)
  }

  // Chapter methods
  async listChapters(volumeId: string): Promise<ResultEnvelope<ChapterMeta[]>> {
    await this.delay(100)
    return this.success(this.mockChapters.get(volumeId) || [] as ChapterMeta[])
  }

  async loadChapter(chapterId: string): Promise<ResultEnvelope<ChapterContent>> {
    await this.delay(200)
    const content = this.mockChapterContents.get(chapterId)
    if (content !== undefined) {
      const meta: ChapterMeta = {
        id: chapterId,
        title: '第一章 初遇',
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
        order: 1,
        wordCount: content.replace(/\s/g, '').length,
        hash: 'mock-hash-' + chapterId,
        note: undefined
      }
      const data: ChapterContent = {
        meta,
        content
      }
      return this.success(data)
    }

    const volKeys: string[] = Array.from(this.mockChapters.keys())
    for (let i = 0; i < volKeys.length; i++) {
      const volId: string = volKeys[i]
      const chapters: ChapterMeta[] = this.mockChapters.get(volId) || [] as ChapterMeta[]
      const chapter = chapters.find(ch => ch.id === chapterId)
      if (chapter) {
        const sampleContent = `这是《${chapter.title}》的示例内容。\n\n在此处开始写作...\n\n鸿蒙端当前使用 mock 数据，后续将对接真实 Rust Core。`
        const data: ChapterContent = {
          meta: chapter,
          content: sampleContent
        }
        return this.success(data)
      }
    }

    return this.error('CHAPTER_NOT_FOUND', '章节不存在')
  }

  async saveChapter(chapterId: string, text: string): Promise<ResultEnvelope<ChapterSaveReceipt>> {
    await this.delay(300)
    if (!text || text.trim().length === 0) {
      const result: ResultEnvelope<ChapterSaveReceipt> = {
        success: false,
        errorCode: 'EMPTY_OVERWRITE_BLOCKED',
        messageKey: 'error.empty_overwrite_blocked',
        rawError: 'Cannot save empty content',
        warnings: [] as string[],
        changedPaths: [] as string[],
        changedEntities: [] as ChangedEntity[]
      }
      return result
    }

    this.mockChapterContents.set(chapterId, text)
    const receipt: ChapterSaveReceipt = {
      chapterRelativePath: `chapters/${chapterId}.md`,
      contentLen: text.length,
      contentHash: 'mock-hash-content-' + Date.now(),
      metaHash: 'mock-hash-meta-' + Date.now(),
      updatedAt: new Date().toISOString(),
      wordCount: text.replace(/\s/g, '').length
    }
    return this.success(receipt)
  }

  async clearChapter(chapterId: string): Promise<ResultEnvelope<ChapterSaveReceipt>> {
    await this.delay(200)
    this.mockChapterContents.set(chapterId, '')
    const receipt: ChapterSaveReceipt = {
      chapterRelativePath: `chapters/${chapterId}.md`,
      contentLen: 0,
      contentHash: 'mock-hash-empty',
      metaHash: 'mock-hash-meta-' + Date.now(),
      updatedAt: new Date().toISOString(),
      wordCount: 0
    }
    return this.success(receipt)
  }

  async createChapter(volumeId: string, name: string): Promise<ResultEnvelope<ChapterMeta>> {
    await this.delay(150)
    const chapters = this.mockChapters.get(volumeId) || [] as ChapterMeta[]
    const newChapter: ChapterMeta = {
      id: `ch-${Date.now()}`,
      title: name,
      wordCount: 0,
      order: chapters.length + 1,
      updatedAt: new Date().toISOString(),
      createdAt: new Date().toISOString(),
      hash: 'mock-hash-new',
      note: undefined
    }
    chapters.push(newChapter)
    this.mockChapters.set(volumeId, chapters)
    return this.success(newChapter)
  }

  async renameChapter(chapterId: string, newName: string): Promise<ResultEnvelope<boolean>> {
    await this.delay(150)
    const volKeys: string[] = Array.from(this.mockChapters.keys())
    for (let i = 0; i < volKeys.length; i++) {
      const volId: string = volKeys[i]
      const chapters: ChapterMeta[] = this.mockChapters.get(volId) || [] as ChapterMeta[]
      const chapter = chapters.find(ch => ch.id === chapterId)
      if (chapter) {
        chapter.title = newName
        chapter.updatedAt = new Date().toISOString()
        return this.success(true)
      }
    }
    return this.error('CHAPTER_NOT_FOUND', '章节不存在')
  }

  async deleteChapter(chapterId: string): Promise<ResultEnvelope<boolean>> {
    await this.delay(150)
    const volKeys: string[] = Array.from(this.mockChapters.keys())
    for (let i = 0; i < volKeys.length; i++) {
      const volId: string = volKeys[i]
      const chapters: ChapterMeta[] = this.mockChapters.get(volId) || [] as ChapterMeta[]
      const index = chapters.findIndex(ch => ch.id === chapterId)
      if (index !== -1) {
        chapters.splice(index, 1)
        this.mockChapterContents.delete(chapterId)
        return this.success(true)
      }
    }
    return this.error('CHAPTER_NOT_FOUND', '章节不存在')
  }

  async reorderChapters(volumeId: string, orderedChapterIds: string[]): Promise<ResultEnvelope<boolean>> {
    await this.delay(150)
    const chapters = this.mockChapters.get(volumeId) || [] as ChapterMeta[]
    orderedChapterIds.forEach((id, index) => {
      const chapter = chapters.find(ch => ch.id === id)
      if (chapter) {
        chapter.order = index + 1
      }
    })
    return this.success(true)
  }

  // StarMap methods
  async listStarMaps(): Promise<ResultEnvelope<StarMapMeta[]>> {
    await this.delay(100)
    return this.success(this.mockStarMaps)
  }

  async listStarMapsForProject(projectId: string): Promise<ResultEnvelope<StarMapMeta[]>> {
    await this.delay(100)
    // StarMapMeta 当前无 projectId 字段，mock 暂返回全部
    return this.success(this.mockStarMaps)
  }

  async getStarMap(starmapId: string): Promise<ResultEnvelope<StarMapMeta>> {
    await this.delay(100)
    const starmap = this.mockStarMaps.find(sm => sm.id === starmapId)
    if (!starmap) {
      return this.error('STARMAP_NOT_FOUND', '星图不存在')
    }
    return this.success(starmap)
  }

  async getStarMapGraph(starmapId: string): Promise<ResultEnvelope<StarMapGraph>> {
    await this.delay(200)
    if (this.mockStarMapGraph.id === starmapId) {
      return this.success(this.mockStarMapGraph)
    }
    return this.error('STARMAP_NOT_FOUND', '星图不存在')
  }

  async createStarMap(title: string, description: string): Promise<ResultEnvelope<StarMapMeta>> {
    await this.delay(200)
    const newStarMap: StarMapMeta = {
      id: `starmap-${Date.now()}`,
      title,
      description,
      nodeCount: 0,
      edgeCount: 0,
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString()
    }
    this.mockStarMaps.push(newStarMap)
    return this.success(newStarMap)
  }

  async deleteStarMap(starmapId: string): Promise<ResultEnvelope<boolean>> {
    await this.delay(150)
    const index = this.mockStarMaps.findIndex(sm => sm.id === starmapId)
    if (index === -1) {
      return this.error('STARMAP_NOT_FOUND', '星图不存在')
    }
    this.mockStarMaps.splice(index, 1)
    return this.success(true)
  }

  async renameStarMap(starmapId: string, newTitle: string): Promise<ResultEnvelope<boolean>> {
    await this.delay(150)
    const starmap = this.mockStarMaps.find(sm => sm.id === starmapId)
    if (!starmap) {
      return this.error('STARMAP_NOT_FOUND', '星图不存在')
    }
    starmap.title = newTitle
    starmap.updatedAt = new Date().toISOString()
    return this.success(true)
  }

  async saveStarMapLayout(starmapId: string, layout: StarMapLayout): Promise<ResultEnvelope<boolean>> {
    await this.delay(150)
    const starmap = this.mockStarMaps.find(sm => sm.id === starmapId)
    if (!starmap) {
      return this.error('STARMAP_NOT_FOUND', '星图不存在')
    }
    this.mockStarMapLayout = layout
    return this.success(true)
  }

  async getStarMapLayout(starmapId: string): Promise<ResultEnvelope<StarMapLayout>> {
    await this.delay(100)
    const starmap = this.mockStarMaps.find(sm => sm.id === starmapId)
    if (!starmap) {
      return this.error('STARMAP_NOT_FOUND', '星图不存在')
    }
    return this.success(this.mockStarMapLayout)
  }

  async getStarMapMotionPolicy(): Promise<ResultEnvelope<StarMapMotionPolicy>> {
    await this.delay(100)
    const policy: StarMapMotionPolicy = {
      enabled: true,
      idleWobbleEnabled: true,
      idleAmplitudeVp: 2,
      idlePeriodMs: 3000,
      dragLiftScale: 1.05,
      dragShadowBoost: 1.2,
      settleDurationMs: 300,
      reduceMotion: false
    }
    return this.success(policy)
  }

  async computeStarMapEdgeRenders(graph: StarMapGraph): Promise<ResultEnvelope<StarMapEdgeRender[]>> {
    await this.delay(100)
    return this.success([] as StarMapEdgeRender[])
  }

  // Settings methods
  async getLocalSettings(): Promise<ResultEnvelope<LocalSettings>> {
    await this.delay(100)
    const copy: LocalSettings = {
      themeMode: this.mockSettings.themeMode,
      locale: this.mockSettings.locale,
      autoSaveEnabled: this.mockSettings.autoSaveEnabled,
      editorFontSize: this.mockSettings.editorFontSize,
      editorLineSpacingMultiplier: this.mockSettings.editorLineSpacingMultiplier,
      windowWidth: this.mockSettings.windowWidth,
      windowHeight: this.mockSettings.windowHeight,
      autoSaveDelayMs: this.mockSettings.autoSaveDelayMs,
      autoIndentEnabled: this.mockSettings.autoIndentEnabled,
      autoIndentWidth: this.mockSettings.autoIndentWidth,
      editorTypingAnimationEnabled: this.mockSettings.editorTypingAnimationEnabled,
      editorSmoothCursorEnabled: this.mockSettings.editorSmoothCursorEnabled,
      editorTypingAnimationDurationMs: this.mockSettings.editorTypingAnimationDurationMs,
      editorSmoothCursorDurationMs: this.mockSettings.editorSmoothCursorDurationMs,
      aiEnabled: this.mockSettings.aiEnabled,
      statsDeviceId: this.mockSettings.statsDeviceId,
      desktopSidebarWidth: this.mockSettings.desktopSidebarWidth,
      desktopEditorWidth: this.mockSettings.desktopEditorWidth
    }
    return this.success(copy)
  }

  async saveLocalSettings(settings: LocalSettings): Promise<ResultEnvelope<boolean>> {
    await this.delay(200)
    this.mockSettings = {
      themeMode: settings.themeMode,
      locale: settings.locale,
      autoSaveEnabled: settings.autoSaveEnabled,
      editorFontSize: settings.editorFontSize,
      editorLineSpacingMultiplier: settings.editorLineSpacingMultiplier,
      windowWidth: settings.windowWidth,
      windowHeight: settings.windowHeight,
      autoSaveDelayMs: settings.autoSaveDelayMs,
      autoIndentEnabled: settings.autoIndentEnabled,
      autoIndentWidth: settings.autoIndentWidth,
      editorTypingAnimationEnabled: settings.editorTypingAnimationEnabled,
      editorSmoothCursorEnabled: settings.editorSmoothCursorEnabled,
      editorTypingAnimationDurationMs: settings.editorTypingAnimationDurationMs,
      editorSmoothCursorDurationMs: settings.editorSmoothCursorDurationMs,
      aiEnabled: settings.aiEnabled,
      statsDeviceId: settings.statsDeviceId,
      desktopSidebarWidth: settings.desktopSidebarWidth,
      desktopEditorWidth: settings.desktopEditorWidth
    }
    return this.success(true)
  }

  async getSyncableSettings(): Promise<ResultEnvelope<SyncableSettings>> {
    await this.delay(100)
    const result: SyncableSettings = {
      fontSize: this.mockSettings.editorFontSize,
      themeMode: this.mockSettings.themeMode || 'system',
      monetColor: 'default',
      themePaletteJson: ''
    }
    return this.success(result)
  }

  async saveSyncableSettings(settings: SyncableSettings): Promise<ResultEnvelope<boolean>> {
    await this.delay(200)
    if (settings.fontSize !== undefined) this.mockSettings.editorFontSize = settings.fontSize
    if (settings.themeMode !== undefined) this.mockSettings.themeMode = settings.themeMode
    return this.success(true)
  }

  // Sync methods
  async loadSyncConfig(): Promise<ResultEnvelope<SyncConfig>> {
    await this.delay(100)
    const copy: SyncConfig = {
      enabled: this.mockSyncConfig.enabled,
      backendType: this.mockSyncConfig.backendType,
      remoteUrl: this.mockSyncConfig.remoteUrl,
      transport: this.mockSyncConfig.transport,
      branch: this.mockSyncConfig.branch,
      autoSync: this.mockSyncConfig.autoSync,
      syncIntervalSeconds: this.mockSyncConfig.syncIntervalSeconds,
      username: this.mockSyncConfig.username,
      androidHasInternetPermission: this.mockSyncConfig.androidHasInternetPermission,
      androidHasAccessNetworkStatePermission: this.mockSyncConfig.androidHasAccessNetworkStatePermission
    }
    return this.success(copy)
  }

  async saveSyncConfig(config: SyncConfig): Promise<ResultEnvelope<boolean>> {
    await this.delay(200)
    this.mockSyncConfig = {
      enabled: config.enabled,
      backendType: config.backendType,
      remoteUrl: config.remoteUrl,
      transport: config.transport,
      branch: config.branch,
      autoSync: config.autoSync,
      syncIntervalSeconds: config.syncIntervalSeconds,
      username: config.username,
      androidHasInternetPermission: config.androidHasInternetPermission,
      androidHasAccessNetworkStatePermission: config.androidHasAccessNetworkStatePermission
    }
    return this.success(true)
  }

  async syncDiagnostics(): Promise<ResultEnvelope<SyncDiagnosticsResult>> {
    await this.delay(500)
    const result: SyncDiagnosticsResult = {
      success: true,
      backendType: this.mockSyncConfig.backendType,
      androidHasInternetPermission: true,
      androidHasAccessNetworkStatePermission: true,
      androidNetworkState: 'connected',
      tcpProbeOk: true,
      tcpProbeStatus: 'ok',
      httpConnectProbeOk: true,
      httpConnectProbeStatus: 'ok',
      libgit2ProbeOk: true,
      libgit2ProbeStatus: 'ok',
      networkOk: true,
      authOk: true,
      repoOk: true,
      branchOk: true,
      networkStatus: 'connected',
      authStatus: 'authenticated',
      repoStatus: 'valid',
      branchStatus: 'synced',
      remoteUrlSanitized: 'https://github.com/***',
      transport: 'https',
      errorCategory: '',
      userMessage: undefined,
      rawError: undefined,
      chosenNetworkMode: undefined,
      networkProbeSummary: undefined
    }
    return this.success(result)
  }

  async syncDryRun(): Promise<ResultEnvelope<SyncResult>> {
    await this.delay(300)
    const result: SyncResult = {
      status: 'dry-run',
      uploadedFiles: [] as string[],
      downloadedFiles: [] as string[],
      localDeletes: [] as string[],
      remoteDeletes: [] as string[],
      overwrittenFiles: [] as string[],
      ignoredFiles: [] as string[],
      conflicts: [] as SyncConflict[],
      firstSyncMode: 'none'
    }
    return this.success(result)
  }

  async performSync(): Promise<ResultEnvelope<SyncResult>> {
    await this.delay(1000)
    const result: SyncResult = {
      status: 'success',
      uploadedFiles: ['projects/proj-001/volumes/vol-001/chapters/ch-001.md'] as string[],
      downloadedFiles: [] as string[],
      localDeletes: [] as string[],
      remoteDeletes: [] as string[],
      overwrittenFiles: [] as string[],
      ignoredFiles: [] as string[],
      conflicts: [] as SyncConflict[],
      commitHash: 'abc123',
      firstSyncMode: 'none'
    }
    return this.success(result)
  }

  // Stats methods
  async getWritingStats(): Promise<ResultEnvelope<WritingStats>> {
    await this.delay(150)
    const stats: WritingStats = {
      totalChars: 25600,
      totalWords: 18200,
      totalChapters: 8,
      todayChars: 1200,
      todayDuration: 45,
      averageSpeed: 800
    }
    return this.success(stats)
  }

  async calculateWordCount(text: string): Promise<number> {
    return text.replace(/\s/g, '').length
  }

  async processWritingEvent(deviceId: string, platform: string, projectId: string, volumeId: string,
    chapterId: string, oldText: string, newText: string, durationSeconds: number, sessionId: string): Promise<ResultEnvelope<boolean>> {
    await this.delay(50)
    return this.success(true)
  }

  async getRecentEdits(): Promise<ResultEnvelope<RecentEdit[]>> {
    await this.delay(100)
    return this.success([] as RecentEdit[])
  }

  // Layout Policy methods — 委托给 LayoutPolicyHelper 纯函数
  resolveLayout(metrics: WindowMetrics): Promise<ResultEnvelope<LayoutPlan>> {
    const plan = resolveLayoutFromMetrics(metrics)
    return Promise.resolve({
      success: true,
      data: plan,
      warnings: [] as string[],
      changedPaths: [] as string[],
      changedEntities: [] as ChangedEntity[]
    } as ResultEnvelope<LayoutPlan>)
  }

  // Screen Policy methods — mock 返回与 Core 一致的策略
  resolveScreenPolicy(screenRole: ScreenRole, shellMode: ShellMode): Promise<ResultEnvelope<ScreenPolicy>> {
    const policy = this.mockResolveScreenPolicy(screenRole, shellMode)
    return Promise.resolve(this.success(policy))
  }

  private mockResolveScreenPolicy(screenRole: ScreenRole, shellMode: ShellMode): ScreenPolicy {
    const allSlots = this.mockBaseSlots(screenRole)
    const filtered = allSlots.filter((slot: ActionSlot) => slot.visibleIn.includes(shellMode))
    return {
      screenRole: screenRole,
      actionSlots: filtered
    }
  }

  private mockBaseSlots(screenRole: ScreenRole): ActionSlot[] {
    switch (screenRole) {
      case ScreenRole.Home:
        return [
          { actionId: 'settings', role: ActionRole.Settings, placement: ActionPlacement.TopTrailing, visibleIn: [ShellMode.SinglePane, ShellMode.SupportingPane, ShellMode.TwoPane], requiresConfirmation: false },
          { actionId: 'search', role: ActionRole.Search, placement: ActionPlacement.TopTrailing, visibleIn: [ShellMode.SinglePane, ShellMode.SupportingPane, ShellMode.TwoPane], requiresConfirmation: false }
        ]
      case ScreenRole.Workspace:
        return [
          { actionId: 'create_project', role: ActionRole.CreateProject, placement: ActionPlacement.Floating, visibleIn: [ShellMode.SinglePane], requiresConfirmation: false },
          { actionId: 'create_project', role: ActionRole.CreateProject, placement: ActionPlacement.TopTrailing, visibleIn: [ShellMode.SupportingPane, ShellMode.TwoPane], requiresConfirmation: false },
          { actionId: 'delete', role: ActionRole.Delete, placement: ActionPlacement.ContextMenu, visibleIn: [ShellMode.SinglePane, ShellMode.SupportingPane, ShellMode.TwoPane], requiresConfirmation: true },
          { actionId: 'rename', role: ActionRole.Rename, placement: ActionPlacement.ContextMenu, visibleIn: [ShellMode.SinglePane, ShellMode.SupportingPane, ShellMode.TwoPane], requiresConfirmation: false }
        ]
      case ScreenRole.Writing:
        return [
          { actionId: 'back', role: ActionRole.Back, placement: ActionPlacement.TopLeading, visibleIn: [ShellMode.SinglePane, ShellMode.SupportingPane, ShellMode.TwoPane], requiresConfirmation: false },
          { actionId: 'save', role: ActionRole.Save, placement: ActionPlacement.TopTrailing, visibleIn: [ShellMode.SinglePane, ShellMode.SupportingPane, ShellMode.TwoPane], requiresConfirmation: false }
        ]
      case ScreenRole.Settings:
        return [
          { actionId: 'back', role: ActionRole.Back, placement: ActionPlacement.TopLeading, visibleIn: [ShellMode.SinglePane, ShellMode.SupportingPane, ShellMode.TwoPane], requiresConfirmation: false }
        ]
      case ScreenRole.Sync:
        return [
          { actionId: 'back', role: ActionRole.Back, placement: ActionPlacement.TopLeading, visibleIn: [ShellMode.SinglePane, ShellMode.SupportingPane, ShellMode.TwoPane], requiresConfirmation: false },
          { actionId: 'sync', role: ActionRole.Sync, placement: ActionPlacement.Floating, visibleIn: [ShellMode.SinglePane, ShellMode.SupportingPane, ShellMode.TwoPane], requiresConfirmation: false }
        ]
      default:
        return []
    }
  }

  // Native Status methods
  async getNativeLoadStatus(): Promise<ResultEnvelope<string>> {
    return this.success('mock')
  }

  async isWorkspaceValid(): Promise<ResultEnvelope<boolean>> {
    return this.success(true)
  }

  async isAiAvailable(): Promise<ResultEnvelope<boolean>> {
    return this.success(false)
  }
}
