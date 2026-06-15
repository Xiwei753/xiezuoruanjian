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
  ErrorCode
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
    } as Project,
    {
      id: 'proj-002',
      name: '随笔集',
      volumeCount: 1,
      chapterCount: 3,
      totalWordCount: 5500,
      createdAt: '2026-03-20T14:00:00Z',
      updatedAt: '2026-06-13T20:15:00Z',
      description: '日常生活感悟'
    } as Project
  ]

  private mockVolumes: Map<string, Volume[]> = new Map<string, Volume[]>([
    ['proj-001', [
      { id: 'vol-001', projectId: 'proj-001', name: '第一卷 启程', order: 1, chapterCount: 3, totalWordCount: 9500 } as Volume,
      { id: 'vol-002', projectId: 'proj-001', name: '第二卷 风暴', order: 2, chapterCount: 2, totalWordCount: 7000 } as Volume
    ] as Volume[]],
    ['proj-002', [
      { id: 'vol-003', projectId: 'proj-002', name: '春日随笔', order: 1, chapterCount: 3, totalWordCount: 5500 } as Volume
    ] as Volume[]]
  ])

  private mockChapters: Map<string, Chapter[]> = new Map<string, Chapter[]>([
    ['vol-001', [
      { id: 'ch-001', volumeId: 'vol-001', name: '第一章 初遇', wordCount: 3200, order: 1, updatedAt: '2026-06-14T08:30:00Z', status: 'draft' } as Chapter,
      { id: 'ch-002', volumeId: 'vol-001', name: '第二章 出发', wordCount: 2800, order: 2, updatedAt: '2026-06-13T15:20:00Z', status: 'draft' } as Chapter,
      { id: 'ch-003', volumeId: 'vol-001', name: '第三章 旅途', wordCount: 3500, order: 3, updatedAt: '2026-06-12T22:10:00Z', status: 'draft' } as Chapter
    ] as Chapter[]],
    ['vol-002', [
      { id: 'ch-004', volumeId: 'vol-002', name: '第四章 暴风雨', wordCount: 4100, order: 1, updatedAt: '2026-06-11T18:45:00Z', status: 'draft' } as Chapter,
      { id: 'ch-005', volumeId: 'vol-002', name: '第五章 新生', wordCount: 2900, order: 2, updatedAt: '2026-06-10T12:30:00Z', status: 'draft' } as Chapter
    ] as Chapter[]],
    ['vol-003', [
      { id: 'ch-006', volumeId: 'vol-003', name: '樱花时节', wordCount: 1800, order: 1, updatedAt: '2026-06-09T09:15:00Z', status: 'draft' } as Chapter,
      { id: 'ch-007', volumeId: 'vol-003', name: '雨后', wordCount: 2200, order: 2, updatedAt: '2026-06-08T16:40:00Z', status: 'draft' } as Chapter,
      { id: 'ch-008', volumeId: 'vol-003', name: '夏日蝉鸣', wordCount: 1500, order: 3, updatedAt: '2026-06-07T11:20:00Z', status: 'draft' } as Chapter
    ] as Chapter[]]
  ])

  private mockChapterContents: Map<string, string> = new Map<string, string>([
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
    } as StarMapMeta,
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
    } as StarMapMeta
  ]

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
    id: 'starmap-001',
    title: '人物关系图',
    description: '主要角色之间的关系网络',
    nodes: [
      { id: 'node-001', label: '主角', x: 400, y: 300, type: 'character', color: '#4A90D9', description: '故事的主人公' } as StarMapNode,
      { id: 'node-002', label: '伙伴A', x: 250, y: 200, type: 'character', color: '#7B68EE', description: '主角的挚友' } as StarMapNode,
      { id: 'node-003', label: '伙伴B', x: 550, y: 200, type: 'character', color: '#7B68EE', description: '旅途中的同伴' } as StarMapNode,
      { id: 'node-004', label: '导师', x: 300, y: 450, type: 'character', color: '#2ECC71', description: '传授技艺的长者' } as StarMapNode,
      { id: 'node-005', label: '对手', x: 500, y: 450, type: 'character', color: '#E74C3C', description: '宿命之敌' } as StarMapNode,
      { id: 'node-006', label: '故乡', x: 150, y: 350, type: 'location', color: '#F39C12', description: '主角出生的地方' } as StarMapNode,
      { id: 'node-007', label: '王城', x: 650, y: 350, type: 'location', color: '#F39C12', description: '王国的中心' } as StarMapNode,
      { id: 'node-008', label: '启程', x: 200, y: 100, type: 'event', color: '#9B59B6', description: '冒险的开始' } as StarMapNode
    ],
    edges: [
      { id: 'edge-001', sourceId: 'node-001', targetId: 'node-002', label: '挚友', type: 'relation', style: 'solid' } as StarMapEdge,
      { id: 'edge-002', sourceId: 'node-001', targetId: 'node-003', label: '同伴', type: 'relation', style: 'solid' } as StarMapEdge,
      { id: 'edge-003', sourceId: 'node-001', targetId: 'node-004', label: '师徒', type: 'relation', style: 'solid' } as StarMapEdge,
      { id: 'edge-004', sourceId: 'node-001', targetId: 'node-005', label: '宿敌', type: 'conflict', style: 'dashed' } as StarMapEdge,
      { id: 'edge-005', sourceId: 'node-001', targetId: 'node-006', label: '出身地', type: 'relation', style: 'solid' } as StarMapEdge,
      { id: 'edge-006', sourceId: 'node-001', targetId: 'node-008', label: '经历', type: 'timeline', style: 'solid' } as StarMapEdge,
      { id: 'edge-007', sourceId: 'node-002', targetId: 'node-003', label: '相识', type: 'relation', style: 'solid' } as StarMapEdge,
      { id: 'edge-008', sourceId: 'node-004', targetId: 'node-005', label: '旧识', type: 'relation', style: 'dotted' } as StarMapEdge,
      { id: 'edge-009', sourceId: 'node-005', targetId: 'node-007', label: '盘踞', type: 'relation', style: 'solid' } as StarMapEdge,
      { id: 'edge-010', sourceId: 'node-008', targetId: 'node-006', label: '离开', type: 'timeline', style: 'solid' } as StarMapEdge,
      { id: 'edge-011', sourceId: 'node-002', targetId: 'node-004', label: '同门', type: 'relation', style: 'solid' } as StarMapEdge,
      { id: 'edge-012', sourceId: 'node-003', targetId: 'node-007', label: '目的地', type: 'dependency', style: 'solid' } as StarMapEdge
    ],
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
    lastWorkspacePath: '/mock/workspace',
    lastProjectId: 'proj-001',
    lastChapterId: 'ch-001'
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
      recentEdits: [
        {
          projectId: 'proj-001',
          projectTitle: '示例小说',
          chapterId: 'ch-001',
          chapterTitle: '第一章 初遇',
          editedAt: '2026-06-14T08:30:00Z',
          wordCount: 3200
        } as RecentEdit
      ] as RecentEdit[],
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
    return this.success([
      {
        projectId: 'proj-001',
        projectTitle: '示例小说',
        chapterId: 'ch-001',
        chapterTitle: '第一章 初遇',
        editedAt: '2026-06-14T08:30:00Z',
        wordCount: 3200
      } as RecentEdit,
      {
        projectId: 'proj-001',
        projectTitle: '示例小说',
        chapterId: 'ch-004',
        chapterTitle: '第四章 暴风雨',
        editedAt: '2026-06-13T18:45:00Z',
        wordCount: 4100
      } as RecentEdit,
      {
        projectId: 'proj-002',
        projectTitle: '随笔集',
        chapterId: 'ch-006',
        chapterTitle: '樱花时节',
        editedAt: '2026-06-12T09:15:00Z',
        wordCount: 1800
      } as RecentEdit
    ] as RecentEdit[])
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
