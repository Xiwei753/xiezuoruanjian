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
  StarMapNode,
  StarMapEdge,
  StarMapEmbed,
  StarMapLink,
  StarMapViewport,
  StarMapLayout,
  LocalSettings,
  SyncableSettings,
  EffectiveSettings,
  ResultEnvelope,
  ProjectTree,
  VolumeTree,
  ProjectStats,
  WritingStats,
  DailyStats,
  SyncConfig,
  SyncReport,
  SyncDiagnostics,
  SyncResult,
  RecentEdit,
  SyncState,
  ErrorCode,
  WindowMetrics,
  LayoutPlan,
  FoldPosture,
  WidthClass,
  HeightClass,
  ShellMode,
  EditorMode,
  NavigationMode
} from '../model/CoreDtos'

export class MockWriterCoreBridge implements IWriterCoreBridge {
  private currentWorkspacePath: string = '/mock/workspace'
  private mockProjects: Project[] = []

  private mockVolumes: Map<string, Volume[]> = new Map<string, Volume[]>()

  private mockChapters: Map<string, Chapter[]> = new Map<string, Chapter[]>()

  private mockChapterContents: Map<string, string> = new Map<string, string>()

  private mockStarMaps: StarMapMeta[] = []

  private mockStarMapViewport: StarMapViewport = {
    centerX: 400,
    centerY: 300,
    zoom: 1,
    rotation: 0
  } as StarMapViewport

  private mockStarMapLayout: StarMapLayout = {
    type: 'force',
    params: undefined
  } as StarMapLayout

  private mockStarMapGraph: StarMapGraph = {
    id: '',
    title: '',
    description: '',
    nodes: [] as StarMapNode[],
    edges: [] as StarMapEdge[],
    embeds: [] as StarMapEmbed[],
    links: [] as StarMapLink[],
    viewport: this.mockStarMapViewport,
    layout: this.mockStarMapLayout
  }

  private mockSettings: LocalSettings = {
    fontSize: 16,
    lineHeight: 1.8,
    fontFamily: 'HarmonyOS Sans',
    theme: 'system',
    autoSave: true,
    autoSaveInterval: 30,
    autoIndent: true,
    showWordCount: true,
    showLineNumbers: false,
    wordWrap: true,
    spellCheck: false,
    lastWorkspacePath: '',
    lastProjectId: '',
    lastChapterId: ''
  } as LocalSettings

  private mockSyncConfig: SyncConfig = {
    enabled: false,
    provider: 'github',
    autoSync: false,
    conflictStrategy: 'manual'
  } as SyncConfig

  private delay(ms: number): Promise<void> {
    return new Promise<void>(resolve => setTimeout(resolve, ms))
  }

  private success<T>(data: T): ResultEnvelope<T> {
    const result: ResultEnvelope<T> = {
      success: true,
      data,
      timestamp: Date.now()
    }
    return result
  }

  private error<T>(errorCode: ErrorCode, userMessage: string): ResultEnvelope<T> {
    const result: ResultEnvelope<T> = {
      success: false,
      errorCode: errorCode,
      userMessage,
      timestamp: Date.now()
    }
    return result
  }

  // Workspace methods
  async listWorkspaces(): Promise<ResultEnvelope<Workspace[]>> {
    await this.delay(100)
    return this.success([
      { path: '/mock/workspace', name: '我的写作空间', isValid: true, lockState: 'unlocked' } as Workspace
    ] as Workspace[])
  }

  async openWorkspace(path: string): Promise<ResultEnvelope<WorkspaceState>> {
    await this.delay(200)
    this.currentWorkspacePath = path
    const state: WorkspaceState = {
      workspace: { path, name: '我的写作空间', isValid: true, lockState: 'unlocked' } as Workspace,
      projects: this.mockProjects,
      recentEdits: [] as RecentEdit[],
      syncState: {
        status: 'idle',
        lastSyncAt: '2026-06-14T00:00:00Z'
      } as SyncState
    }
    return this.success(state)
  }

  async validateWorkspace(path: string): Promise<ResultEnvelope<boolean>> {
    await this.delay(100)
    return this.success(true)
  }

