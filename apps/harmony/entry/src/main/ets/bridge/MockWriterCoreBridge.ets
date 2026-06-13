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
  WritingStats,
  SyncConfig,
  SyncReport,
  SyncDiagnostics,
  SyncResult,
  RecentEdit
} from '../model/CoreDtos'

export class MockWriterCoreBridge implements IWriterCoreBridge {
  private currentWorkspacePath: string = '/mock/workspace'
  private mockProjects: Project[] = [
    {
      id: 'proj-001',
      name: '示例小说',
      volumeCount: 2,
      chapterCount: 5,
      totalWordCount: 16500,
      createdAt: '2026-01-15T10:00:00Z',
      updatedAt: '2026-06-14T08:30:00Z',
      description: '一个关于冒险的故事'
    },
    {
      id: 'proj-002',
      name: '随笔集',
      volumeCount: 1,
      chapterCount: 3,
      totalWordCount: 5500,
      createdAt: '2026-03-20T14:00:00Z',
      updatedAt: '2026-06-13T20:15:00Z',
      description: '日常生活感悟'
    }
  ]

  private mockVolumes: Map<string, Volume[]> = new Map([
    ['proj-001', [
      { id: 'vol-001', projectId: 'proj-001', name: '第一卷 启程', order: 1, chapterCount: 3, totalWordCount: 9500 },
      { id: 'vol-002', projectId: 'proj-001', name: '第二卷 风暴', order: 2, chapterCount: 2, totalWordCount: 7000 }
    ]],
    ['proj-002', [
      { id: 'vol-003', projectId: 'proj-002', name: '春日随笔', order: 1, chapterCount: 3, totalWordCount: 5500 }
    ]]
  ])

  private mockChapters: Map<string, Chapter[]> = new Map([
    ['vol-001', [
      { id: 'ch-001', volumeId: 'vol-001', name: '第一章 初遇', wordCount: 3200, order: 1, updatedAt: '2026-06-14T08:30:00Z', status: 'draft' },
      { id: 'ch-002', volumeId: 'vol-001', name: '第二章 出发', wordCount: 2800, order: 2, updatedAt: '2026-06-13T15:20:00Z', status: 'draft' },
      { id: 'ch-003', volumeId: 'vol-001', name: '第三章 旅途', wordCount: 3500, order: 3, updatedAt: '2026-06-12T22:10:00Z', status: 'draft' }
    ]],
    ['vol-002', [
      { id: 'ch-004', volumeId: 'vol-002', name: '第四章 暴风雨', wordCount: 4100, order: 1, updatedAt: '2026-06-11T18:45:00Z', status: 'draft' },
      { id: 'ch-005', volumeId: 'vol-002', name: '第五章 新生', wordCount: 2900, order: 2, updatedAt: '2026-06-10T12:30:00Z', status: 'draft' }
    ]],
    ['vol-003', [
      { id: 'ch-006', volumeId: 'vol-003', name: '樱花时节', wordCount: 1800, order: 1, updatedAt: '2026-06-09T09:15:00Z', status: 'draft' },
      { id: 'ch-007', volumeId: 'vol-003', name: '雨后', wordCount: 2200, order: 2, updatedAt: '2026-06-08T16:40:00Z', status: 'draft' },
      { id: 'ch-008', volumeId: 'vol-003', name: '夏日蝉鸣', wordCount: 1500, order: 3, updatedAt: '2026-06-07T11:20:00Z', status: 'draft' }
    ]]
  ])

  private mockChapterContents: Map<string, string> = new Map([
    ['ch-001', '那是一个风和日丽的早晨，林间小路上走来了一个年轻人。\n\n他背着简单的行囊，目光坚定地望着远方。这是他第一次离开家乡，心中既有期待，也有些许不安。\n\n"终于要开始了。"他自言自语道，嘴角微微上扬。\n\n前方的路还很长，但他已经做好了准备。'],
    ['ch-002', '清晨的阳光透过窗帘的缝隙洒进房间。\n\n李明睁开眼睛，看了一眼墙上的时钟。六点半，比闹钟早了十分钟。\n\n他迅速起床，洗漱完毕，背上早已准备好的背包。今天是他出发的日子。'],
  ])

