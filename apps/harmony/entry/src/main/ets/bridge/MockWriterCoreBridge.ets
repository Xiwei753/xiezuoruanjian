import { IWriterCoreBridge } from './IWriterCoreBridge'
import {
  AppSummary,
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
  ThemePaletteRecordDto,
  ThemeColorSchemeDto,
  BuiltinThemeDto,
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
  WindowCapabilities,
  LayoutContract,
  ScreenPolicy,
  ActionSlot,
  ScreenRole,
  ActionRole,
  ActionRegion,
  ActionTarget,
  EditorVisualTransaction,
  EditorTransactionCause
} from '../model/CoreDtos'
import { resolveLayoutFromCapabilities } from '../common/LayoutPolicyHelper'

export class MockWriterCoreBridge implements IWriterCoreBridge {
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
    linuxQtSidebarWidth: 280,
    linuxQtEditorWidth: 820
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

  // App State 查询方法
  async listAppSummaries(): Promise<ResultEnvelope<AppSummary[]>> {
    await this.delay(100)
    return this.success([
      {
        projects: this.mockProjects,
        recentEdits: [] as RecentEdit[]
      } as AppSummary
    ] as AppSummary[])
  }

  async getAppState(): Promise<ResultEnvelope<AppSummary>> {
    await this.delay(200)
    const summary: AppSummary = {
      projects: this.mockProjects,
      recentEdits: [] as RecentEdit[]
    }
    return this.success(summary)
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
      linuxQtSidebarWidth: this.mockSettings.linuxQtSidebarWidth,
      linuxQtEditorWidth: this.mockSettings.linuxQtEditorWidth
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
      linuxQtSidebarWidth: settings.linuxQtSidebarWidth,
      linuxQtEditorWidth: settings.linuxQtEditorWidth
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

  async listPaletteRecords(): Promise<ResultEnvelope<ThemePaletteRecordDto[]>> {
    await this.delay(100)
    return this.success([] as ThemePaletteRecordDto[])
  }

  async loadPaletteRecord(deviceId: string, fingerprint: string): Promise<ResultEnvelope<ThemePaletteRecordDto>> {
    await this.delay(100)
    return this.error('NOT_FOUND', 'Palette record not found')
  }

  async deletePaletteRecord(deviceId: string, fingerprint: string): Promise<ResultEnvelope<boolean>> {
    await this.delay(100)
    return this.success(true)
  }

  async listBuiltinThemes(): Promise<ResultEnvelope<BuiltinThemeDto[]>> {
    await this.delay(100)
    return this.success([{
      theme_id: 'sujian_default',
      name: '素笺默认',
      light_scheme: {} as ThemeColorSchemeDto,
      dark_scheme: {} as ThemeColorSchemeDto
    }] as BuiltinThemeDto[])
  }

  // Sync methods
  async loadSyncConfig(_projectId: string): Promise<ResultEnvelope<SyncConfig>> {
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

  async saveSyncConfig(_projectId: string, config: SyncConfig): Promise<ResultEnvelope<boolean>> {
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

  async syncDiagnostics(_projectId: string): Promise<ResultEnvelope<SyncDiagnosticsResult>> {
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

  // ── 应用级同步通道（Issue #600 评论 #3 问题四，mock 占位） ──
  async loadAppSyncConfig(): Promise<ResultEnvelope<SyncConfig>> {
    await this.delay(100)
    return this.loadSyncConfig('app')
  }

  async saveAppSyncConfig(config: SyncConfig): Promise<ResultEnvelope<boolean>> {
    await this.delay(200)
    return this.saveSyncConfig('app', config)
  }

  async performAppSync(): Promise<ResultEnvelope<SyncResult>> {
    await this.delay(300)
    return this.performSync('app')
  }

  async appSyncDiagnostics(): Promise<ResultEnvelope<SyncDiagnosticsResult>> {
    await this.delay(500)
    return this.syncDiagnostics('app')
  }

  // per-project sync: projectId selects the sync root (mock ignores it).
  async syncDryRun(projectId: string): Promise<ResultEnvelope<SyncResult>> {
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

  async performSync(projectId: string): Promise<ResultEnvelope<SyncResult>> {
    await this.delay(1000)
    const result: SyncResult = {
      status: 'success',
      uploadedFiles: [`projects/${projectId}/volumes/vol-001/chapters/ch-001.md`] as string[],
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

  // Layout Contract methods — 委托给 LayoutPolicyHelper 纯函数（与 Core 完全一致）
  resolveLayout(capabilities: WindowCapabilities): Promise<ResultEnvelope<LayoutContract>> {
    const contract = resolveLayoutFromCapabilities(capabilities)
    return Promise.resolve({
      success: true,
      data: contract,
      warnings: [] as string[],
      changedPaths: [] as string[],
      changedEntities: [] as ChangedEntity[]
    } as ResultEnvelope<LayoutContract>)
  }

  // Screen Policy methods — mock 返回与 Core 一致的策略（#610：区域/顺序是产品语义）
  resolveScreenPolicy(screenRole: ScreenRole): Promise<ResultEnvelope<ScreenPolicy>> {
    const policy = this.mockResolveScreenPolicy(screenRole)
    return Promise.resolve(this.success(policy))
  }

  private mockResolveScreenPolicy(screenRole: ScreenRole): ScreenPolicy {
    return {
      screenRole: screenRole,
      actionSlots: this.mockBaseSlots(screenRole)
    }
  }

  private mockBaseSlots(screenRole: ScreenRole): ActionSlot[] {
    switch (screenRole) {
      case ScreenRole.Home:
        return [
          { role: ActionRole.Search, target: ActionTarget.App, region: ActionRegion.HeaderTrailing, order: 20, requiresConfirmation: false },
          { role: ActionRole.Settings, target: ActionTarget.App, region: ActionRegion.HeaderTrailing, order: 30, requiresConfirmation: false }
        ]
      case ScreenRole.ProjectList:
        return [
          // #610 评论五：作品列表顶栏右侧与 ProjectWorkspace 一致，
          // 同步 / 搜索 / 设置（order 升序）。
          { role: ActionRole.Sync, target: ActionTarget.App, region: ActionRegion.HeaderTrailing, order: 10, requiresConfirmation: false },
          { role: ActionRole.Search, target: ActionTarget.App, region: ActionRegion.HeaderTrailing, order: 20, requiresConfirmation: false },
          { role: ActionRole.Settings, target: ActionTarget.App, region: ActionRegion.HeaderTrailing, order: 30, requiresConfirmation: false },
          // #610 评论四：新建作品是页面主操作（PrimaryAction）。
          { role: ActionRole.CreateProject, target: ActionTarget.Project, region: ActionRegion.PrimaryAction, order: 10, requiresConfirmation: false },
          { role: ActionRole.Delete, target: ActionTarget.Project, region: ActionRegion.Context, order: 10, requiresConfirmation: true },
          { role: ActionRole.Rename, target: ActionTarget.Project, region: ActionRegion.Context, order: 20, requiresConfirmation: false }
        ]
      case ScreenRole.ProjectWorkspace:
        return [
          // #610 评论五：作品工作区顶栏左侧返回动作。
          { role: ActionRole.Back, target: ActionTarget.App, region: ActionRegion.HeaderLeading, order: 10, requiresConfirmation: false },
          { role: ActionRole.Sync, target: ActionTarget.App, region: ActionRegion.HeaderTrailing, order: 10, requiresConfirmation: false },
          { role: ActionRole.Search, target: ActionTarget.App, region: ActionRegion.HeaderTrailing, order: 20, requiresConfirmation: false },
          { role: ActionRole.Settings, target: ActionTarget.App, region: ActionRegion.HeaderTrailing, order: 30, requiresConfirmation: false },
          { role: ActionRole.CreateVolume, target: ActionTarget.Project, region: ActionRegion.ListHeader, order: 10, requiresConfirmation: false },
          { role: ActionRole.CreateChapter, target: ActionTarget.Volume, region: ActionRegion.ItemTrailing, order: 10, requiresConfirmation: false },
          { role: ActionRole.CreateChapter, target: ActionTarget.Volume, region: ActionRegion.EmptyState, order: 10, requiresConfirmation: false },
          { role: ActionRole.Delete, target: ActionTarget.Volume, region: ActionRegion.Context, order: 10, requiresConfirmation: true },
          { role: ActionRole.Delete, target: ActionTarget.Chapter, region: ActionRegion.Context, order: 20, requiresConfirmation: true },
          { role: ActionRole.Rename, target: ActionTarget.Volume, region: ActionRegion.Context, order: 30, requiresConfirmation: false },
          { role: ActionRole.Rename, target: ActionTarget.Chapter, region: ActionRegion.Context, order: 40, requiresConfirmation: false },
          // #610 评论四：卷/章节的真实顺序动作（上移/下移）。
          { role: ActionRole.MoveEarlier, target: ActionTarget.Volume, region: ActionRegion.Context, order: 50, requiresConfirmation: false },
          { role: ActionRole.MoveLater, target: ActionTarget.Volume, region: ActionRegion.Context, order: 60, requiresConfirmation: false },
          { role: ActionRole.MoveEarlier, target: ActionTarget.Chapter, region: ActionRegion.Context, order: 70, requiresConfirmation: false },
          { role: ActionRole.MoveLater, target: ActionTarget.Chapter, region: ActionRegion.Context, order: 80, requiresConfirmation: false }
        ]
      case ScreenRole.Writing:
        return [
          { role: ActionRole.Back, target: ActionTarget.App, region: ActionRegion.HeaderLeading, order: 10, requiresConfirmation: false },
          { role: ActionRole.Sync, target: ActionTarget.App, region: ActionRegion.HeaderTrailing, order: 20, requiresConfirmation: false },
          { role: ActionRole.Settings, target: ActionTarget.App, region: ActionRegion.HeaderTrailing, order: 30, requiresConfirmation: false }
        ]
      case ScreenRole.StarMap:
      case ScreenRole.Stats:
        return []
      case ScreenRole.Settings:
        return [
          { role: ActionRole.Back, target: ActionTarget.App, region: ActionRegion.HeaderLeading, order: 10, requiresConfirmation: false }
        ]
      case ScreenRole.Sync:
        return [
          { role: ActionRole.Back, target: ActionTarget.App, region: ActionRegion.HeaderLeading, order: 10, requiresConfirmation: false },
          { role: ActionRole.Sync, target: ActionTarget.App, region: ActionRegion.HeaderTrailing, order: 10, requiresConfirmation: false }
        ]
      default:
        return []
    }
  }

  // Native Status methods
  async getNativeLoadStatus(): Promise<ResultEnvelope<string>> {
    return this.success('mock')
  }

  async isAiAvailable(): Promise<ResultEnvelope<boolean>> {
    return this.success(false)
  }

  // Editor Animation — mock 返回 null（不需要动画）
  async editorVisualTransaction(
    oldText: string,
    newText: string,
    oldCursorIndex: number,
    newCursorIndex: number,
    cause: EditorTransactionCause,
    maxAnimatedChars: number,
    animationDurationMs: number
  ): Promise<ResultEnvelope<EditorVisualTransaction | null>> {
    await this.delay(10)
    return this.success(null)
  }
}
