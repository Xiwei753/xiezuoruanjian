// SyncConflictViewModel — 设置页同步冲突面板的状态与数据入口（Issue #926）。
//
// 唯一数据来源是 Core 的全局冲突列表（NativeSyncBridge.listAllSyncConflicts）：
// UI 不维护第二份"假冲突列表"，每次解决后都重新从 Core 拉取剩余冲突。
// 预览只在用户展开某项时按需加载；操作失败时保留 Core 返回的结构化 errorCode，
// 不伪造成功。
//
// 活跃章节协调不在这里做：面板里的"重新同步"走 SyncOperationController.performSync()，
// 由 #923 的 ActiveWritingSessionRegistry.withSyncBarrier 统一保存活跃正文并按落盘身份回填，
// 本文件不复制第二套编辑器会话逻辑。

import hilog from '@ohos.hilog'
import { getBridge } from '../../../app/AppContext'
import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'
import { ChapterSaveReceipt } from '../../../corebridge/dto/ProjectDtos'
import { AllSyncConflictEntry, SyncConflictPreview } from '../../../corebridge/dto/SyncDtos'
import { NativeSyncBridge } from '../../../corebridge/native/NativeSyncBridge'
import { NativeCoreModule } from '../../../corebridge/native/NativeCoreModule'
import { ActiveWritingSessionRegistry, SyncBarrierScope, SyncPreflightError, SYNC_PREFLIGHT_SAVE_FAILED } from '../../editor/session/ActiveWritingSessionRegistry'

const TAG = 'SyncConflictViewModel'
const DOMAIN = 0x0000

/** 冲突解决动作线格式，与 Core FFI 的 decision 参数一致。 */
export type SyncConflictDecision = 'keep_local' | 'take_remote' | 'mark_merged'

/** 兼容 `string` / `string | null` 两种 DTO 字段类型：只做展示，不改写数据。 */
function safeDisplayText(value: string | null): string {
  return value === null ? '' : value
}

/** 双方内容是否存在（参数保持 `string | null`，兼容 DTO 字段可空/不可空两种声明）。 */
function hasTextContent(value: string | null): boolean {
  return value !== null
}

/** Core 冲突条目的稳定身份；同一个冲突在多次 Core 拉取之间保持不变。 */
function conflictKeyOfEntry(entry: AllSyncConflictEntry): string {
  return `${entry.targetKind}|${safeDisplayText(entry.projectId)}|${entry.path}`
}

/**
 * 从章节冲突路径解析 chapterId。
 *
 * 章节正文落盘路径固定为 `volumes/<volumeId>/chapters/<chapterId>/chapter.md`
 * （相对项目根，见 Core chapter/mod.rs）。只有这种路径才支持在面板内做真实合并：
 * 合并结果必须先写回该章节，再清除冲突标记。
 */
export function resolveChapterIdFromConflictPath(path: string): string | null {
  const parts: string[] = path.split('/')
  if (parts.length !== 5) {
    return null
  }
  if (parts[0] !== 'volumes' || parts[2] !== 'chapters' || parts[4] !== 'chapter.md') {
    return null
  }
  const chapterId: string = parts[3]
  return chapterId.length > 0 ? chapterId : null
}

/**
 * 单个冲突条目的 UI 状态。
 *
 * @ObservedV2 + @Trace：逐项展开、预览加载进度与解决进度都能被面板直接观察，
 * 不需要在 UI 层复制一份状态。
 */
@ObservedV2
export class SyncConflictItemState {
  // 条目身份字段来自 Core，创建后不再变化，不需要 @Trace。
  targetKind: string
  projectId: string
  projectTitle: string
  path: string
  kind: string
  createdAt: number

  @Trace expanded: boolean = false
  @Trace previewLoading: boolean = false
  @Trace previewLoaded: boolean = false
  @Trace preview: SyncConflictPreview | null = null
  @Trace previewErrorCode: string = ''
  @Trace resolving: boolean = false
  @Trace actionErrorCode: string = ''
  /** 面板内合并编辑：正在编辑合并结果 / 合并内容 / 正在保存合并结果。 */
  @Trace mergeEditing: boolean = false
  @Trace mergeDraft: string = ''
  @Trace mergeSaving: boolean = false

  constructor(entry: AllSyncConflictEntry) {
    this.targetKind = entry.targetKind
    // App target 在 Core 里 project_id 为空字符串，这里统一压成空串（不参与解决调用）。
    this.projectId = safeDisplayText(entry.projectId)
    this.projectTitle = safeDisplayText(entry.projectTitle)
    this.path = entry.path
    this.kind = entry.kind
    this.createdAt = entry.createdAt
  }

