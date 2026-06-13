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
  ResultEnvelope,
  ProjectTree,
  WritingStats
} from '../model/CoreDtos'

class WriterCoreBridge {
  private static instance: WriterCoreBridge
  private currentWorkspacePath: string = '/mock/workspace'
  private mockProjects: Project[] = [
    {
      id: 'proj-001',
      name: '示例小说',
      volumeCount: 2,
      chapterCount: 5,
      createdAt: '2026-01-15T10:00:00Z',
      updatedAt: '2026-06-14T08:30:00Z'
    },
    {
      id: 'proj-002',
      name: '随笔集',
      volumeCount: 1,
      chapterCount: 3,
      createdAt: '2026-03-20T14:00:00Z',
      updatedAt: '2026-06-13T20:15:00Z'
    }
  ]

  private mockVolumes: Map<string, Volume[]> = new Map([
    ['proj-001', [
      { id: 'vol-001', projectId: 'proj-001', name: '第一卷 启程', order: 1, chapterCount: 3 },
      { id: 'vol-002', projectId: 'proj-001', name: '第二卷 风暴', order: 2, chapterCount: 2 }
    ]],
    ['proj-002', [
      { id: 'vol-003', projectId: 'proj-002', name: '春日随笔', order: 1, chapterCount: 3 }
    ]]
  ])