  private mockStarMaps: StarMapMeta[] = [
    {
      id: 'starmap-001',
      title: '人物关系图',
      description: '主要角色之间的关系网络',
      nodeCount: 8,
      edgeCount: 12,
      projectId: 'proj-001',
      createdAt: '2026-02-10T10:00:00Z',
      updatedAt: '2026-06-14T08:00:00Z',
      layoutType: 'force'
    },
    {
      id: 'starmap-002',
      title: '剧情线索',
      description: '故事主线与支线发展',
      nodeCount: 15,
      edgeCount: 20,
      projectId: 'proj-001',
      createdAt: '2026-03-05T14:30:00Z',
      updatedAt: '2026-06-13T16:45:00Z',
      layoutType: 'tree'
    }
  ]

  private mockStarMapGraph: StarMapGraph = {
    id: 'starmap-001',
    title: '人物关系图',
    description: '主要角色之间的关系网络',
    nodes: [
      { id: 'node-001', label: '主角', x: 400, y: 300, type: 'character', color: '#4A90D9', description: '故事的主人公' },
      { id: 'node-002', label: '伙伴A', x: 250, y: 200, type: 'character', color: '#7B68EE', description: '主角的挚友' },
      { id: 'node-003', label: '伙伴B', x: 550, y: 200, type: 'character', color: '#7B68EE', description: '旅途中的同伴' },
      { id: 'node-004', label: '导师', x: 300, y: 450, type: 'character', color: '#2ECC71', description: '传授技艺的长者' },
      { id: 'node-005', label: '对手', x: 500, y: 450, type: 'character', color: '#E74C3C', description: '宿命之敌' },
      { id: 'node-006', label: '故乡', x: 150, y: 350, type: 'location', color: '#F39C12', description: '主角出生的地方' },
      { id: 'node-007', label: '王城', x: 650, y: 350, type: 'location', color: '#F39C12', description: '王国的中心' },
      { id: 'node-008', label: '启程', x: 200, y: 100, type: 'event', color: '#9B59B6', description: '冒险的开始' }
    ],
    edges: [
      { id: 'edge-001', sourceId: 'node-001', targetId: 'node-002', label: '挚友', type: 'relation', style: 'solid' },
      { id: 'edge-002', sourceId: 'node-001', targetId: 'node-003', label: '同伴', type: 'relation', style: 'solid' },
      { id: 'edge-003', sourceId: 'node-001', targetId: 'node-004', label: '师徒', type: 'relation', style: 'solid' },
      { id: 'edge-004', sourceId: 'node-001', targetId: 'node-005', label: '宿敌', type: 'conflict', style: 'dashed' },
      { id: 'edge-005', sourceId: 'node-001', targetId: 'node-006', label: '出身地', type: 'relation', style: 'solid' },
      { id: 'edge-006', sourceId: 'node-001', targetId: 'node-008', label: '经历', type: 'timeline', style: 'solid' },
      { id: 'edge-007', sourceId: 'node-002', targetId: 'node-003', label: '相识', type: 'relation', style: 'solid' },
      { id: 'edge-008', sourceId: 'node-004', targetId: 'node-005', label: '旧识', type: 'relation', style: 'dotted' },
      { id: 'edge-009', sourceId: 'node-005', targetId: 'node-007', label: '盘踞', type: 'relation', style: 'solid' },
      { id: 'edge-010', sourceId: 'node-008', targetId: 'node-006', label: '离开', type: 'timeline', style: 'solid' },
      { id: 'edge-011', sourceId: 'node-002', targetId: 'node-004', label: '同门', type: 'relation', style: 'solid' },
      { id: 'edge-012', sourceId: 'node-003', targetId: 'node-007', label: '目的地', type: 'dependency', style: 'solid' }
    ],
    embeds: [],
    links: [],
    viewport: { centerX: 400, centerY: 300, zoom: 1, rotation: 0 },
    layout: { type: 'force', params: { gravity: 0.1, linkDistance: 100 } }
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
    lastWorkspacePath: '/mock/workspace',
    lastProjectId: 'proj-001',
    lastChapterId: 'ch-001'
  }

  private mockSyncConfig: SyncConfig = {
    enabled: false,
    provider: 'github',
    autoSync: false,
    conflictStrategy: 'manual'
  }

  private delay(ms: number): Promise<void> {
    return new Promise(resolve => setTimeout(resolve, ms))
  }

  private success<T>(data: T): ResultEnvelope<T> {
    return {
      success: true,
      data,
      timestamp: Date.now()
    }
  }

  private error<T>(errorCode: string, userMessage: string): ResultEnvelope<T> {
    return {
      success: false,
      errorCode: errorCode as any,
      userMessage,
      timestamp: Date.now()
    }
  }

  // Workspace methods
  async listWorkspaces(): Promise<ResultEnvelope<Workspace[]>> {
    await this.delay(100)
    return this.success([
      { path: '/mock/workspace', name: '我的写作空间', isValid: true, lockState: 'unlocked' }
    ])
  }

