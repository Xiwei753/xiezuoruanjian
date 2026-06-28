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
      case 'error.invalid_workspace':
        return context.resourceManager.getStringSync($r('app.string.error_invalid_workspace').id)
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
      case 'error.refuse_delete_workspace_root':
        return context.resourceManager.getStringSync($r('app.string.error_refuse_delete_workspace_root').id)
      case 'error.invalid_delete_target':
        return context.resourceManager.getStringSync($r('app.string.error_invalid_delete_target').id)
      case 'error.sync_conflict':
        return context.resourceManager.getStringSync($r('app.string.error_sync_conflict').id)
      case 'error.sync_failed':
        return context.resourceManager.getStringSync($r('app.string.error_sync_failed_message').id)
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

  private static fallbackMessage(context: Context, errorCode: string | undefined): string {
    if (errorCode && errorCode.trim().length > 0) {
      return context.resourceManager.getStringSync($r('app.string.error_internal').id)
    }
    return context.resourceManager.getStringSync($r('app.string.error_other').id)
  }
}