  /** 与 conflictKeyOfEntry 同口径，用于刷新时复用已有条目状态。 */
  key(): string {
    return `${this.targetKind}|${this.projectId}|${this.path}`
  }

  /** 解决/预览接口的 projectId 参数：app target 传 null。 */
  resolveProjectId(): string | null {
    return this.targetKind === 'project' ? this.projectId : null
  }

  /** 需要展开加载预览的条目。 */
  needsPreviewLoad(): boolean {
    return !this.previewLoaded && !this.previewLoading
  }

  /** 该冲突的章节正文身份（仅章节正文冲突可解析）。 */
  resolveChapterId(): string | null {
    return resolveChapterIdFromConflictPath(this.path)
  }

  /**
   * 面板内"合并"可用：章节正文冲突、预览已加载、远端未被删除。
   * 至少一侧有内容（本地内容优先）才能作为合并草稿；真正写盘与清冲突在 saveMerged。
   */
  canMergeInPanel(): boolean {
    const current: SyncConflictPreview | null = this.preview
    if (current === null) {
      return false
    }
    if (current.remoteDeleted) {
      return false
    }
    if (this.resolveChapterId() === null) {
      return false
    }
    return hasTextContent(current.localContent) || hasTextContent(current.remoteContent)
  }
}

@ObservedV2
export class SyncConflictViewModel {
  private bridge: NativeSyncBridge = new NativeSyncBridge(new NativeCoreModule())

  /** Core 全局冲突列表的 UI 状态；每次操作后整体从 Core 重新构建。 */
  @Trace items: SyncConflictItemState[] = []
  @Trace isLoading: boolean = false
  @Trace isResolving: boolean = false
  @Trace listErrorCode: string = ''
  /**
   * 解决操作成功但活跃写作会话未能按新落盘内容回填时的结构化错误码。
   * 此时条目可能已从剩余冲突列表消失，必须留在面板级提示，不能静默。
   */
  @Trace resolveNoticeCode: string = ''

  /** 是否有任何列表加载/解决操作在进行（面板据此统一禁用操作）。 */
  isBusy(): boolean {
    return this.isLoading || this.isResolving
  }

  /**
   * 从 Core 重新拉取全局冲突列表。
   * 仍存在的冲突复用原条目对象，保留展开/预览状态；已解决的条目自然消失。
   */
  async loadConflicts(): Promise<void> {
    if (this.isLoading) {
      return
    }
    this.isLoading = true
    this.listErrorCode = ''
    try {
      const result: ResultEnvelope<AllSyncConflictEntry[]> = await this.bridge.listAllSyncConflicts()
      if (!result.success || result.data === undefined) {
        // 拉取失败保留 Core 的结构化错误码，不清空上一次列表、不伪造"无冲突"。
        this.listErrorCode = result.errorCode ?? 'UNKNOWN_ERROR'
        hilog.error(DOMAIN, TAG, 'loadConflicts failed: errorCode=%{public}s', this.listErrorCode)
        return
      }

      const previous: SyncConflictItemState[] = this.items
      const next: SyncConflictItemState[] = []
      for (const entry of result.data) {
        const key: string = conflictKeyOfEntry(entry)
        let reused: SyncConflictItemState | null = null
        for (const old of previous) {
          if (old.key() === key) {
            reused = old
            break
          }
        }
        next.push(reused !== null ? reused : new SyncConflictItemState(entry))
      }
      this.items = next
    } catch (e) {
      // 只记异常类型，不记 String(e)。
      hilog.error(DOMAIN, TAG, 'loadConflicts exception: %{public}s',
        e instanceof Error ? e.name : 'Unknown')
      this.listErrorCode = 'NATIVE_ERROR'
    } finally {
      this.isLoading = false
    }
  }

  /** 展开/收起单条冲突；首次展开时按需加载预览。 */
  toggleExpand(item: SyncConflictItemState): void {
    item.expanded = !item.expanded
    if (item.expanded && item.needsPreviewLoad()) {
      void this.loadPreview(item)
    }
  }

