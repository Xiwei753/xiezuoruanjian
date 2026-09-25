// =============================================================================
// HarmonyColorModeController.ets — 深浅色模式真正下发控制器
// =============================================================================
//
// Issue #767 评论 5831321551：此前 HarmonyThemeAdapter 只维护一份假的深浅色状态，
// 从未调用 UIAbilityContext.setColorMode()，导致用户在设置页选择的浅色/深色模式
// 无法真正影响 HarmonyOS 系统层面的颜色模式。
//
// 本 controller 持有 UIAbilityContext，在页面 loadContent 完成后把用户选择的
// ThemeMode 通过 setColorMode() 真正下发给系统。
//
// 映射规则：
//   system → ConfigurationConstant.ColorMode.COLOR_MODE_NOT_SET（跟随系统）
//   light  → ConfigurationConstant.ColorMode.COLOR_MODE_LIGHT
//   dark   → ConfigurationConstant.ColorMode.COLOR_MODE_DARK
//
// 华为文档要求：setColorMode() 必须在 loadContent 成功之后才能调用，
// 因此引入 pageLoaded 标志和 desiredMode 缓存机制。
// =============================================================================

import { common, ConfigurationConstant } from '@kit.AbilityKit'
import hilog from '@ohos.hilog'
import { ThemeMode } from './HarmonyThemeTypes'

const LOG_DOMAIN: number = 0x0001
const LOG_TAG: string = 'HarmonyColorModeController'

export class HarmonyColorModeController {
  private static instance: HarmonyColorModeController
  private context: common.UIAbilityContext | null = null
  private pageLoaded: boolean = false
  private desiredMode: ThemeMode = 'system'

  static getInstance(): HarmonyColorModeController {
    if (!HarmonyColorModeController.instance) {
      HarmonyColorModeController.instance = new HarmonyColorModeController()
    }
    return HarmonyColorModeController.instance
  }

  // Issue #767 评论 5831321551：注入 UIAbilityContext，在 EntryAbility.onCreate 时调用。
  // context 就绪后立即尝试下发缓存的 desiredMode。
  bindContext(context: common.UIAbilityContext): void {
    this.context = context
    hilog.info(LOG_DOMAIN, LOG_TAG, 'bindContext: UIAbilityContext 已注入')
    this.applyDesiredMode()
  }

  // Issue #767 评论 5831321551：标记页面已 loadContent 完成，在 loadContent 成功回调里调用。
  // 此时如果有缓存的 desiredMode，真正下发给 UIAbility。
  markPageLoaded(): void {
    this.pageLoaded = true
    hilog.info(LOG_DOMAIN, LOG_TAG, 'markPageLoaded: 页面已加载完成，开始下发缓存的 desiredMode')
    this.applyDesiredMode()
  }

  // Issue #767 评论 5831321551：设置期望的模式（由 HarmonyThemeAdapter.setMode/updateFromLocalSettings 调用）。
  // 映射：system → COLOR_MODE_NOT_SET, light → COLOR_MODE_LIGHT, dark → COLOR_MODE_DARK
  setDesiredMode(mode: ThemeMode): void {
    this.desiredMode = mode
    hilog.info(LOG_DOMAIN, LOG_TAG, `setDesiredMode: 期望模式已设置为 ${mode}`)
    this.applyDesiredMode()
  }

  // Issue #767 评论 5831321551：把 desiredMode 真正下发给 UIAbilityContext.setColorMode()。
  // 只在 pageLoaded && context != null 时才调用 setColorMode（华为文档要求 loadContent 后才能调用）。
  applyDesiredMode(): void {
    if (!this.pageLoaded || this.context === null) {
      hilog.info(LOG_DOMAIN, LOG_TAG,
        `applyDesiredMode: 条件不满足（pageLoaded=${this.pageLoaded}, context=${this.context === null ? 'null' : 'ready'}），跳过下发`)
      return
    }

    // Issue #767 评论 5831321551：使用 ConfigurationConstant.ColorMode 枚举，不手写数字。
    let colorMode: ConfigurationConstant.ColorMode
    if (this.desiredMode === 'system') {
      colorMode = ConfigurationConstant.ColorMode.COLOR_MODE_NOT_SET
    } else if (this.desiredMode === 'light') {
      colorMode = ConfigurationConstant.ColorMode.COLOR_MODE_LIGHT
    } else {
      colorMode = ConfigurationConstant.ColorMode.COLOR_MODE_DARK
    }

    try {
      // Issue #767 评论 5831658277：改用 ApplicationContext.setColorMode()（API11+），
      // 不再用 UIAbilityContext.setColorMode()（API18+），因为 compatibleSdkVersion=12。
      // ApplicationContext 语义正好是应用级深浅色，不需要 UIAbility 级覆盖。
      const appContext = this.context!.getApplicationContext()
      appContext.setColorMode(colorMode)
      hilog.info(LOG_DOMAIN, LOG_TAG,
        `applyDesiredMode: ApplicationContext.setColorMode(${this.desiredMode} → ${colorMode}) 调用成功`)
    } catch (error) {
      hilog.error(LOG_DOMAIN, LOG_TAG,
        `applyDesiredMode: ApplicationContext.setColorMode(${this.desiredMode} → ${colorMode}) 调用失败: ${error}`)
    }
  }
}
