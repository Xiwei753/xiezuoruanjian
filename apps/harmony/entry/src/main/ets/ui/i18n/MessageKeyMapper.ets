/**
 * 将 Core 返回的 messageKey + messageArgs 映射到 HarmonyOS string resource。
 *
 * 这是 messageKey 合同在 HarmonyOS 端的唯一映射点。
 * 所有错误展示都应通过此 mapper 获取用户可见文案。
 */
export class MessageKeyMapper {
  /**
   * 将 messageKey 映射为本地化字符串。
   * 返回值是已解析的本地化文案，供 UI 直接展示。
   *
   * 通过 context.resourceManager.getStringSync($r('app.string.xxx').id) 走资源文件，
   * 确保所有文案统一由 string.json 管理，支持 i18n。
   */
  static resolveMessage(context: Context, messageKey: string | undefined, messageArgs: Record<string, string> | undefined,
    errorCode: string | undefined): string {
    if (!messageKey || messageKey.trim().length === 0) {
      return MessageKeyMapper.fallbackMessage(context, errorCode)
    }
    switch (messageKey) {
      case 'error.io':
        return context.resourceManager.getStringSync($r('app.string.error_io').id)
      case 'error.json':
        return context.resourceManager.getStringSync($r('app.string.error_json').id)
      case 'error.project_not_found':
        return context.resourceManager.getStringSync($r('app.string.error_project_not_found').id)
      case 'error.volume_not_found':
        return context.resourceManager.getStringSync($r('app.string.error_volume_not_found').id)
      case 'error.chapter_not_found':
        return context.resourceManager.getStringSync($r('app.string.error_chapter_not_found').id)
      case 'error.empty_overwrite_blocked':
        return context.resourceManager.getStringSync($r('app.string.error_empty_overwrite_blocked').id)
      case 'error.not_implemented':
        return context.resourceManager.getStringSync($r('app.string.error_not_implemented').id)
      case 'error.refuse_delete_root':
        return context.resourceManager.getStringSync($r('app.string.error_refuse_delete_root').id)
      case 'error.invalid_delete_target':
        return context.resourceManager.getStringSync($r('app.string.error_invalid_delete_target').id)
      case 'error.sync_conflict':
        return context.resourceManager.getStringSync($r('app.string.error_sync_conflict').id)
      case 'error.sync_failed':
        return context.resourceManager.getStringSync($r('app.string.error_sync_failed_message').id)
      case 'error.token_missing':
        return context.resourceManager.getStringSync($r('app.string.sync_token_missing').id)
      case 'error.token_invalid':
      case 'error.token_permission_denied':
      case 'error.auth_error':
      case 'error.auth_failed':
      case 'error.github_unauthorized':
      case 'error.github_forbidden':
        return context.resourceManager.getStringSync($r('app.string.sync_auth_failed').id)
      case 'error.repo_not_found_or_no_permission':
        return context.resourceManager.getStringSync($r('app.string.sync_repo_not_found').id)
      case 'error.remote_branch_missing':
      case 'error.branch_missing':
        return context.resourceManager.getStringSync($r('app.string.sync_branch_missing').id)
      case 'error.github_network_failed':
      case 'error.dns_failed':
      case 'error.tls_failed':
      case 'error.network_probe_failed':
      case 'error.network_failed':
        return context.resourceManager.getStringSync($r('app.string.sync_network_failed').id)
      case 'error.non_fast_forward':
        return context.resourceManager.getStringSync($r('app.string.sync_non_fast_forward').id)
      case 'error.unrelated_histories':
        return context.resourceManager.getStringSync($r('app.string.sync_unrelated_histories').id)
      case 'error.checkout_conflict':
      case 'error.local_blocking_file':
      case 'error.conflict':
        return context.resourceManager.getStringSync($r('app.string.error_sync_conflict').id)
      case 'error.empty_url':
      case 'error.not_configured':
        return context.resourceManager.getStringSync($r('app.string.sync_token_missing').id)
      case 'error.missing_permission':
      case 'error.permission_missing':
        return context.resourceManager.getStringSync($r('app.string.sync_network_failed').id)
      case 'error.dirty_repo_blocked':
        return context.resourceManager.getStringSync($r('app.string.sync_dirty_repo_blocked').id)
      case 'error.sync_retryable_network':
        return context.resourceManager.getStringSync($r('app.string.error_sync_retryable_network').id)
      case 'error.sync_retryable_io':
        return context.resourceManager.getStringSync($r('app.string.error_sync_retryable_io').id)
      case 'error.sync_dirty_repository':
        return context.resourceManager.getStringSync($r('app.string.error_sync_dirty_repository').id)
      case 'error.sync_protocol_error':
        return context.resourceManager.getStringSync($r('app.string.error_sync_protocol_error').id)
      case 'error.sync_fatal':
        return context.resourceManager.getStringSync($r('app.string.error_sync_fatal').id)
      case 'sync.result.generic_error':
        return context.resourceManager.getStringSync($r('app.string.sync_result_generic_error').id)
      case 'sync.result.auth_failed':
        return context.resourceManager.getStringSync($r('app.string.sync_result_auth_failed').id)
      case 'sync.result.token_permission_denied':
        return context.resourceManager.getStringSync($r('app.string.sync_result_token_permission_denied').id)
      case 'sync.result.conflict_summary':
        return context.resourceManager.getStringSync($r('app.string.sync_result_conflict_summary').id)
      case 'sync.result.network_failed':
        return context.resourceManager.getStringSync($r('app.string.sync_result_network_failed').id)
      // SyncErrorCategory 官方线格式码（core/writer_core/src/sync/types.rs::SyncErrorCategory::from_code）
      // 这些是 Core 可能直接返回的 errorCategory 值，用于缺少 messageKey 时的回退映射。
      case 'auth_failed':
      case 'auth_error':
        return context.resourceManager.getStringSync($r('app.string.sync_result_auth_failed').id)
      case 'permission_denied':
      case 'token_permission_denied':
      case 'missing_permission':
        return context.resourceManager.getStringSync($r('app.string.sync_result_token_permission_denied').id)
      case 'not_found':
        // NotFound 在 Core to_message_key() 中映射到 auth_failed，因为 UI 文案是"认证失败/请检查 token"
        return context.resourceManager.getStringSync($r('app.string.sync_result_auth_failed').id)
      case 'precondition_failed':
      case 'remote_sha_conflict':
      case 'conflict':
        return context.resourceManager.getStringSync($r('app.string.sync_result_conflict_summary').id)
      case 'rate_limited':
        return context.resourceManager.getStringSync($r('app.string.sync_result_generic_error').id)
      case 'network':
      case 'network_failed':
      case 'network_error':
        return context.resourceManager.getStringSync($r('app.string.sync_result_network_failed').id)
      case 'temporary_unavailable':
        return context.resourceManager.getStringSync($r('app.string.sync_result_network_failed').id)
      case 'local_io':
      case 'local_io_error':
        return context.resourceManager.getStringSync($r('app.string.sync_result_generic_error').id)
      case 'error.other':
        return context.resourceManager.getStringSync($r('app.string.error_other').id)
      case 'error.native_error':
        return context.resourceManager.getStringSync($r('app.string.error_internal').id)
      case 'error.parse_error':
        return context.resourceManager.getStringSync($r('app.string.error_json').id)
      case 'error.not_implemented_bridge':
        return context.resourceManager.getStringSync($r('app.string.error_not_implemented').id)
      default:
        return MessageKeyMapper.fallbackMessage(context, errorCode)
    }
  }

