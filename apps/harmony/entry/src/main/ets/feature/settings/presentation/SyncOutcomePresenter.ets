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
    const typedReason: string = SyncOutcomePresenter.typedReason(context, outcome)

    if (outcome.errorCode === 'SYNC_SESSION_UNALIGNED') {
      const sessionReason: string = typedReason.length > 0
        ? typedReason
        : '远端内容已同步，但当前写作会话未对齐，请检查当前文稿后再继续'
      return `${sessionReason}${counts}`
    }

    const hasCoreConflict: boolean = outcome.errorCode === 'SYNC_CONFLICT' ||
      status === 'partial_conflict' || status === 'conflict' ||
      outcome.messageKey === 'sync.result.conflict_summary'
    if (hasCoreConflict) {
      const conflictCount: number = outcome.totalConflicts ?? 0
      const conflictSummary: string = conflictCount > 0
        ? `同步发生冲突，部分内容已处理，剩余 ${conflictCount} 处需要解决`
        : '同步发生冲突，部分内容已处理，请查看设置中的待解决项'
      return `${conflictSummary}${counts}`
    }

    if (status === 'recoverable_error') {
      return `同步部分失败${counts}${typedReason.length > 0 ? `：${typedReason}` : '，请查看设置中的失败详情'}`
    }
    if (status === 'fatal_error' || status === 'error') {
      return `同步失败${counts}${typedReason.length > 0 ? `：${typedReason}` : ''}`
    }

    if (outcome.phase !== 'success') {
      const failureTitle: string = outcome.phase === 'failed' ? '同步失败' : '同步尚未完成'
      return `${failureTitle}${counts}${typedReason.length > 0 ? `：${typedReason}` : ''}`
    }

    if (status === 'no_changes') {
      return `同步完成，没有文件变化${counts}`
    }
    if (status === 'success' || status === 'latest_wins_applied') {
      return `同步完成${counts}`
    }

    return `同步结果尚未确认${counts}${typedReason.length > 0 ? `：${typedReason}` : ''}`
  }

  /** 仅使用类型化字段；rawError/message 可能包含 URL、凭据或远端原文，不展示给用户。 */
  private static typedReason(context: Context, outcome: SyncOutcome): string {
    if (outcome.failureStage === 'config_load') {
      return '同步配置读取失败，请检查本地同步配置。'
    }
    if (outcome.failureStage === 'secrets_load') {
      return '同步凭据读取失败，请检查本地凭据设置。'
    }
    switch (outcome.errorCategory) {
      case 'repository_unconfigured':
        return '云端仓库未配置，请先完成同步设置。'
      case 'network':
      case 'network_error':
      case 'network_failed':
      case 'retryable_network':
        return context.resourceManager.getStringSync($r('app.string.sync_result_network_failed').id)
      case 'authentication':
      case 'authentication_error':
      case 'auth_failed':
      case 'auth_error':
      case 'token_missing':
        return context.resourceManager.getStringSync($r('app.string.sync_result_auth_failed').id)
      case 'local_io':
      case 'local_io_error':
      case 'local_decode':
      case 'local_decode_error':
      case 'config_local_io':
      case 'config_file_read':
      case 'config_file_decode':
      case 'credentials_local_io':
      case 'credentials_file_read':
      case 'credentials_file_decode':
      case 'secure_storage_read':
      case 'secure_storage_decode':
      case 'credential_migration_io':
        return '读取本地同步数据失败，请检查配置和凭据文件。'
    }
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
