import UIAbility from '@ohos.app.ability.UIAbility'
import Want from '@ohos.app.ability.Want'
import AbilityConstant from '@ohos.app.ability.AbilityConstant'
import hilog from '@ohos.hilog'
import window from '@ohos.window'
import { HarmonyLifecycle } from '../system/HarmonyLifecycle'
import { getAppContext } from '../common/AppContext'

export default class EntryAbility extends UIAbility {
  private lifecycle: HarmonyLifecycle = HarmonyLifecycle.getInstance()

  onCreate(want: Want, launchParam: AbilityConstant.LaunchParam): void {
    hilog.info(0x0000, 'EntryAbility', 'onCreate')
    getAppContext().setAbilityContext(this.context)
    this.lifecycle.notifyCreate()
  }

  onDestroy() {
    hilog.info(0x0000, 'EntryAbility', 'onDestroy')
    this.lifecycle.notifyDestroy()
  }

  onWindowStageCreate(windowStage: window.WindowStage) {
    hilog.info(0x0000, 'EntryAbility', 'onWindowStageCreate')

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
}