  /**
   * 解析 SyncErrorCategory 线格式码到本地化字符串。
   * 只在 category 属于 Core 有效 SyncErrorCategory 时返回对应资源；
   * 未知返回 undefined，由调用方按 errorCode 回退。
   *
   * Core 的 SyncErrorCategory 定义在 core/writer_core/src/sync/types.rs::SyncErrorCategory::from_code，
   * 有效值包括：auth_failed, auth_error, permission_denied, token_permission_denied,
   * missing_permission, not_found, precondition_failed, remote_sha_conflict, conflict,
   * rate_limited, network, network_failed, network_error, temporary_unavailable,
   * local_io, local_io_error。
   */
  static resolveSyncErrorCategory(context: Context, category: string | undefined,
    messageArgs: Record<string, string> | undefined): string | undefined {
    if (!category || category.trim().length === 0) {
      return undefined
    }
    switch (category) {
      case 'auth_failed':
      case 'auth_error':
        return context.resourceManager.getStringSync($r('app.string.sync_result_auth_failed').id)
      case 'permission_denied':
      case 'token_permission_denied':
      case 'missing_permission':
        return context.resourceManager.getStringSync($r('app.string.sync_result_token_permission_denied').id)
      case 'not_found':
        return context.resourceManager.getStringSync($r('app.string.sync_result_auth_failed').id)
      case 'precondition_failed':
      case 'remote_sha_conflict':
      case 'conflict':
        return context.resourceManager.getStringSync($r('app.string.sync_result_conflict_summary').id)
      case 'rate_limited':
        return context.resourceManager.getStringSync($r('app.string.sync_result_generic_error').id)
      case 'network':
      case 'network_failed':
      case 'network_error':
      case 'temporary_unavailable':
        return context.resourceManager.getStringSync($r('app.string.sync_result_network_failed').id)
      case 'local_io':
      case 'local_io_error':
        return context.resourceManager.getStringSync($r('app.string.sync_result_generic_error').id)
      default:
        return undefined
    }
  }

  private static fallbackMessage(context: Context, errorCode: string | undefined): string {
    if (errorCode && errorCode.trim().length > 0) {
      // 原生同步任务启动/排队失败，或 Core 未返回结果：给系统任务失败提示。
      switch (errorCode) {
        case 'ASYNC_CREATE_FAILED':
        case 'ASYNC_QUEUE_FAILED':
        case 'NULL_RESULT':
          return context.resourceManager.getStringSync($r('app.string.error_sync_task_failed').id)
        default:
          return context.resourceManager.getStringSync($r('app.string.error_internal').id)
      }
    }
    return context.resourceManager.getStringSync($r('app.string.error_other').id)
  }
}
