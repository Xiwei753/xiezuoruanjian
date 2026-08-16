import UIAbility from '@ohos.app.ability.UIAbility'
import Want from '@ohos.app.ability.Want'
import AbilityConstant from '@ohos.app.ability.AbilityConstant'
import hilog from '@ohos.hilog'
import window from '@ohos.window'
import { HarmonyLifecycle } from '../platform/lifecycle/HarmonyLifecycle'
import { getAppContext } from '../app/AppContext'
import { getAdaptiveContext } from '../ui/adaptive/AdaptiveContext'
import { AppContinuationService } from '../platform/continuity/AppContinuationService'

export default class EntryAbility extends UIAbility {
  private lifecycle: HarmonyLifecycle = HarmonyLifecycle.getInstance()
  private continuationService: AppContinuationService = AppContinuationService.getInstance()

  onCreate(want: Want, launchParam: AbilityConstant.LaunchParam): void {
    hilog.info(0x0000, 'EntryAbility', 'onCreate')
    getAppContext().setAbilityContext(this.context)
    this.lifecycle.notifyCreate()

    // 接续恢复：检查 launchReason === CONTINUATION，从 want.parameters 读取 payload。
    if (launchParam.launchReason === AbilityConstant.LaunchReason.CONTINUATION) {
      this.restoreContinuationFromWant(want)
    }
  }

  onDestroy() {
    hilog.info(0x0000, 'EntryAbility', 'onDestroy')
    this.lifecycle.notifyDestroy()
  }

  onWindowStageCreate(windowStage: window.WindowStage) {
    hilog.info(0x0000, 'EntryAbility', 'onWindowStageCreate')

    // 初始化响应式断点系统
    getAdaptiveContext().initialize(windowStage)

    windowStage.loadContent('pages/Index', (err, data) => {
      if (err.code) {
        hilog.error(0x0000, 'EntryAbility', 'Failed to load the content. Cause: %{public}s', JSON.stringify(err))
        return
      }
      hilog.info(0x0000, 'EntryAbility', 'Succeeded in loading the content. Data: %{public}s', JSON.stringify(data))
    })

    this.lifecycle.notifyForeground()
  }

  onWindowStageDestroy() {
    hilog.info(0x0000, 'EntryAbility', 'onWindowStageDestroy')
    this.lifecycle.notifyBackground()
  }

  onForeground() {
    hilog.info(0x0000, 'EntryAbility', 'onForeground')
    this.lifecycle.notifyForeground()
  }

  onBackground() {
    hilog.info(0x0000, 'EntryAbility', 'onBackground')
    this.lifecycle.notifyBackground()
  }

  // 应用接续：源设备触发接续时调用。
  // 从 AppContinuationService.getCurrentPayload() 取 payload 写入 wantParam。
  onContinue(wantParam: Record<string, Object>): void {
    hilog.info(0x0000, 'EntryAbility', 'onContinue')
    const payload = this.continuationService.getCurrentPayload()
    if (payload !== null) {
      wantParam['continuationData'] = this.continuationService.serializeForWant(payload)
    }
  }

  // 新 want 到达（接续恢复在目标设备的二次入口）。
  onNewWant(want: Want, launchParam: AbilityConstant.LaunchParam): void {
    hilog.info(0x0000, 'EntryAbility', 'onNewWant')
    if (launchParam.launchReason === AbilityConstant.LaunchReason.CONTINUATION) {
      this.restoreContinuationFromWant(want)
    }
  }

  // —— private ——
  // 从 want.parameters 读取接续 payload，交给 AppContinuationService.restoreFromWant() 还原。
  private restoreContinuationFromWant(want: Want): void {
    const parameters: Record<string, Object> | undefined = want.parameters
    if (parameters === undefined || parameters === null) {
      return
    }
    const restored = this.continuationService.restoreFromWant(parameters)
    if (restored !== null) {
      hilog.info(0x0000, 'EntryAbility', 'Continuation restored: projectId=%{public}s', restored.projectId ?? '')
    }
  }
}
