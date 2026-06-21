/**
 * 将 Core 返回的 messageKey + messageArgs 映射到 HarmonyOS string resource。
 *
 * 这是 messageKey 合同在 HarmonyOS 端的唯一映射点。
 * 所有错误展示都应通过此 mapper 获取用户可见文案。
 */
export class MessageKeyMapper {
  /**
   * 将 messageKey 映射为本地化字符串。
   * 返回值是已解析的中文文案，供 UI 直接展示。
   *
   * 由于 HarmonyOS ArkTS 的 $r() 限制（静态方法中无法直接使用），
   * 当前实现使用硬编码中文文案作为映射值，与现有行为一致，
   * 但通过 mapper 统一入口，后续可切换为 context.resourceManager 方案。
   */
  static resolveMessage(messageKey: string | undefined, messageArgs: Record<string, string> | undefined,
    errorCode: string | undefined): string {
    if (!messageKey || messageKey.trim().length === 0) {
      return MessageKeyMapper.fallbackMessage(errorCode)
    }
    switch (messageKey) {
      case 'error.io':
        return '文件读写失败，请检查工作区权限和磁盘状态'
      case 'error.json':
        return '数据文件格式异常，请检查工作区文件是否损坏'
      case 'error.invalid_workspace':
        return '不是有效的工作区'
      case 'error.project_not_found':
        return '作品不存在或已被删除'
      case 'error.volume_not_found':
        return '卷不存在或已被删除'
      case 'error.chapter_not_found':
        return '章节不存在或已被删除'
      case 'error.empty_overwrite_blocked':
        return '已阻止空内容覆盖现有章节'
      case 'error.not_implemented':
        return '该功能尚未实现'
      case 'error.refuse_delete_workspace_root':
        return '拒绝删除工作区根目录'
      case 'error.invalid_delete_target':
        return '删除目标无效'
      case 'error.sync_conflict':
        return '同步冲突，请手动处理冲突文件后重试'
      case 'error.sync_failed':
        return '同步失败，请检查网络和配置'
      case 'error.other':
        return '操作失败'
      case 'error.native_error':
        return '应用内部错误，请查看日志'
      case 'error.parse_error':
        return '数据文件格式异常，请检查工作区文件是否损坏'
      case 'error.not_implemented_bridge':
        return '该功能尚未实现'
      default:
        return MessageKeyMapper.fallbackMessage(errorCode)
    }
  }

  private static fallbackMessage(errorCode: string | undefined): string {
    if (errorCode && errorCode.trim().length > 0) {
      return '应用内部错误，请查看日志'
    }
    return '操作失败'
  }
}