  async openWorkspace(path: string): Promise<ResultEnvelope<WorkspaceState>> {
    await this.delay(200)
    this.currentWorkspacePath = path
    return this.success({
      workspace: { path, name: '我的写作空间', isValid: true, lockState: 'unlocked' },
      projects: this.mockProjects,
      recentEdits: [
        {
          projectId: 'proj-001',
          projectTitle: '示例小说',
          chapterId: 'ch-001',
          chapterTitle: '第一章 初遇',
          editedAt: '2026-06-14T08:30:00Z',
          wordCount: 3200
        }
      ],
      syncState: {
        status: 'idle',
        lastSyncAt: '2026-06-14T00:00:00Z'
      }
    })
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

    const volumes = this.mockVolumes.get(projectId) || []
    const volumeTrees = volumes.map(vol => ({
      volume: vol,
      chapters: this.mockChapters.get(vol.id) || []
    }))

    return this.success({
      project,
      volumes: volumeTrees,
      stats: {
        totalChapters: volumeTrees.reduce((sum, vt) => sum + vt.chapters.length, 0),
        totalVolumes: volumes.length,
        totalWordCount: project.totalWordCount || 0,
        averageChapterLength: 2500,
        lastUpdatedAt: project.updatedAt
      }
    })
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
    this.mockVolumes.set(newProject.id, [])
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
    return this.success({
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
      weeklyStats: []
    })
  }

  // Volume methods
  async listVolumes(projectId: string): Promise<ResultEnvelope<Volume[]>> {
    await this.delay(100)
    return this.success(this.mockVolumes.get(projectId) || [])
  }

  async createVolume(projectId: string, name: string): Promise<ResultEnvelope<Volume>> {
    await this.delay(150)
    const volumes = this.mockVolumes.get(projectId) || []
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
    const volumes = this.mockVolumes.get(projectId) || []
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
    const volumes = this.mockVolumes.get(projectId) || []
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
    const volumes = this.mockVolumes.get(projectId) || []
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
    return this.success(this.mockChapters.get(volumeId) || [])
  }

  async loadChapter(chapterId: string): Promise<ResultEnvelope<ChapterData>> {
    await this.delay(200)
    const content = this.mockChapterContents.get(chapterId)
    if (content !== undefined) {
      return this.success({
        id: chapterId,
        title: '第一章 初遇',
        content,
        wordCount: content.length,
        volumeId: 'vol-001',
        projectId: 'proj-001',
        updatedAt: new Date().toISOString(),
        status: 'draft'
      })
    }

    for (const [volId, chapters] of this.mockChapters) {
      const chapter = chapters.find(ch => ch.id === chapterId)
      if (chapter) {
        return this.success({
          id: chapter.id,
          title: chapter.name,
          content: `这是《${chapter.name}》的示例内容。\n\n在此处开始写作...\n\n鸿蒙端当前使用 mock 数据，后续将对接真实 Rust Core。`,
          wordCount: chapter.wordCount,
          volumeId: chapter.volumeId,
          projectId: 'proj-001',
          updatedAt: chapter.updatedAt,
          status: chapter.status
        })
      }
    }

    return this.error('CHAPTER_NOT_FOUND', '章节不存在')
  }

  async saveChapter(chapterId: string, text: string): Promise<ResultEnvelope<SaveReceipt>> {
    await this.delay(300)
    if (!text || text.trim().length === 0) {
      return {
        success: false,
        errorCode: 'EMPTY_OVERWRITE_BLOCKED',
        userMessage: '不能保存空内容',
        data: {
          success: false,
          wordCount: 0,
          savedAt: new Date().toISOString(),
          errorCode: 'EMPTY_OVERWRITE_BLOCKED',
          errorMessage: '不能保存空内容'
        },
        timestamp: Date.now()
      }
    }

    this.mockChapterContents.set(chapterId, text)
    return this.success({
      success: true,
      wordCount: text.replace(/\s/g, '').length,
      savedAt: new Date().toISOString()
    })
  }

  async clearChapter(chapterId: string): Promise<ResultEnvelope<SaveReceipt>> {
    await this.delay(200)
    this.mockChapterContents.set(chapterId, '')
    return this.success({
      success: true,
      wordCount: 0,
      savedAt: new Date().toISOString()
    })
  }

