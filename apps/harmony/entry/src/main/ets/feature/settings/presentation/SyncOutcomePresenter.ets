// SyncOutcomePresenter — 同步结果到用户可见文案的唯一展示点。
//
// 顶栏 Toast 与设置页都通过这里把 Controller 的类型化结果映射成中文，
// 复用 MessageKeyMapper 这一份映射表，不再各自维护中文错误字典。

import { MessageKeyMapper } from '../../../ui/i18n/MessageKeyMapper'
import { SyncOutcome } from './SyncOperationController'

export class SyncOutcomePresenter {
  /**
   * 把同步结果渲染为用户可见文案。
   * @param context 资源管理器来源（UIAbilityContext 或页面 Context）。
   */
  static present(context: Context, outcome: SyncOutcome): string {
    if (outcome.phase === 'success') {
      const count: number = outcome.totalDownloaded ?? 0
      const format: string = context.resourceManager.getStringSync($r('app.string.sync_complete_format').id)
      return format.replace('%s', count.toString())
    }
    // 失败：优先使用 Core 的真实 messageKey，其次回退 errorCode，均由 MessageKeyMapper 统一映射。
    if (outcome.messageKey !== undefined && outcome.messageKey.trim().length > 0) {
      return MessageKeyMapper.resolveMessage(context, outcome.messageKey, outcome.messageArgs, outcome.errorCode)
    }
    return MessageKeyMapper.resolveMessage(context, undefined, undefined, outcome.errorCode)
  }
}
