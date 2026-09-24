// ProjectWorkspaceViewModel — 作品工作区 ViewModel。
// Issue #760 评论 5821290182 问题5+7：
// - ViewModel 改为 @ObservedV2，所有页面观察的状态字段加 @Trace
// - 加载 ScreenPolicy，暴露排序后的 Context actions 供 UI 渲染菜单
// - Rename 接 bridge.renameVolume/renameChapter，Delete 按 requiresConfirmation 决定是否弹确认框
// - 页面不再维护 @State 副本，直接观察 ViewModel 的 @Trace 字段

import { IWriterCoreBridge } from '../../../corebridge/IWriterCoreBridge'
import { Project, VolumeTree, ChapterMeta } from '../../../corebridge/dto/ProjectDtos'
import { ScreenPolicy, ScreenRole, ActionSlot, ActionTarget, ActionRegion } from '../../../corebridge/dto/PlatformDtos'
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

/** 重命名目标类型 */
export type RenameTargetType = 'volume' | 'chapter'

/** 重命名目标 — 包含类型、ID 和当前名称 */
@ObservedV2
export class RenameTarget {
  @Trace type: RenameTargetType = 'volume'
  @Trace id: string = ''
  @Trace currentName: string = ''
}

/** 删除目标 — 包含类型、ID、名称和是否需要确认 */
@ObservedV2
export class DeleteTarget {
  @Trace type: RenameTargetType = 'volume'
  @Trace id: string = ''
  @Trace name: string = ''
  @Trace requiresConfirmation: boolean = true
}

// ─── ViewModel（@ObservedV2，页面直接观察 @Trace 字段） ───

/**
 * ProjectWorkspaceViewModel — 作品工作区状态与业务逻辑管理器。
 *
 * 职责：
 * - 加载项目树（getProjectTree）和页面策略（resolveScreenPolicy）
 * - 创建/删除/重命名卷和章节
 * - 管理 expandedVolumes 展开状态
 * - 将 Core DTO（VolumeTree / ChapterMeta）转换为页面状态对象（VolumeCardState / ChapterRowState）
 * - 管理 loading / error / rename / delete / sync 状态
 * - 暴露排序后的 Context actions 供 UI 按 action slot 渲染菜单
 *
 * 不负责：
 * - UI 渲染、导航、AlertDialog、Menu 组件
 * - 资源字符串本地化（由 UI 层处理）
 */
@ObservedV2
export class ProjectWorkspaceViewModel {
  private bridge: IWriterCoreBridge = getBridge()