  async getWorkspaceState(): Promise<ResultEnvelope<WorkspaceState>> {
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
        chapters: this.mockChapters.get(vol.id) || [] as Chapter[]
      }
      return vt
    })

    const stats: ProjectStats = {
      totalChapters: volumeTrees.reduce((sum: number, vt: VolumeTree) => sum + vt.chapters.length, 0),
      totalVolumes: volumes.length,
      totalWordCount: project.totalWordCount || 0,
      averageChapterLength: 2500,
      lastUpdatedAt: project.updatedAt
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
      name,
      volumeCount: 0,
      chapterCount: 0,
      totalWordCount: 0,
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
    project.name = newName
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
      totalProjects: 2,
      todayChars: 1200,
      todayDuration: 45,
      todaySessions: 3,
      averageSpeed: 800,
      longestStreak: 15,
      currentStreak: 7,
      weeklyStats: [] as DailyStats[]
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
      projectId,
      name,
      order: volumes.length + 1,
      chapterCount: 0,
      totalWordCount: 0,
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
    volume.name = newName
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
  async listChapters(volumeId: string): Promise<ResultEnvelope<Chapter[]>> {
    await this.delay(100)
    return this.success(this.mockChapters.get(volumeId) || [] as Chapter[])
  }

  async loadChapter(chapterId: string): Promise<ResultEnvelope<ChapterData>> {
    await this.delay(200)
    const content = this.mockChapterContents.get(chapterId)
    if (content !== undefined) {
      const data: ChapterData = {
        id: chapterId,
        title: '第一章 初遇',
        content,
        wordCount: content.length,
        volumeId: 'vol-001',
        projectId: 'proj-001',
        updatedAt: new Date().toISOString(),
        status: 'draft'
      }
      return this.success(data)
    }

    const volKeys: string[] = Array.from(this.mockChapters.keys())
    for (let i = 0; i < volKeys.length; i++) {
      const volId: string = volKeys[i]
      const chapters: Chapter[] = this.mockChapters.get(volId) || [] as Chapter[]
      const chapter = chapters.find(ch => ch.id === chapterId)
      if (chapter) {
        const data: ChapterData = {
          id: chapter.id,
          title: chapter.name,
          content: `这是《${chapter.name}》的示例内容。\n\n在此处开始写作...\n\n鸿蒙端当前使用 mock 数据，后续将对接真实 Rust Core。`,
          wordCount: chapter.wordCount,
          volumeId: chapter.volumeId,
          projectId: 'proj-001',
          updatedAt: chapter.updatedAt,
          status: chapter.status
        }
        return this.success(data)
      }
    }

    return this.error('CHAPTER_NOT_FOUND', '章节不存在')
  }

  async saveChapter(chapterId: string, text: string): Promise<ResultEnvelope<SaveReceipt>> {
    await this.delay(300)
    if (!text || text.trim().length === 0) {
      const errorReceipt: SaveReceipt = {
        success: false,
        wordCount: 0,
        savedAt: new Date().toISOString(),
        errorCode: 'EMPTY_OVERWRITE_BLOCKED',
        errorMessage: '不能保存空内容'
      }
      const result: ResultEnvelope<SaveReceipt> = {
        success: false,
        errorCode: 'EMPTY_OVERWRITE_BLOCKED',
        userMessage: '不能保存空内容',
        data: errorReceipt,
        timestamp: Date.now()
      }
      return result
    }

    this.mockChapterContents.set(chapterId, text)
    const receipt: SaveReceipt = {
      success: true,
      wordCount: text.replace(/\s/g, '').length,
      savedAt: new Date().toISOString()
    }
    return this.success(receipt)
  }

  async clearChapter(chapterId: string): Promise<ResultEnvelope<SaveReceipt>> {
    await this.delay(200)
    this.mockChapterContents.set(chapterId, '')
    const receipt: SaveReceipt = {
      success: true,
      wordCount: 0,
      savedAt: new Date().toISOString()
    }
    return this.success(receipt)
  }

  async createChapter(volumeId: string, name: string): Promise<ResultEnvelope<Chapter>> {
    await this.delay(150)
    const chapters = this.mockChapters.get(volumeId) || [] as Chapter[]
    const newChapter: Chapter = {
      id: `ch-${Date.now()}`,
      volumeId,
      name,
      wordCount: 0,
      order: chapters.length + 1,
      updatedAt: new Date().toISOString(),
      createdAt: new Date().toISOString(),
      status: 'draft'
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
      const chapters: Chapter[] = this.mockChapters.get(volId) || [] as Chapter[]
      const chapter = chapters.find(ch => ch.id === chapterId)
      if (chapter) {
        chapter.name = newName
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
      const chapters: Chapter[] = this.mockChapters.get(volId) || [] as Chapter[]
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
    const chapters = this.mockChapters.get(volumeId) || [] as Chapter[]
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
    return this.success(this.mockStarMaps.filter(sm => sm.projectId === projectId))
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
      projectId: '',
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
      layoutType: 'force'
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

  // Settings methods
  async getLocalSettings(): Promise<ResultEnvelope<LocalSettings>> {
    await this.delay(100)
    const copy: LocalSettings = {
      fontSize: this.mockSettings.fontSize,
      lineHeight: this.mockSettings.lineHeight,
      fontFamily: this.mockSettings.fontFamily,
      theme: this.mockSettings.theme,
      autoSave: this.mockSettings.autoSave,
      autoSaveInterval: this.mockSettings.autoSaveInterval,
      autoIndent: this.mockSettings.autoIndent,
      showWordCount: this.mockSettings.showWordCount,
      showLineNumbers: this.mockSettings.showLineNumbers,
      wordWrap: this.mockSettings.wordWrap,
      spellCheck: this.mockSettings.spellCheck,
      lastWorkspacePath: this.mockSettings.lastWorkspacePath,
      lastProjectId: this.mockSettings.lastProjectId,
      lastChapterId: this.mockSettings.lastChapterId
    }
    return this.success(copy)
  }

  async saveLocalSettings(settings: LocalSettings): Promise<ResultEnvelope<boolean>> {
    await this.delay(200)
    this.mockSettings = {
      fontSize: settings.fontSize,
      lineHeight: settings.lineHeight,
      fontFamily: settings.fontFamily,
      theme: settings.theme,
      autoSave: settings.autoSave,
      autoSaveInterval: settings.autoSaveInterval,
      autoIndent: settings.autoIndent,
      showWordCount: settings.showWordCount,
      showLineNumbers: settings.showLineNumbers,
      wordWrap: settings.wordWrap,
      spellCheck: settings.spellCheck,
      lastWorkspacePath: settings.lastWorkspacePath,
      lastProjectId: settings.lastProjectId,
      lastChapterId: settings.lastChapterId
    }
    return this.success(true)
  }

  async getSyncableSettings(): Promise<ResultEnvelope<SyncableSettings>> {
    await this.delay(100)
    const result: SyncableSettings = {
      fontSize: this.mockSettings.fontSize,
      lineHeight: this.mockSettings.lineHeight,
      fontFamily: this.mockSettings.fontFamily,
      theme: this.mockSettings.theme,
      autoIndent: this.mockSettings.autoIndent,
      showWordCount: this.mockSettings.showWordCount,
      showLineNumbers: this.mockSettings.showLineNumbers,
      wordWrap: this.mockSettings.wordWrap,
      uiLanguage: this.mockSettings.uiLanguage
    }
    return this.success(result)
  }

  async saveSyncableSettings(settings: SyncableSettings): Promise<ResultEnvelope<boolean>> {
    await this.delay(200)
    if (settings.fontSize !== undefined) this.mockSettings.fontSize = settings.fontSize
    if (settings.lineHeight !== undefined) this.mockSettings.lineHeight = settings.lineHeight
    if (settings.fontFamily !== undefined) this.mockSettings.fontFamily = settings.fontFamily
    if (settings.theme !== undefined) this.mockSettings.theme = settings.theme
    if (settings.autoIndent !== undefined) this.mockSettings.autoIndent = settings.autoIndent
    if (settings.showWordCount !== undefined) this.mockSettings.showWordCount = settings.showWordCount
    if (settings.showLineNumbers !== undefined) this.mockSettings.showLineNumbers = settings.showLineNumbers
    if (settings.wordWrap !== undefined) this.mockSettings.wordWrap = settings.wordWrap
    if (settings.uiLanguage !== undefined) this.mockSettings.uiLanguage = settings.uiLanguage
    return this.success(true)
  }

  async getEffectiveSettings(): Promise<ResultEnvelope<EffectiveSettings>> {
    await this.delay(100)
    const result: EffectiveSettings = {
      fontSize: this.mockSettings.fontSize,
      lineHeight: this.mockSettings.lineHeight,
      fontFamily: this.mockSettings.fontFamily,
      theme: this.mockSettings.theme,
      autoSave: this.mockSettings.autoSave,
      autoSaveInterval: this.mockSettings.autoSaveInterval,
      autoIndent: this.mockSettings.autoIndent,
      showWordCount: this.mockSettings.showWordCount,
      showLineNumbers: this.mockSettings.showLineNumbers,
      wordWrap: this.mockSettings.wordWrap,
      spellCheck: this.mockSettings.spellCheck,
      lastWorkspacePath: this.mockSettings.lastWorkspacePath,
      lastProjectId: this.mockSettings.lastProjectId,
      lastChapterId: this.mockSettings.lastChapterId,
      isSynced: false
    }
    return this.success(result)
  }

  // Sync methods
  async loadSyncConfig(): Promise<ResultEnvelope<SyncConfig>> {
    await this.delay(100)
    const copy: SyncConfig = {
      enabled: this.mockSyncConfig.enabled,
      provider: this.mockSyncConfig.provider,
      remoteUrl: this.mockSyncConfig.remoteUrl,
      branch: this.mockSyncConfig.branch,
      autoSync: this.mockSyncConfig.autoSync,
      autoSyncInterval: this.mockSyncConfig.autoSyncInterval,
      conflictStrategy: this.mockSyncConfig.conflictStrategy,
      lastSyncAt: this.mockSyncConfig.lastSyncAt,
      syncPath: this.mockSyncConfig.syncPath
    }
    return this.success(copy)
  }

  async saveSyncConfig(config: SyncConfig): Promise<ResultEnvelope<boolean>> {
    await this.delay(200)
    this.mockSyncConfig = {
      enabled: config.enabled,
      provider: config.provider,
      remoteUrl: config.remoteUrl,
      branch: config.branch,
      autoSync: config.autoSync,
      autoSyncInterval: config.autoSyncInterval,
      conflictStrategy: config.conflictStrategy,
      lastSyncAt: config.lastSyncAt,
      syncPath: config.syncPath
    }
    return this.success(true)
  }

  async syncDryRun(): Promise<ResultEnvelope<SyncReport>> {
    await this.delay(300)
    const report: SyncReport = {
      filesToAdd: [] as string[],
      filesToModify: ['projects/proj-001/volumes/vol-001/chapters/ch-001.md'] as string[],
      filesToDelete: [] as string[],
      conflicts: [] as string[],
      warnings: [] as string[],
      estimatedDuration: 5
    }
    return this.success(report)
  }

  async syncDiagnostics(): Promise<ResultEnvelope<SyncDiagnostics>> {
    await this.delay(200)
    const diag: SyncDiagnostics = {
      isGitRepository: true,
      hasRemote: true,
      remoteUrl: 'https://github.com/example/writer-data.git',
      branch: 'main',
      isClean: false,
      lastCommit: 'abc123',
      lastCommitDate: '2026-06-14T00:00:00Z',
      errors: [] as string[],
      warnings: ['有 1 个未提交的更改'] as string[]
    }
    return this.success(diag)
  }

  async performSync(): Promise<ResultEnvelope<SyncResult>> {
    await this.delay(1000)
    const result: SyncResult = {
      success: true,
      filesAdded: 0,
      filesModified: 1,
      filesDeleted: 0,
      conflicts: [] as string[],
      duration: 1200
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
      totalProjects: 2,
      todayChars: 1200,
      todayDuration: 45,
      todaySessions: 3,
      averageSpeed: 800,
      longestStreak: 15,
      currentStreak: 7,
      weeklyStats: [
        { date: '2026-06-14', charsAdded: 1200, durationSeconds: 2700, sessionsCount: 3 } as DailyStats,
        { date: '2026-06-13', charsAdded: 2500, durationSeconds: 5400, sessionsCount: 5 } as DailyStats,
        { date: '2026-06-12', charsAdded: 1800, durationSeconds: 3600, sessionsCount: 4 } as DailyStats,
        { date: '2026-06-11', charsAdded: 3200, durationSeconds: 7200, sessionsCount: 6 } as DailyStats,
        { date: '2026-06-10', charsAdded: 800, durationSeconds: 1800, sessionsCount: 2 } as DailyStats,
        { date: '2026-06-09', charsAdded: 2100, durationSeconds: 4500, sessionsCount: 4 } as DailyStats,
        { date: '2026-06-08', charsAdded: 1500, durationSeconds: 3000, sessionsCount: 3 } as DailyStats
      ] as DailyStats[]
    }
    return this.success(stats)
  }

  async calculateWordCount(text: string): Promise<number> {
    return text.replace(/\s/g, '').length
  }

  async processWritingEvent(charsAdded: number, durationSeconds: number): Promise<ResultEnvelope<boolean>> {
    await this.delay(50)
    return this.success(true)
  }

  async getRecentEdits(): Promise<ResultEnvelope<RecentEdit[]>> {
    await this.delay(100)
    return this.success([] as RecentEdit[])
  }

  // Layout Policy methods
  resolveLayout(metrics: WindowMetrics): Promise<ResultEnvelope<LayoutPlan>> {
    const widthVp = metrics.widthVp
    const heightVp = metrics.heightVp

    let widthClass: WidthClass
    if (widthVp < 600) {
      widthClass = WidthClass.Compact
    } else if (widthVp < 840) {
      widthClass = WidthClass.Medium
    } else {
      widthClass = WidthClass.Expanded
    }

    let heightClass: HeightClass
    if (heightVp < 480) {
      heightClass = HeightClass.Compact
    } else if (heightVp < 900) {
      heightClass = HeightClass.Medium
    } else {
      heightClass = HeightClass.Expanded
    }

    let shellMode: ShellMode
    let navigationMode: NavigationMode
    let editorMode: EditorMode
    let contentMaxWidthVp: number
    let pagePaddingVp: number
    let gridColumns: number
    let showSidePanel: boolean
    let showBottomBar: boolean

    switch (widthClass) {
      case WidthClass.Compact:
        shellMode = ShellMode.SinglePane
        navigationMode = NavigationMode.Stack
        editorMode = EditorMode.FullWidth
        contentMaxWidthVp = 0
        pagePaddingVp = 16
        gridColumns = 2
        showSidePanel = false
        showBottomBar = true
        break
      case WidthClass.Medium:
        shellMode = ShellMode.SupportingPane
        navigationMode = NavigationMode.Stack
        editorMode = EditorMode.CenteredPaper
        contentMaxWidthVp = 720
        pagePaddingVp = 24
        gridColumns = 3
        showSidePanel = false
        showBottomBar = true
        break
      case WidthClass.Expanded:
        shellMode = ShellMode.TwoPane
        navigationMode = NavigationMode.ListDetail
        editorMode = EditorMode.CenteredPaper
        contentMaxWidthVp = 840
        pagePaddingVp = 32
        gridColumns = 4
        showSidePanel = true
        showBottomBar = false
        break
    }

    if (metrics.foldPosture === FoldPosture.HalfOpened) {
      shellMode = ShellMode.SupportingPane
    }

    if (metrics.keyboardVisible) {
      showBottomBar = false
    }

    const plan: LayoutPlan = {
      widthClass,
      heightClass,
      shellMode,
      editorMode,
      navigationMode,
      contentMaxWidthVp,
      pagePaddingVp,
      gridColumns,
      showSidePanel,
      showBottomBar
    }

    return Promise.resolve({
      success: true,
      data: plan
    } as ResultEnvelope<LayoutPlan>)
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
