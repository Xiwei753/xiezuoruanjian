import { AbilityConstant, UIAbility, Want, wantConstant, Configuration, ConfigurationConstant } from '@kit.AbilityKit'
import hilog from '@ohos.hilog'
import window from '@ohos.window'
import { HarmonyLifecycle } from '../platform/lifecycle/HarmonyLifecycle'
import { getAppContext } from '../app/AppContext'
import { getAdaptiveContext } from '../ui/adaptive/AdaptiveContext'
import { AppContinuationService } from '../platform/continuity/AppContinuationService'
import { ContinuationPayload } from '../platform/continuity/ContinuationPayload'
import { NavigationTransactionCoordinator } from '../app/navigation/NavigationTransactionCoordinator'
import { NativeWriterCoreBridge } from '../corebridge/NativeWriterCoreBridge'
import { HarmonyDeviceIdentity } from '../platform/identity/HarmonyDeviceIdentity'
import { HarmonyThemeAdapter } from '../ui/theme/HarmonyThemeAdapter'
import { HarmonyColorModeController } from '../ui/theme/HarmonyColorModeController'
import i18n from '@ohos.i18n'
import bundleManager from '@ohos.bundle.bundleManager'

// Issue #670 评论 5652500781：构建身份（命名 interface，避免对象字面量类型）。
interface BuildIdentity {
  appVersion: string
  buildKey: string
}

export default class EntryAbility extends UIAbility {
  private lifecycle: HarmonyLifecycle = HarmonyLifecycle.getInstance()
  private continuationService: AppContinuationService = AppContinuationService.getInstance()

  onCreate(want: Want, launchParam: AbilityConstant.LaunchParam): void {
    hilog.info(0x0000, 'EntryAbility', 'onCreate')
    getAppContext().setAbilityContext(this.context)

    // Issue #767 评论 5831321551：把 UIAbilityContext 注入 HarmonyColorModeController，
    // 使其能在页面 loadContent 后调用 setColorMode 下发用户选择的深浅色模式。
    HarmonyColorModeController.getInstance().bindContext(this.context)

    // Issue #767：首次启动注入当前系统 colorMode，避免一直靠默认值 0 先按亮色启动。
    this.initSystemColorMode()

    // Issue #670 评论 5652500781：共享 Rust diagnostics 必须在应用生命周期通知之前初始化，
    // 否则 lifecycle/observer 链产生的第一批结构化事件会天然漏掉 create。
    // hilog 作为系统实时输出可以在前面；Harmony 起 ArkTS → NAPI → C ABI → Rust。
    this.initDiagnostics()

    this.lifecycle.notifyCreate()

    // 接续恢复：检查 launchReason === CONTINUATION，从 want.parameters 读取 payload。
    if (launchParam.launchReason === AbilityConstant.LaunchReason.CONTINUATION) {
      this.restoreContinuationFromWant(want)
    }
  }

  // Issue #767：读取当前系统 colorMode 并注入 HarmonyThemeAdapter。
  // 使用 this.context.config.colorMode 获取系统当前的深浅色模式。
  private initSystemColorMode(): void {
    try {
      const config = this.context.config
      if (config && config.colorMode !== undefined) {
        HarmonyThemeAdapter.getInstance().initSystemColorMode(config.colorMode)
        hilog.info(0x0000, 'EntryAbility', 'initSystemColorMode: colorMode=%{public}d', config.colorMode)
      }
    } catch (e) {
      hilog.error(0x0000, 'EntryAbility', 'initSystemColorMode exception: %{public}s', String(e))
    }
  }

  // 初始化 Rust 诊断后端 — Issue #670 评论 5652119660 修复 2e。
  // 在 EntryAbility.onCreate() 中调用，把目录/设备/版本/locale/timezone 传进 Rust。
  // 失败只记 hilog，不阻塞应用启动。
  private initDiagnostics(): void {
    try {
      const cacheDir = this.context.cacheDir
      const logDir = `${cacheDir}/log`
      const deviceId = HarmonyDeviceIdentity.getOrCreateDeviceId(this.context)
      const identity = this.getBuildIdentity()
      const appVersion = identity.appVersion
      const buildKey = identity.buildKey
      // Issue #670 评论 5652500781：i18n.getSystemTimezone() 不存在，
      // 官方时区接口是 i18n.getTimeZone()，默认返回当前系统时区，用 getID() 取 IANA 时区 ID。
      // locale 用新文档推荐的 i18n.System.getSystemLanguage()。
      const locale = i18n.System.getSystemLanguage()
      const timezone = i18n.getTimeZone().getID()
      const result = NativeWriterCoreBridge.getInstance().initDiagnostics(
        logDir, deviceId, appVersion, buildKey, locale, timezone
      )
      if (result !== 0) {
        hilog.error(0x0000, 'EntryAbility', 'initDiagnostics failed with code %{public}d', result)
      }
    } catch (e) {
      hilog.error(0x0000, 'EntryAbility', 'initDiagnostics exception: %{public}s', String(e))
    }
  }

  // 获取构建身份 — Issue #670 评论 5652500781：一次读取完整 bundle info，
  // 同时给出 appVersion（versionName）和 buildKey（harmony-v{versionCode}-{versionName}）。
  // buildKey 用于 writer_diagnostics 日志文件名 sujian-current-{build_key}.log 和导出 manifest 的 build_identity，
  // 不能留空，否则不同版本的日志无法按构建身份区分。失败返回 'unknown' / 'harmony-unknown'。
  // 以后有 git SHA/正式构建号时再拼进同一个 buildKey。
  private getBuildIdentity(): BuildIdentity {
    try {
      const info = bundleManager.getBundleInfoForSelfSync(
        bundleManager.BundleFlag.GET_BUNDLE_INFO_DEFAULT
      )
      const identity: BuildIdentity = {
        appVersion: info.versionName,
        buildKey: `harmony-v${info.versionCode}-${info.versionName}`
      }
      return identity
    } catch (_) {
      const fallback: BuildIdentity = {
        appVersion: 'unknown',
        buildKey: 'harmony-unknown'
      }
      return fallback
    }
  }