  private mockChapters: Map<string, Chapter[]> = new Map([
    ['vol-001', [
      { id: 'ch-001', volumeId: 'vol-001', name: '第一章 初遇', wordCount: 3200, order: 1, updatedAt: '2026-06-14T08:30:00Z' },
      { id: 'ch-002', volumeId: 'vol-001', name: '第二章 出发', wordCount: 2800, order: 2, updatedAt: '2026-06-13T15:20:00Z' },
      { id: 'ch-003', volumeId: 'vol-001', name: '第三章 旅途', wordCount: 3500, order: 3, updatedAt: '2026-06-12T22:10:00Z' }
    ]],
    ['vol-002', [
      { id: 'ch-004', volumeId: 'vol-002', name: '第四章 暴风雨', wordCount: 4100, order: 1, updatedAt: '2026-06-11T18:45:00Z' },
      { id: 'ch-005', volumeId: 'vol-002', name: '第五章 新生', wordCount: 2900, order: 2, updatedAt: '2026-06-10T12:30:00Z' }
    ]],
    ['vol-003', [
      { id: 'ch-006', volumeId: 'vol-003', name: '樱花时节', wordCount: 1800, order: 1, updatedAt: '2026-06-09T09:15:00Z' },
      { id: 'ch-007', volumeId: 'vol-003', name: '雨后', wordCount: 2200, order: 2, updatedAt: '2026-06-08T16:40:00Z' },
      { id: 'ch-008', volumeId: 'vol-003', name: '夏日蝉鸣', wordCount: 1500, order: 3, updatedAt: '2026-06-07T11:20:00Z' }
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
      updatedAt: '2026-06-14T08:00:00Z'
    },
    {
      id: 'starmap-002',
      title: '剧情线索',
      description: '故事主线与支线发展',
      nodeCount: 15,
      edgeCount: 20,
      projectId: 'proj-001',
      createdAt: '2026-03-05T14:30:00Z',
      updatedAt: '2026-06-13T16:45:00Z'
    }
  ]

  private mockStarMapGraph: StarMapGraph = {
    id: 'starmap-001',
    title: '人物关系图',
    description: '主要角色之间的关系网络',
    nodes: [
      { id: 'node-001', label: '主角', x: 400, y: 300, type: 'character', color: '#4A90D9' },
      { id: 'node-002', label: '伙伴A', x: 250, y: 200, type: 'character', color: '#7B68EE' },
      { id: 'node-003', label: '伙伴B', x: 550, y: 200, type: 'character', color: '#7B68EE' },
      { id: 'node-004', label: '导师', x: 300, y: 450, type: 'character', color: '#2ECC71' },
      { id: 'node-005', label: '对手', x: 500, y: 450, type: 'character', color: '#E74C3C' },
      { id: 'node-006', label: '故乡', x: 150, y: 350, type: 'location', color: '#F39C12' },
      { id: 'node-007', label: '王城', x: 650, y: 350, type: 'location', color: '#F39C12' },
      { id: 'node-008', label: '启程', x: 200, y: 100, type: 'event', color: '#9B59B6' }
    ],
    edges: [
      { id: 'edge-001', sourceId: 'node-001', targetId: 'node-002', label: '挚友', type: 'relation' },
      { id: 'edge-002', sourceId: 'node-001', targetId: 'node-003', label: '同伴', type: 'relation' },
      { id: 'edge-003', sourceId: 'node-001', targetId: 'node-004', label: '师徒', type: 'relation' },
      { id: 'edge-004', sourceId: 'node-001', targetId: 'node-005', label: '宿敌', type: 'relation' },
      { id: 'edge-005', sourceId: 'node-001', targetId: 'node-006', label: '出身地', type: 'relation' },
      { id: 'edge-006', sourceId: 'node-001', targetId: 'node-008', label: '经历', type: 'timeline' },
      { id: 'edge-007', sourceId: 'node-002', targetId: 'node-003', label: '相识', type: 'relation' },
      { id: 'edge-008', sourceId: 'node-004', targetId: 'node-005', label: '旧识', type: 'relation' },
      { id: 'edge-009', sourceId: 'node-005', targetId: 'node-007', label: '盘踞', type: 'relation' },
      { id: 'edge-010', sourceId: 'node-008', targetId: 'node-006', label: '离开', type: 'timeline' },
      { id: 'edge-011', sourceId: 'node-002', targetId: 'node-004', label: '同门', type: 'relation' },
      { id: 'edge-012', sourceId: 'node-003', targetId: 'node-007', label: '目的地', type: 'dependency' }
    ],
    embeds: [],
    links: []
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
    lastWorkspacePath: '/mock/workspace'
  }

  static getInstance(): WriterCoreBridge {
    if (!WriterCoreBridge.instance) {
      WriterCoreBridge.instance = new WriterCoreBridge()
    }
    return WriterCoreBridge.instance
  }

  private delay(ms: number): Promise<void> {
    return new Promise(resolve => setTimeout(resolve, ms))
  }

  // Workspace methods
  async listWorkspaces(): Promise<ResultEnvelope<Workspace[]>> {
    await this.delay(100)
    return {
      success: true,
      data: [
        { path: '/mock/workspace', name: '我的写作空间', isValid: true }
      ]
    }
  }

  async openWorkspace(path: string): Promise<ResultEnvelope<WorkspaceState>> {
    await this.delay(200)
    this.currentWorkspacePath = path
    return {
      success: true,
      data: {
        workspace: { path, name: '我的写作空间', isValid: true },
        projects: this.mockProjects,
        recentEdits: [
          {
            projectId: 'proj-001',
            projectTitle: '示例小说',
            chapterId: 'ch-001',
            chapterTitle: '第一章 初遇',
            editedAt: '2026-06-14T08:30:00Z'
          }
        ]
      }
    }
  }

  // Project methods
  async listProjects(): Promise<ResultEnvelope<Project[]>> {
    await this.delay(100)
    return {
      success: true,
      data: this.mockProjects
    }
  }

  async getProjectTree(projectId: string): Promise<ResultEnvelope<ProjectTree>> {
    await this.delay(150)
    const project = this.mockProjects.find(p => p.id === projectId)
    if (!project) {
      return {
        success: false,
        errorCode: 'PROJECT_NOT_FOUND',
        userMessage: '项目不存在'
      }
    }

    const volumes = this.mockVolumes.get(projectId) || []
    const volumeTrees = volumes.map(vol => ({
      volume: vol,
      chapters: this.mockChapters.get(vol.id) || []
    }))

    return {
      success: true,
      data: { project, volumes: volumeTrees }
    }
  }

  async createProject(name: string): Promise<ResultEnvelope<Project>> {
    await this.delay(200)
    const newProject: Project = {
      id: `proj-${Date.now()}`,
      name,
      volumeCount: 0,
      chapterCount: 0,
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString()
    }
    this.mockProjects.push(newProject)
    this.mockVolumes.set(newProject.id, [])
    return { success: true, data: newProject }
  }

  async deleteProject(projectId: string): Promise<ResultEnvelope<boolean>> {
    await this.delay(150)
    const index = this.mockProjects.findIndex(p => p.id === projectId)
    if (index === -1) {
      return {
        success: false,
        errorCode: 'PROJECT_NOT_FOUND',
        userMessage: '项目不存在'
      }
    }
    this.mockProjects.splice(index, 1)
    this.mockVolumes.delete(projectId)
    return { success: true, data: true }
  }

  // Volume methods
  async listVolumes(projectId: string): Promise<ResultEnvelope<Volume[]>> {
    await this.delay(100)
    return {
      success: true,
      data: this.mockVolumes.get(projectId) || []
    }
  }

  async createVolume(projectId: string, name: string): Promise<ResultEnvelope<Volume>> {
    await this.delay(150)
    const volumes = this.mockVolumes.get(projectId) || []
    const newVolume: Volume = {
      id: `vol-${Date.now()}`,
      projectId,
      name,
      order: volumes.length + 1,
      chapterCount: 0
    }
    volumes.push(newVolume)
    this.mockVolumes.set(projectId, volumes)
    return { success: true, data: newVolume }
  }

  // Chapter methods
  async listChapters(volumeId: string): Promise<ResultEnvelope<Chapter[]>> {
    await this.delay(100)
    return {
      success: true,
      data: this.mockChapters.get(volumeId) || []
    }
  }

  async loadChapter(chapterId: string): Promise<ResultEnvelope<ChapterData>> {
    await this.delay(200)
    const content = this.mockChapterContents.get(chapterId)
    if (content === undefined) {
      // Find chapter info from mock data
      for (const [volId, chapters] of this.mockChapters) {
        const chapter = chapters.find(ch => ch.id === chapterId)
        if (chapter) {
          return {
            success: true,
            data: {
              id: chapter.id,
              title: chapter.name,
              content: `这是《${chapter.name}》的示例内容。\n\n在此处开始写作...\n\n鸿蒙端当前使用 mock 数据，后续将对接真实 Rust Core。`,
              wordCount: chapter.wordCount,
              volumeId: chapter.volumeId,
              projectId: 'proj-001',
              updatedAt: chapter.updatedAt
            }
          }
        }
      }
      return {
        success: false,
        errorCode: 'CHAPTER_NOT_FOUND',
        userMessage: '章节不存在'
      }
    }

    return {
      success: true,
      data: {
        id: chapterId,
        title: '第一章 初遇',
        content,
        wordCount: content.length,
        volumeId: 'vol-001',
        projectId: 'proj-001',
        updatedAt: new Date().toISOString()
      }
    }
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
        }
      }
    }

