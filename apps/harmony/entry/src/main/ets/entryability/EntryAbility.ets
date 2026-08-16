import { AbilityConstant, UIAbility, Want, wantConstant } from '@kit.AbilityKit'
import hilog from '@ohos.hilog'
import window from '@ohos.window'
import { HarmonyLifecycle } from '../platform/lifecycle/HarmonyLifecycle'
import { getAppContext } from '../app/AppContext'
import { getAdaptiveContext } from '../ui/adaptive/AdaptiveContext'
import { AppContinuationService } from '../platform/continuity/AppContinuationService'
import { ContinuationPayload } from '../platform/continuity/ContinuationPayload'
import { AppNavigationHost } from '../app/navigation/AppNavigation'

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

  // 接续恢复阶段的目标设备窗口初始化：与 onWindowStageCreate 等价的根页面装载。
  // SUPPORT_CONTINUE_PAGE_STACK_KEY=false 禁用了自动页面栈迁移，这里只装载根页 pages/Index，
  // AppNavigation 首帧 aboutToAppear 时从 AppNavigationHost 消费已交付的接续 payload，
  // 由 AppNavigationHost.restoreFromContinuation 手动重建 NavPathStack。
  onWindowStageRestore(windowStage: window.WindowStage) {
    hilog.info(0x0000, 'EntryAbility', 'onWindowStageRestore')

    // 初始化响应式断点系统（与 onWindowStageCreate 等价）。
    getAdaptiveContext().initialize(windowStage)

    windowStage.loadContent('pages/Index', (err, data) => {
      if (err.code) {
        hilog.error(0x0000, 'EntryAbility', 'Failed to load the content on restore. Cause: %{public}s', JSON.stringify(err))
        return
      }
      hilog.info(0x0000, 'EntryAbility', 'Succeeded in loading the content on restore. Data: %{public}s', JSON.stringify(data))
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
  // 返回 AGREE 表示同意接续、REJECT 表示拒绝（无 payload 时）。
  // SUPPORT_CONTINUE_PAGE_STACK_KEY=false：评论明确要求禁用自动页面栈迁移，
  // Navigation 页面栈不能跨设备自动迁移，由 AppNavigation.restoreFromContinuation 手动重建。
  // 用官方 wantConstant.Params.SUPPORT_CONTINUE_PAGE_STACK_KEY 常量，不自己写字符串字面量。
  onContinue(wantParam: Record<string, Object>): AbilityConstant.OnContinueResult {
    hilog.info(0x0000, 'EntryAbility', 'onContinue')
    wantParam[wantConstant.Params.SUPPORT_CONTINUE_PAGE_STACK_KEY] = false as Object
    const payload: ContinuationPayload | null = this.continuationService.getCurrentPayload()
    if (payload !== null) {
      wantParam['continuationData'] = this.continuationService.serializeForWant(payload)
      return AbilityConstant.OnContinueResult.AGREE
    }
    return AbilityConstant.OnContinueResult.REJECT
  }

  // 新 want 到达（接续恢复在目标设备的二次入口）。
  onNewWant(want: Want, launchParam: AbilityConstant.LaunchParam): void {
    hilog.info(0x0000, 'EntryAbility', 'onNewWant')
    if (launchParam.launchReason === AbilityConstant.LaunchReason.CONTINUATION) {
      this.restoreContinuationFromWant(want)
    }
  }

  // —— private ——
  // 从 want.parameters 读取接续 payload，交给 AppContinuationService.restoreFromWant() 还原，
  // 再交 AppNavigationHost.restoreFromContinuation() 手动重建 NavPathStack
  // （SUPPORT_CONTINUE_PAGE_STACK_KEY=false 禁用了自动页面栈迁移）。
  private restoreContinuationFromWant(want: Want): void {
    const parameters: Record<string, Object> | undefined = want.parameters
    if (parameters === undefined || parameters === null) {
      return
    }
    const restored: ContinuationPayload | null = this.continuationService.restoreFromWant(parameters)
    if (restored !== null) {
      hilog.info(0x0000, 'EntryAbility', 'Continuation restored: projectId=%{public}s', restored.projectId ?? '')
      AppNavigationHost.getInstance().restoreFromContinuation(restored)
    }
  }
}