  // ─── 页面观察的状态字段（@Trace） ───
  @Trace project: Project | null = null
  @Trace volumeTrees: VolumeTree[] = []
  @Trace volumeCardStates: VolumeCardState[] = []
  @Trace isLoadingState: boolean = true
  @Trace errorMessageState: string = ''
  @Trace expandedVolumes: Set<string> = new Set()
  @Trace screenPolicy: ScreenPolicy | null = null
  @Trace renameTarget: RenameTarget | null = null
  @Trace renameInput: string = ''
  @Trace deleteTarget: DeleteTarget | null = null
  @Trace isSyncing: boolean = false
  @Trace syncMessage: string = ''
  @Trace projectId: string = ''
  @Trace projectName: string = ''
  // 当前更多菜单上下文 — 用于 @Builder 无参数方法访问正确的 volumeId/chapterId
  @Trace currentMenuVolumeId: string = ''
  @Trace currentMenuChapterId: string = ''

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
          const newSet = new Set<string>()
          newSet.add(this.volumeCardStates[0].volumeId)
          this.expandedVolumes = newSet
        }
      } else {
        this.errorMessageState = '加载项目失败'
      }

      // 加载页面策略（ScreenPolicy）
      const policyResult = await this.bridge.resolveScreenPolicy(ScreenRole.ProjectWorkspace)
      if (policyResult.success && policyResult.data) {
        this.screenPolicy = policyResult.data
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

  // ─── ScreenPolicy / Context Actions ───

  /** 获取指定目标的排序后 Context actions（供 UI 按 action slot 渲染菜单） */
  getContextActions(target: ActionTarget): ActionSlot[] {
    if (!this.screenPolicy) {
      return []
    }
    const actions: ActionSlot[] = []
    for (const slot of this.screenPolicy.actionSlots) {
      if (slot.region === ActionRegion.Context && slot.target === target) {
        actions.push(slot)
      }
    }
    actions.sort((a: ActionSlot, b: ActionSlot) => a.order - b.order)
    return actions
  }

  /** 查找指定目标和角色的 ActionSlot（用于获取 requiresConfirmation 等属性） */
  findContextAction(target: ActionTarget, role: string): ActionSlot | null {
    const actions = this.getContextActions(target)
    for (const slot of actions) {
      if (slot.role === role) {
        return slot
      }
    }
    return null
  }

  // ─── 展开状态管理 ───

  isVolumeExpanded(volumeId: string): boolean {
    return this.expandedVolumes.has(volumeId)
  }

  toggleVolume(volumeId: string): void {
    const newSet = new Set<string>(this.expandedVolumes)
    if (newSet.has(volumeId)) {
      newSet.delete(volumeId)
    } else {
      newSet.add(volumeId)
    }
    this.expandedVolumes = newSet
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

  async renameVolume(volumeId: string, newName: string): Promise<boolean> {
    try {
      const result = await this.bridge.renameVolume(this.projectId, volumeId, newName)
      if (result.success) {
        await this.loadData(this.projectId)
        return true
      }
    } catch (_err) {
      console.error('Failed to rename volume:', _err)
    }
    return false
  }

  async renameChapter(chapterId: string, newName: string): Promise<boolean> {
    try {
      const result = await this.bridge.renameChapter(chapterId, newName)
      if (result.success) {
        await this.loadData(this.projectId)
        return true
      }
    } catch (_err) {
      console.error('Failed to rename chapter:', _err)
    }
    return false
  }

  // ─── 卷/章节排序（MoveEarlier / MoveLater） ───

  /** 将指定卷上移一位（与前一个卷交换顺序） */
  async moveVolumeEarlier(volumeId: string): Promise<boolean> {
    const ids: string[] = []
    for (const card of this.volumeCardStates) {
      ids.push(card.volumeId)
    }
    const index = ids.indexOf(volumeId)
    if (index <= 0) {
      return false
    }
    // 交换 index 和 index-1
    const tmp = ids[index]
    ids[index] = ids[index - 1]
    ids[index - 1] = tmp
    try {
      const result = await this.bridge.reorderVolumes(this.projectId, ids)
      if (result.success) {
        await this.loadData(this.projectId)
        return true
      }
    } catch (_err) {
      console.error('Failed to move volume earlier:', _err)
    }
    return false
  }

  /** 将指定卷下移一位（与后一个卷交换顺序） */
  async moveVolumeLater(volumeId: string): Promise<boolean> {
    const ids: string[] = []
    for (const card of this.volumeCardStates) {
      ids.push(card.volumeId)
    }
    const index = ids.indexOf(volumeId)
    if (index < 0 || index >= ids.length - 1) {
      return false
    }
    // 交换 index 和 index+1
    const tmp = ids[index]
    ids[index] = ids[index + 1]
    ids[index + 1] = tmp
    try {
      const result = await this.bridge.reorderVolumes(this.projectId, ids)
      if (result.success) {
        await this.loadData(this.projectId)
        return true
      }
    } catch (_err) {
      console.error('Failed to move volume later:', _err)
    }
    return false
  }

  /** 将指定章节上移一位（在同一卷内与前一个章节交换顺序） */
  async moveChapterEarlier(chapterId: string): Promise<boolean> {
    // 找到 chapterId 所在的 volume 及其在 chapters 中的位置
    let volumeId: string = ''
    let chapterIds: string[] = []
    let chapterIndex: number = -1
    for (const card of this.volumeCardStates) {
      const idx = card.chapters.findIndex((ch: ChapterRowState) => ch.chapterId === chapterId)
      if (idx >= 0) {
        volumeId = card.volumeId
        chapterIds = card.chapters.map((ch: ChapterRowState) => ch.chapterId)
        chapterIndex = idx
        break
      }
    }
    if (chapterIndex <= 0) {
      return false
    }
    // 交换 chapterIndex 和 chapterIndex-1
    const tmp = chapterIds[chapterIndex]
    chapterIds[chapterIndex] = chapterIds[chapterIndex - 1]
    chapterIds[chapterIndex - 1] = tmp
    try {
      const result = await this.bridge.reorderChapters(volumeId, chapterIds)
      if (result.success) {
        await this.loadData(this.projectId)
        return true
      }
    } catch (_err) {
      console.error('Failed to move chapter earlier:', _err)
    }
    return false
  }

  /** 将指定章节下移一位（在同一卷内与后一个章节交换顺序） */
  async moveChapterLater(chapterId: string): Promise<boolean> {
    // 找到 chapterId 所在的 volume 及其在 chapters 中的位置
    let volumeId: string = ''
    let chapterIds: string[] = []
    let chapterIndex: number = -1
    for (const card of this.volumeCardStates) {
      const idx = card.chapters.findIndex((ch: ChapterRowState) => ch.chapterId === chapterId)
      if (idx >= 0) {
        volumeId = card.volumeId
        chapterIds = card.chapters.map((ch: ChapterRowState) => ch.chapterId)
        chapterIndex = idx
        break
      }
    }
    if (chapterIndex < 0 || chapterIndex >= chapterIds.length - 1) {
      return false
    }
    // 交换 chapterIndex 和 chapterIndex+1
    const tmp = chapterIds[chapterIndex]
    chapterIds[chapterIndex] = chapterIds[chapterIndex + 1]
    chapterIds[chapterIndex + 1] = tmp
    try {
      const result = await this.bridge.reorderChapters(volumeId, chapterIds)
      if (result.success) {
        await this.loadData(this.projectId)
        return true
      }
    } catch (_err) {
      console.error('Failed to move chapter later:', _err)
    }
    return false
  }

  // ─── 重命名对话框状态管理 ───

  /** 开始重命名卷 — 设置 renameTarget 和 renameInput */
  startRenameVolume(volumeId: string): void {
    let currentName = ''
    for (const card of this.volumeCardStates) {
      if (card.volumeId === volumeId) {
        currentName = card.volumeTitle
        break
      }
    }
    const target = new RenameTarget()
    target.type = 'volume'
    target.id = volumeId
    target.currentName = currentName
    this.renameTarget = target
    this.renameInput = currentName
  }

  /** 开始重命名章节 — 设置 renameTarget 和 renameInput */
  startRenameChapter(chapterId: string): void {
    let currentName = ''
    for (const card of this.volumeCardStates) {
      for (const ch of card.chapters) {
        if (ch.chapterId === chapterId) {
          currentName = ch.chapterTitle
          break
        }
      }
    }
    const target = new RenameTarget()
    target.type = 'chapter'
    target.id = chapterId
    target.currentName = currentName
    this.renameTarget = target
    this.renameInput = currentName
  }

  /** 确认重命名 — 调用 bridge 方法并清除对话框状态 */
  async confirmRename(): Promise<void> {
    if (this.renameTarget && this.renameInput.trim().length > 0) {
      if (this.renameTarget.type === 'volume') {
        await this.renameVolume(this.renameTarget.id, this.renameInput.trim())
      } else {
        await this.renameChapter(this.renameTarget.id, this.renameInput.trim())
      }
    }
    this.renameTarget = null
    this.renameInput = ''
  }

  /** 取消重命名 — 清除对话框状态 */
  cancelRename(): void {
    this.renameTarget = null
    this.renameInput = ''
  }

  // ─── 删除确认状态管理 ───

  /** 开始删除卷 — 根据 requiresConfirmation 决定是否弹确认框 */
  startDeleteVolume(volumeId: string, requiresConfirmation: boolean): void {
    if (!requiresConfirmation) {
      // 不需要确认，直接删除
      this.deleteVolume(volumeId)
      return
    }
    // 需要确认，设置 deleteTarget 触发确认对话框
    let name = ''
    for (const card of this.volumeCardStates) {
      if (card.volumeId === volumeId) {
        name = card.volumeTitle
        break
      }
    }
    const target = new DeleteTarget()
    target.type = 'volume'
    target.id = volumeId
    target.name = name
    target.requiresConfirmation = true
    this.deleteTarget = target
  }

  /** 开始删除章节 — 根据 requiresConfirmation 决定是否弹确认框 */
  startDeleteChapter(chapterId: string, requiresConfirmation: boolean): void {
    if (!requiresConfirmation) {
      // 不需要确认，直接删除
      this.deleteChapter(chapterId)
      return
    }
    // 需要确认，设置 deleteTarget 触发确认对话框
    let name = ''
    for (const card of this.volumeCardStates) {
      for (const ch of card.chapters) {
        if (ch.chapterId === chapterId) {
          name = ch.chapterTitle
          break
        }
      }
    }
    const target = new DeleteTarget()
    target.type = 'chapter'
    target.id = chapterId
    target.name = name
    target.requiresConfirmation = true
    this.deleteTarget = target
  }

  /** 确认删除 — 调用 bridge 方法并清除对话框状态 */
  async confirmDelete(): Promise<void> {
    if (this.deleteTarget) {
      if (this.deleteTarget.type === 'volume') {
        await this.deleteVolume(this.deleteTarget.id)
      } else {
        await this.deleteChapter(this.deleteTarget.id)
      }
    }
    this.deleteTarget = null
  }

  /** 取消删除 — 清除对话框状态 */
  cancelDelete(): void {
    this.deleteTarget = null
  }

  // ─── 同步 ───

  async performSync(): Promise<void> {
    this.isSyncing = true
    this.syncMessage = ''
    try {
      const result = await this.bridge.performSync()
      if (result.success) {
        this.syncMessage = '同步完成'
      } else {
        this.syncMessage = '同步失败'
      }
    } catch (_err) {
      this.syncMessage = '同步时发生错误'
    } finally {
      this.isSyncing = false
    }
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
