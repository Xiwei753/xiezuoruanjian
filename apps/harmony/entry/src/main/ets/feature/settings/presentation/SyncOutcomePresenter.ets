// SyncOutcomePresenter — 同步结果到用户可见文案的唯一展示点。
//
// 顶栏 Toast 与设置页都通过这里把 Controller 的类型化结果映射成中文，
// 复用 MessageKeyMapper 这一份映射表，不再各自维护中文错误字典。

import { MessageKeyMapper } from '../../../ui/i18n/MessageKeyMapper'
import { SyncOutcome } from './SyncRunTypes'

export class SyncOutcomePresenter {
  /**
   * 把同步结果渲染为用户可见文案。
   * @param context 资源管理器来源（UIAbilityContext 或页面 Context）。
   */
  static present(context: Context, outcome: SyncOutcome): string {
    const uploaded: number = outcome.totalUploaded ?? 0
    const downloaded: number = outcome.totalDownloaded ?? 0
    const counts: string = `（上传 ${uploaded} 个文件，下载 ${downloaded} 个文件）`
    const status: string = outcome.overallStatus ?? ''
    const typedReason: string = this.typedReason(context, outcome)

    // 会话对齐可能在远端同步已经成功后失败，Controller 此时会同时返回
    // phase=failed、SYNC_CONFLICT 和 overallStatus=success。失败语义优先，
    // 不让远端状态掩盖本地仍需处理的冲突。
    if (outcome.phase !== 'success') {
      if (outcome.phase === 'failed' && outcome.errorCode === 'SYNC_CONFLICT') {
        const conflictCount: number = outcome.totalConflicts ?? 0
        const conflictText: string = conflictCount > 0 ? `，${conflictCount} 处冲突` : ''
        return `远端已同步，但当前写作会话未对齐，需要处理${conflictText}${counts}`
      }
      const failureTitle: string = outcome.phase === 'failed' ? '同步失败' : '同步尚未完成'
      return `${failureTitle}${counts}${typedReason.length > 0 ? `：${typedReason}` : ''}`
    }

    if (outcome.errorCode === 'SYNC_CONFLICT') {
      const conflictCount: number = outcome.totalConflicts ?? 0
      const conflictText: string = conflictCount > 0 ? `，${conflictCount} 处冲突` : ''
      return `远端已同步，但当前写作会话未对齐，需要处理${conflictText}${counts}`
    }

    if (status === 'partial_conflict' || status === 'conflict' ||
      outcome.messageKey === 'sync.result.conflict_summary' ||
      (outcome.totalConflicts !== undefined && outcome.totalConflicts > 0)) {
      const conflictCount: number = outcome.totalConflicts ?? 0
      const conflictText: string = conflictCount > 0 ? `，${conflictCount} 处冲突` : '，存在冲突'
      return `部分同步完成${conflictText}${counts}`
    }

    if (status === 'no_changes') {
      return `同步完成，没有文件变化${counts}`
    }
    if (status === 'success' || status === 'latest_wins_applied') {
      return `同步完成${counts}`
    }
    if (status === 'recoverable_error') {
      return `同步遇到可恢复错误${counts}${typedReason.length > 0 ? `：${typedReason}` : ''}`
    }
    if (status === 'fatal_error' || status === 'error') {
      return `同步失败${counts}${typedReason.length > 0 ? `：${typedReason}` : ''}`
    }

    return `同步完成${counts}`
  }

  /** 仅使用类型化字段；rawError/message 可能包含 URL、凭据或远端原文，不展示给用户。 */
  private static typedReason(context: Context, outcome: SyncOutcome): string {
    if (outcome.messageKey !== undefined && outcome.messageKey.trim().length > 0) {
      return MessageKeyMapper.resolveMessage(context, outcome.messageKey, outcome.messageArgs, outcome.errorCode)
    }
    // messageKey 缺失：尝试用 errorCategory 推断用户可读原因
    if (outcome.errorCategory !== undefined && outcome.errorCategory.trim().length > 0) {
      const categoryMessage: string | undefined = MessageKeyMapper.resolveSyncErrorCategory(
        context, outcome.errorCategory, outcome.messageArgs)
      if (categoryMessage !== undefined) {
        return categoryMessage
      }
    }
    if (outcome.errorCode !== undefined && outcome.errorCode.trim().length > 0) {
      return MessageKeyMapper.resolveMessage(context, undefined, undefined, outcome.errorCode)
    }
    return ''
  }
}