  onDestroy() {
    hilog.info(0x0000, 'EntryAbility', 'onDestroy')
    this.lifecycle.notifyDestroy()
  }

  onWindowStageCreate(windowStage: window.WindowStage) {
    hilog.info(0x0000, 'EntryAbility', 'onWindowStageCreate')

    // Issue #752：先 loadContent 成功再 initialize AdaptiveContext。
    // 实机冷启动 loadContent 完成前 windowRect 可能是 0x0，提前 initialize 会把 0x0 写入布局状态导致白屏。
    windowStage.loadContent('pages/Index', async (err, data) => {
      if (err.code) {
        hilog.error(0x0000, 'EntryAbility', 'Failed to load the content. Cause: %{public}s', JSON.stringify(err))
        return
      }
      hilog.info(0x0000, 'EntryAbility', 'Succeeded in loading the content. Data: %{public}s', JSON.stringify(data))
      // Issue #752：loadContent 成功后才初始化 AdaptiveContext，此时窗口尺寸已就绪。
      hilog.info(0x0000, 'EntryAbility', 'Initializing AdaptiveContext after loadContent succeeded')
      getAdaptiveContext().initialize(windowStage)
      // Issue #767 评论 5831321551：页面 loadContent 成功后标记 pageLoaded，
      // 此时 HarmonyColorModeController 把之前缓存的 desiredMode 真正下发给 UIAbility。
      HarmonyColorModeController.getInstance().markPageLoaded()
    })

    this.lifecycle.notifyForeground()
  }

  // 接续恢复阶段的目标设备窗口初始化：与 onWindowStageCreate 等价的根页面装载。
  // SUPPORT_CONTINUE_PAGE_STACK_KEY=false 禁用了自动页面栈迁移，这里只装载根页 pages/Index，
  // AppNavigation 首帧 aboutToAppear 时从 NavigationTransactionCoordinator 消费已交付的接续 payload，
  // 由 NavigationTransactionCoordinator.restoreFromContinuation 手动重建 NavPathStack。
  onWindowStageRestore(windowStage: window.WindowStage) {
    hilog.info(0x0000, 'EntryAbility', 'onWindowStageRestore')

    // Issue #752：与 onWindowStageCreate 等价，先 loadContent 成功再 initialize AdaptiveContext。
    windowStage.loadContent('pages/Index', async (err, data) => {
      if (err.code) {
        hilog.error(0x0000, 'EntryAbility', 'Failed to load the content on restore. Cause: %{public}s', JSON.stringify(err))
        return
      }
      hilog.info(0x0000, 'EntryAbility', 'Succeeded in loading the content on restore. Data: %{public}s', JSON.stringify(data))
      // Issue #752：loadContent 成功后才初始化 AdaptiveContext，此时窗口尺寸已就绪。
      hilog.info(0x0000, 'EntryAbility', 'Initializing AdaptiveContext after loadContent succeeded on restore')
      getAdaptiveContext().initialize(windowStage)
      // Issue #767 评论 5831321551：页面 loadContent 成功后标记 pageLoaded，
      // 此时 HarmonyColorModeController 把之前缓存的 desiredMode 真正下发给 UIAbility。
      HarmonyColorModeController.getInstance().markPageLoaded()
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

  // Issue #767 评论 5831067980：系统配置变更回调（深色模式切换等）。
  // 读取 newConfig.colorMode 并转给 HarmonyThemeAdapter，
  // 只在 appearanceMode = system 时跟随系统切换（该逻辑在 HarmonyThemeAdapter.onConfigurationUpdate 中处理）。
  // colorMode 是可选字段，必须先判 undefined，不能直接当 number 传。
  onConfigurationUpdate(newConfig: Configuration): void {
    if (newConfig.colorMode === undefined) {
      hilog.info(0x0000, 'EntryAbility', 'onConfigurationUpdate: colorMode is undefined, skip')
      return
    }
    hilog.info(0x0000, 'EntryAbility', 'onConfigurationUpdate: colorMode=%{public}d', newConfig.colorMode)
    try {
      HarmonyThemeAdapter.getInstance().onConfigurationUpdate(newConfig.colorMode)
    } catch (e) {
      hilog.error(0x0000, 'EntryAbility', 'onConfigurationUpdate exception: %{public}s', String(e))
    }
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
  // 再交 NavigationTransactionCoordinator.restoreFromContinuation() 手动重建 NavPathStack
  // （SUPPORT_CONTINUE_PAGE_STACK_KEY=false 禁用了自动页面栈迁移）。
  private restoreContinuationFromWant(want: Want): void {
    const parameters: Record<string, Object> | undefined = want.parameters
    if (parameters === undefined || parameters === null) {
      return
    }
    const restored: ContinuationPayload | null = this.continuationService.restoreFromWant(parameters)
    if (restored !== null) {
      hilog.info(0x0000, 'EntryAbility', 'Continuation restored: projectId=%{public}s', restored.projectId ?? '')
      NavigationTransactionCoordinator.getInstance().restoreFromContinuation(restored)
    }
  }
}
