import { AbilityConstant, Want, wantConstant } from '@kit.AbilityKit'
import hilog from '@ohos.hilog'
import { AppContinuationService } from './AppContinuationService'
import { ContinuationPayload } from './ContinuationPayload'
import { ContinuationSnapshotStore } from './ContinuationSnapshotStore'
import { NavigationTransactionCoordinator } from '../../app/navigation/NavigationTransactionCoordinator'
import { NearbyDeviceSyncCoordinator } from '../peer/NearbyDeviceSyncCoordinator'

// ContinuationEntrypoint — 应用接续（流转）的序列化/恢复入口。
// 从 EntryAbility 提取，使 EntryAbility 仅保留 Ability 生命周期调用。
// 接续是 Ability 级配置（continuable + onContinue/onNewWant），不是 DLP ACL 权限。
export class ContinuationEntrypoint {
  private static instance: ContinuationEntrypoint
  private continuationService: AppContinuationService = AppContinuationService.getInstance()

  static getInstance(): ContinuationEntrypoint {
    if (!ContinuationEntrypoint.instance) {
      ContinuationEntrypoint.instance = new ContinuationEntrypoint()
    }
    return ContinuationEntrypoint.instance
  }

  // 源设备触发接续时调用。从 AppContinuationService 取 payload 写入 wantParam。
  // SUPPORT_CONTINUE_PAGE_STACK_KEY=false：禁用自动页面栈迁移，
  // Navigation 页面栈不能跨设备自动迁移；由 NavigationTransactionCoordinator
  // 接收接续 payload，再交给 AppNavigation.applyContinuation 手动重建。
  // 用官方 wantConstant.Params.SUPPORT_CONTINUE_PAGE_STACK_KEY 常量，不自己写字符串字面量。
  onContinue(wantParam: Record<string, Object>): AbilityConstant.OnContinueResult {
    hilog.info(0x0000, 'ContinuationEntrypoint', 'onContinue')
    wantParam[wantConstant.Params.SUPPORT_CONTINUE_PAGE_STACK_KEY] = false as Object
    const payload: ContinuationPayload | null = this.continuationService.getCurrentPayload()
    if (payload !== null) {
      // Issue #988：未保存正文必须在 onContinue 同步返回前发起交付，但绝不能在这里
      // await 设备/GitHub 传输。目标设备按 snapshotHash 校验后才恢复选区/视口。
      this.publishSnapshotBestEffort(payload)
      wantParam['continuationData'] = this.continuationService.serializeForWant(payload)
      return AbilityConstant.OnContinueResult.AGREE
    }
    return AbilityConstant.OnContinueResult.REJECT
  }

  private publishSnapshotBestEffort(payload: ContinuationPayload): void {
    if (payload.screen !== 'writing' || payload.snapshotId === undefined) {
      return
    }
    const content: string | null = ContinuationSnapshotStore.getInstance().get(payload.snapshotId)
    if (content === null) {
      return
    }
    NearbyDeviceSyncCoordinator.getInstance().publishContinuationSnapshot(
      payload.snapshotId, content)
  }

  // 目标设备从 want 恢复接续 payload。
  // 从 want.parameters 读取 payload，交给 AppContinuationService.restoreFromWant() 还原，
  // 再交 NavigationTransactionCoordinator.restoreFromContinuation() 手动重建 NavPathStack
  // （SUPPORT_CONTINUE_PAGE_STACK_KEY=false 禁用了自动页面栈迁移）。
  restoreFromWant(want: Want): void {
    const parameters: Record<string, Object> | undefined = want.parameters
    if (parameters === undefined || parameters === null) {
      return
    }
    const restored: ContinuationPayload | null = this.continuationService.restoreFromWant(parameters)
    if (restored !== null) {
      hilog.info(0x0000, 'ContinuationEntrypoint', 'Continuation restored: projectId=%{public}s', restored.projectId ?? '')
      NavigationTransactionCoordinator.getInstance().restoreFromContinuation(restored)
    }
  }
}