  async createChapter(volumeId: string, name: string): Promise<ResultEnvelope<Chapter>> {
    await this.delay(150)
    const chapters = this.mockChapters.get(volumeId) || []
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
    for (const [volId, chapters] of this.mockChapters) {
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
    for (const [volId, chapters] of this.mockChapters) {
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
    const chapters = this.mockChapters.get(volumeId) || []
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
    if (starmapId === 'starmap-001') {
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
      projectId: 'proj-001',
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
    return this.success({ ...this.mockSettings })
  }

  async saveLocalSettings(settings: LocalSettings): Promise<ResultEnvelope<boolean>> {
    await this.delay(200)
    this.mockSettings = { ...settings }
    return this.success(true)
  }

  async getSyncableSettings(): Promise<ResultEnvelope<SyncableSettings>> {
    await this.delay(100)
    return this.success({
      fontSize: this.mockSettings.fontSize,
      lineHeight: this.mockSettings.lineHeight,
      fontFamily: this.mockSettings.fontFamily,
      theme: this.mockSettings.theme,
      autoIndent: this.mockSettings.autoIndent,
      showWordCount: this.mockSettings.showWordCount,
      showLineNumbers: this.mockSettings.showLineNumbers,
      wordWrap: this.mockSettings.wordWrap,
      uiLanguage: this.mockSettings.uiLanguage
    })
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
    return this.success({
      ...this.mockSettings,
      isSynced: false
    })
  }

  // Sync methods
  async loadSyncConfig(): Promise<ResultEnvelope<SyncConfig>> {
    await this.delay(100)
    return this.success({ ...this.mockSyncConfig })
  }

  async saveSyncConfig(config: SyncConfig): Promise<ResultEnvelope<boolean>> {
    await this.delay(200)
    this.mockSyncConfig = { ...config }
    return this.success(true)
  }

  async syncDryRun(): Promise<ResultEnvelope<SyncReport>> {
    await this.delay(300)
    return this.success({
      filesToAdd: [],
      filesToModify: ['projects/proj-001/volumes/vol-001/chapters/ch-001.md'],
      filesToDelete: [],
      conflicts: [],
      warnings: [],
      estimatedDuration: 5
    })
  }

  async syncDiagnostics(): Promise<ResultEnvelope<SyncDiagnostics>> {
    await this.delay(200)
    return this.success({
      isGitRepository: true,
      hasRemote: true,
      remoteUrl: 'https://github.com/example/writer-data.git',
      branch: 'main',
      isClean: false,
      lastCommit: 'abc123',
      lastCommitDate: '2026-06-14T00:00:00Z',
      errors: [],
      warnings: ['有 1 个未提交的更改']
    })
  }

  async performSync(): Promise<ResultEnvelope<SyncResult>> {
    await this.delay(1000)
    return this.success({
      success: true,
      filesAdded: 0,
      filesModified: 1,
      filesDeleted: 0,
      conflicts: [],
      duration: 1200
    })
  }

  // Stats methods
  async getWritingStats(): Promise<ResultEnvelope<WritingStats>> {
    await this.delay(150)
    return this.success({
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
        { date: '2026-06-14', charsAdded: 1200, durationSeconds: 2700, sessionsCount: 3 },
        { date: '2026-06-13', charsAdded: 2500, durationSeconds: 5400, sessionsCount: 5 },
        { date: '2026-06-12', charsAdded: 1800, durationSeconds: 3600, sessionsCount: 4 },
        { date: '2026-06-11', charsAdded: 3200, durationSeconds: 7200, sessionsCount: 6 },
        { date: '2026-06-10', charsAdded: 800, durationSeconds: 1800, sessionsCount: 2 },
        { date: '2026-06-09', charsAdded: 2100, durationSeconds: 4500, sessionsCount: 4 },
        { date: '2026-06-08', charsAdded: 1500, durationSeconds: 3000, sessionsCount: 3 }
      ]
    })
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
    return this.success([
      {
        projectId: 'proj-001',
        projectTitle: '示例小说',
        chapterId: 'ch-001',
        chapterTitle: '第一章 初遇',
        editedAt: '2026-06-14T08:30:00Z',
        wordCount: 3200
      },
      {
        projectId: 'proj-001',
        projectTitle: '示例小说',
        chapterId: 'ch-004',
        chapterTitle: '第四章 暴风雨',
        editedAt: '2026-06-13T18:45:00Z',
        wordCount: 4100
      },
      {
        projectId: 'proj-002',
        projectTitle: '随笔集',
        chapterId: 'ch-006',
        chapterTitle: '樱花时节',
        editedAt: '2026-06-12T09:15:00Z',
        wordCount: 1800
      }
    ])
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