  /** 加载单条冲突的本地/远端内容预览。 */
  async loadPreview(item: SyncConflictItemState): Promise<void> {
    if (item.previewLoading) {
      return
    }
    item.previewLoading = true
    item.previewErrorCode = ''
    try {
      const result: ResultEnvelope<SyncConflictPreview> = await this.bridge.loadSyncConflictPreview(
        item.targetKind, item.resolveProjectId(), item.path)
      if (result.success && result.data !== undefined) {
        item.preview = result.data
        item.previewLoaded = true
      } else {
        item.previewErrorCode = result.errorCode ?? 'UNKNOWN_ERROR'
        hilog.error(DOMAIN, TAG, 'loadPreview failed: errorCode=%{public}s', item.previewErrorCode)
      }
    } catch (e) {
      hilog.error(DOMAIN, TAG, 'loadPreview exception: %{public}s',
        e instanceof Error ? e.name : 'Unknown')
      item.previewErrorCode = 'NATIVE_ERROR'
    } finally {
      item.previewLoading = false
    }
  }

  /**
   * 解决单条冲突（保留本地 / 采用远端）。成功后立即从 Core 重新拉取剩余冲突（不在 UI 里减计数）。
   * 失败时把 Core 的结构化 errorCode 留在条目上，由面板展示。
   *
   * `mark_merged` 不走这里：清除合并标记的唯一入口是 saveMerged()，
   * 必须先真实保存合并结果并由 Core 确认写入成功。
   */
  async resolve(item: SyncConflictItemState, decision: SyncConflictDecision): Promise<boolean> {
    if (this.isResolving || item.resolving) {
      return false
    }
    if (decision === 'mark_merged') {
      // 不允许在没有合并保存的情况下直接清冲突标记。
      return false
    }

    this.isResolving = true
    item.resolving = true
    item.actionErrorCode = ''
    this.resolveNoticeCode = ''
    try {
      const result: ResultEnvelope<boolean> = decision === 'take_remote'
        ? await this.resolveTakeRemoteWithinActiveSession(item)
        : await this.bridge.resolveSyncConflict(
          item.targetKind, item.resolveProjectId(), item.path, decision)
      if (!result.success) {
        item.actionErrorCode = result.errorCode ?? 'UNKNOWN_ERROR'
        hilog.error(DOMAIN, TAG, 'resolve failed: decision=%{public}s errorCode=%{public}s',
          decision, item.actionErrorCode)
        return false
      }
      // data=false 表示老数据兼容路径：仅排队 pending_take_remote，live 正文要等下次同步才替换。
      hilog.info(DOMAIN, TAG, 'resolve ok: decision=%{public}s appliedLive=%{public}s',
        decision, (result.data === true).toString())
      // 解决结果只信 Core：无论成功展示态如何都重新拉取剩余冲突。
      await this.loadConflicts()
      return true
    } catch (e) {
      if (e instanceof SyncPreflightError) {
        item.actionErrorCode = e.errorCode.length > 0 ? e.errorCode : SYNC_PREFLIGHT_SAVE_FAILED
        hilog.error(DOMAIN, TAG, 'resolve preflight save failed: decision=%{public}s errorCode=%{public}s',
          decision, item.actionErrorCode)
        return false
      }
      hilog.error(DOMAIN, TAG, 'resolve exception: %{public}s',
        e instanceof Error ? e.name : 'Unknown')
      item.actionErrorCode = 'NATIVE_ERROR'
      return false
    } finally {
      item.resolving = false
      this.isResolving = false
    }
  }

  /**
   * 进入合并编辑：草稿以本地版本为准，本地无内容时用远端版本。
   * 只编辑内存草稿，不改写任何文件；真正写盘在 saveMerged()。
   */
  beginMerge(item: SyncConflictItemState): void {
    if (item.mergeEditing || this.isResolving || item.resolving || this.isLoading) {
      return
    }
    const current: SyncConflictPreview | null = item.preview
    if (current === null || !item.canMergeInPanel()) {
      return
    }
    const localText: string = safeDisplayText(current.localContent)
    const remoteText: string = safeDisplayText(current.remoteContent)
    item.mergeDraft = localText.length > 0 ? localText : remoteText
    item.actionErrorCode = ''
    item.mergeEditing = true
  }

  /** 放弃合并编辑，不改动任何文件与冲突状态。 */
  cancelMerge(item: SyncConflictItemState): void {
    if (item.mergeSaving) {
      return
    }
    item.mergeEditing = false
    item.mergeDraft = ''
    item.actionErrorCode = ''
  }

