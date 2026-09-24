// ProjectWorkspaceViewModel — 作品工作区 ViewModel。
// 从 WorkspaceScreen.ets 抽离的 Core/NAPI 调用、async 加载、CRUD 操作和状态管理。
// ViewModel 是纯逻辑类，不绑定 ArkUI 装饰器；页面状态对象使用 V2 状态管理（@ObservedV2 + @Trace）。
// Core DTO / NAPI DTO 不绑定 ArkUI 装饰器，由 ViewModel 转成页面状态对象供 UI 消费。

import { IWriterCoreBridge } from '../../../corebridge/IWriterCoreBridge'
import { Project, VolumeTree, ChapterMeta, ProjectTree } from '../../../corebridge/dto/ProjectDtos'
import { getBridge } from '../../../app/AppContext'

// ─── 页面状态对象（V2 状态管理） ───
// Core DTO 不绑定 ArkUI 装饰器；这些页面状态对象是 UI 专用视图模型，
// 由 ViewModel 从 Core DTO 转换而来。

/** 章节行状态 — 对应窄屏卷卡展开后的每个章节行 */
@ObservedV2
export class ChapterRowState {
  @Trace chapterId: string = ''
  @Trace chapterTitle: string = ''
  @Trace wordCount: number = 0
  @Trace lastEditedTime: string = ''
  @Trace volumeId: string = ''
}

/** 卷卡状态 — 对应窄屏每个可展开卷卡 */
@ObservedV2
export class VolumeCardState {
  @Trace volumeId: string = ''
  @Trace volumeTitle: string = ''
  @Trace chapterCount: number = 0
  @Trace totalWordCount: number = 0
  @Trace chapters: ChapterRowState[] = []
}

// ─── ViewModel（纯逻辑类，不绑定 ArkUI 装饰器） ───

/**
 * ProjectWorkspaceViewModel — 作品工作区状态与业务逻辑管理器。
 *
 * 职责：
 * - 加载项目树（getProjectTree）
 * - 创建/删除卷和章节（createVolume / createChapter / deleteVolume / deleteChapter）
 * - 管理 expandedVolumes 展开状态
 * - 将 Core DTO（VolumeTree / ChapterMeta）转换为页面状态对象（VolumeCardState / ChapterRowState）
 * - 管理 loading / error 状态
 *
 * 不负责：
 * - UI 渲染、导航、AlertDialog、Menu 组件
 * - 资源字符串本地化（由 UI 层处理）
 */
export class ProjectWorkspaceViewModel {
  private bridge: IWriterCoreBridge = getBridge()
  private projectId: string = ''
  private project: Project | null = null
  private volumeTrees: VolumeTree[] = []
  private volumeCardStates: VolumeCardState[] = []
  private isLoadingState: boolean = true
  private errorMessageState: string = ''
  private expandedVolumes: Set<string> = new Set()

  // ─── 数据加载 ───

  async loadData(projectId: string): Promise<void> {
    this.projectId = projectId
    this.isLoadingState = true
    this.errorMessageState = ''

    try {
      const result = await this.bridge.getProjectTree(this.projectId)
      if (result.success && result.data) {
        this.project = result.data.project
        this.volumeTrees = result.data.volumes
        this.volumeCardStates = this.convertToCardStates(this.volumeTrees)
        // 默认展开第一个卷
        if (this.volumeCardStates.length > 0) {
          this.expandedVolumes.add(this.volumeCardStates[0].volumeId)
        }
      } else {
        this.errorMessageState = '加载项目失败'
      }
    } catch (_err) {
      this.errorMessageState = '加载项目时发生错误'
    } finally {
      this.isLoadingState = false
    }
  }

  async retry(): Promise<void> {
    await this.loadData(this.projectId)
  }

  // ─── 数据访问 ───

  getProject(): Project | null {
    return this.project
  }

  getVolumeTrees(): VolumeTree[] {
    return this.volumeTrees
  }

  getVolumeCardStates(): VolumeCardState[] {
    return this.volumeCardStates
  }

  isLoading(): boolean {
    return this.isLoadingState
  }

  getErrorMessage(): string {
    return this.errorMessageState
  }

  getProjectId(): string {
    return this.projectId
  }

  // ─── 展开状态管理 ───

  isVolumeExpanded(volumeId: string): boolean {
    return this.expandedVolumes.has(volumeId)
  }

  toggleVolume(volumeId: string): void {
    if (this.expandedVolumes.has(volumeId)) {
      this.expandedVolumes.delete(volumeId)
    } else {
      this.expandedVolumes.add(volumeId)
    }
  }

  getExpandedVolumes(): Set<string> {
    return this.expandedVolumes
  }

  // ─── CRUD 操作 ───

  async createVolume(): Promise<boolean> {
    try {
      const result = await this.bridge.createVolume(this.projectId, '新卷')
      if (result.success) {
        await this.loadData(this.projectId)
        return true
      }
    } catch (_err) {
      console.error('Failed to create volume:', _err)
    }
    return false
  }

  async createChapter(volumeId: string): Promise<boolean> {
    try {
      const result = await this.bridge.createChapter(volumeId, '新章节')
      if (result.success) {
        await this.loadData(this.projectId)
        return true
      }
    } catch (_err) {
      console.error('Failed to create chapter:', _err)
    }
    return false
  }

  async deleteVolume(volumeId: string): Promise<boolean> {
    try {
      const result = await this.bridge.deleteVolume(this.projectId, volumeId)
      if (result.success) {
        await this.loadData(this.projectId)
        return true
      }
    } catch (_err) {
      console.error('Failed to delete volume:', _err)
    }
    return false
  }

  async deleteChapter(chapterId: string): Promise<boolean> {
    try {
      const result = await this.bridge.deleteChapter(chapterId)
      if (result.success) {
        await this.loadData(this.projectId)
        return true
      }
    } catch (_err) {
      console.error('Failed to delete chapter:', _err)
    }
    return false
  }

  // ─── Core DTO → 页面状态对象转换 ───

  private convertToCardStates(volumeTrees: VolumeTree[]): VolumeCardState[] {
    const states: VolumeCardState[] = []
    for (const vt of volumeTrees) {
      const cardState: VolumeCardState = new VolumeCardState()
      cardState.volumeId = vt.volume.id
      cardState.volumeTitle = vt.volume.title
      cardState.chapterCount = vt.chapters.length

      let totalWords: number = 0
      const chapterStates: ChapterRowState[] = []
      for (const ch of vt.chapters) {
        const chState: ChapterRowState = new ChapterRowState()
        chState.chapterId = ch.id
        chState.chapterTitle = ch.title
        chState.wordCount = ch.wordCount
        chState.lastEditedTime = ch.updatedAt
        chState.volumeId = vt.volume.id
        totalWords += ch.wordCount
        chapterStates.push(chState)
      }
      cardState.totalWordCount = totalWords
      cardState.chapters = chapterStates
      states.push(cardState)
    }
    return states
  }
}