    this.mockChapterContents.set(chapterId, text)
    return {
      success: true,
      data: {
        success: true,
        wordCount: text.length,
        savedAt: new Date().toISOString()
      }
    }
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
      updatedAt: new Date().toISOString()
    }
    chapters.push(newChapter)
    this.mockChapters.set(volumeId, chapters)
    return { success: true, data: newChapter }
  }

  async deleteChapter(chapterId: string): Promise<ResultEnvelope<boolean>> {
    await this.delay(150)
    for (const [volId, chapters] of this.mockChapters) {
      const index = chapters.findIndex(ch => ch.id === chapterId)
      if (index !== -1) {
        chapters.splice(index, 1)
        this.mockChapterContents.delete(chapterId)
        return { success: true, data: true }
      }
    }
    return {
      success: false,
      errorCode: 'CHAPTER_NOT_FOUND',
      userMessage: '章节不存在'
    }
  }

  // StarMap methods
  async listStarMaps(): Promise<ResultEnvelope<StarMapMeta[]>> {
    await this.delay(100)
    return {
      success: true,
      data: this.mockStarMaps
    }
  }

  async getStarMapGraph(starmapId: string): Promise<ResultEnvelope<StarMapGraph>> {
    await this.delay(200)
    if (starmapId === 'starmap-001') {
      return {
        success: true,
        data: this.mockStarMapGraph
      }
    }
    return {
      success: false,
      errorCode: 'STARMAP_NOT_FOUND',
      userMessage: '星图不存在'
    }
  }

  // Settings methods
  async getLocalSettings(): Promise<ResultEnvelope<LocalSettings>> {
    await this.delay(100)
    return {
      success: true,
      data: { ...this.mockSettings }
    }
  }

  async saveLocalSettings(settings: LocalSettings): Promise<ResultEnvelope<boolean>> {
    await this.delay(200)
    this.mockSettings = { ...settings }
    return { success: true, data: true }
  }

  // Stats methods
  async getWritingStats(): Promise<ResultEnvelope<WritingStats>> {
    await this.delay(150)
    return {
      success: true,
      data: {
        totalChars: 25600,
        totalWords: 18200,
        totalChapters: 8,
        todayChars: 1200,
        todayDuration: 45,
        averageSpeed: 800
      }
    }
  }

  async calculateWordCount(text: string): Promise<number> {
    return text.replace(/\s/g, '').length
  }
}

export default WriterCoreBridge