  /**
   * 保存合并结果（真实合并流程，Issue #926 复核）：
   * 1. 先把合并内容真实写回章节（Core 确认 saveChapter 成功）；
   * 2. 再调用 Core 的 mark_merged 清除冲突标记——只有确认合并内容已落盘才允许清标记；
   * 3. 最后按 #923 活跃写作会话事务回填打开中的章节（磁盘内容与编辑器身份重新对齐）。
   * 任一步失败都保留结构化 errorCode，不伪造成功、不静默清冲突。
   */
  async saveMerged(item: SyncConflictItemState): Promise<boolean> {
    if (this.isResolving || item.resolving || item.mergeSaving) {
      return false
    }
    if (!item.mergeEditing) {
      return false
    }
    const chapterId: string | null = item.resolveChapterId()
    if (chapterId === null) {
      item.actionErrorCode = 'INVALID_ARGUMENT'
      return false
    }
    const draft: string = item.mergeDraft

    this.isResolving = true
    item.resolving = true
    item.mergeSaving = true
    item.actionErrorCode = ''
    this.resolveNoticeCode = ''
    try {
      const merged: boolean = await ActiveWritingSessionRegistry.getInstance().withSyncBarrier<boolean>(
        async (scope: SyncBarrierScope): Promise<boolean> => {
          // 合并结果先落盘；写入失败时冲突标记必须保持原样。
          const saveResult: ResultEnvelope<ChapterSaveReceipt> =
            await getBridge().saveChapter(chapterId, draft)
          if (!saveResult.success) {
            item.actionErrorCode = saveResult.errorCode ?? 'UNKNOWN_ERROR'
            hilog.error(DOMAIN, TAG, 'saveMerged saveChapter failed: errorCode=%{public}s',
              item.actionErrorCode)
            return false
          }
          // Core 已确认合并内容写入成功，才清除冲突标记（mark_merged 语义：live 已是合并版本）。
          const markResult: ResultEnvelope<boolean> = await this.bridge.resolveSyncConflict(
            item.targetKind, item.resolveProjectId(), item.path, 'mark_merged')
          if (!markResult.success) {
            item.actionErrorCode = markResult.errorCode ?? 'UNKNOWN_ERROR'
            hilog.error(DOMAIN, TAG, 'saveMerged markMerged failed: errorCode=%{public}s',
              item.actionErrorCode)
            return false
          }
          // 打开的章节可能正持有旧正文：按落盘身份回填（与 take_remote 同一套会话事务）。
          const aligned: boolean = await scope.reconcileAfterSync()
          if (!aligned) {
            item.actionErrorCode = 'SYNC_CONFLICT'
            this.resolveNoticeCode = 'SYNC_CONFLICT'
            hilog.error(DOMAIN, TAG,
              'saveMerged applied but active session has unsubmitted content; reload required')
            return false
          }
          return true
        })
      if (!merged) {
        return false
      }
      item.mergeEditing = false
      // 合并结果只信 Core：重新拉取剩余冲突，不在 UI 里减计数。
      await this.loadConflicts()
      return true
    } catch (e) {
      if (e instanceof SyncPreflightError) {
        item.actionErrorCode = e.errorCode.length > 0 ? e.errorCode : SYNC_PREFLIGHT_SAVE_FAILED
        hilog.error(DOMAIN, TAG, 'saveMerged preflight save failed: errorCode=%{public}s',
          item.actionErrorCode)
        return false
      }
      hilog.error(DOMAIN, TAG, 'saveMerged exception: %{public}s',
        e instanceof Error ? e.name : 'Unknown')
      item.actionErrorCode = 'NATIVE_ERROR'
      return false
    } finally {
      item.mergeSaving = false
      item.resolving = false
      this.isResolving = false
    }
  }

  /**
   * "采用远端"会替换活跃章节的 live 正文，必须走 #923 的唯一写作会话事务：
   * 先保存活跃正文并建立屏障 → Core 替换文件 → 按落盘内容回填打开中的编辑器。
   * 未提交本地内容导致回填失败时记录明确冲突，不能静默让旧内存正文在下次保存覆盖已解决版本。
   */
  private async resolveTakeRemoteWithinActiveSession(item: SyncConflictItemState): Promise<ResultEnvelope<boolean>> {
    return await ActiveWritingSessionRegistry.getInstance().withSyncBarrier<ResultEnvelope<boolean>>(
      async (scope: SyncBarrierScope): Promise<ResultEnvelope<boolean>> => {
        const result: ResultEnvelope<boolean> =
          await this.bridge.resolveSyncConflict(item.targetKind, item.resolveProjectId(), item.path, 'take_remote')
        if (result.success && result.data === true) {
          const aligned: boolean = await scope.reconcileAfterSync()
          if (!aligned) {
            item.actionErrorCode = 'SYNC_CONFLICT'
            this.resolveNoticeCode = 'SYNC_CONFLICT'
            hilog.error(DOMAIN, TAG,
              'take_remote applied but active session has unsubmitted content; reload required')
          }
        }
        return result
      })
  }
}
