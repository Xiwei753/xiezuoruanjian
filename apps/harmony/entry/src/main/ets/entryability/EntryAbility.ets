import { AbilityConstant, UIAbility, Want, Configuration, ConfigurationConstant } from '@kit.AbilityKit'
import hilog from '@ohos.hilog'
import window from '@ohos.window'
import { HarmonyLifecycle } from '../platform/lifecycle/HarmonyLifecycle'
import { getAppContext, getAppDataRoot } from '../app/AppContext'
import { getAdaptiveContext } from '../ui/adaptive/AdaptiveContext'
import { ContinuationEntrypoint } from '../platform/continuity/ContinuationEntrypoint'
import { NativeWriterCoreBridge } from '../corebridge/NativeWriterCoreBridge'
import { HarmonyDeviceIdentity } from '../platform/identity/HarmonyDeviceIdentity'
import { HarmonyThemeAdapter } from '../ui/theme/HarmonyThemeAdapter'
import { HarmonyColorModeController } from '../ui/theme/HarmonyColorModeController'
import { HarmonyCrashEventWatcher } from '../platform/diagnostics/HarmonyCrashEventWatcher'
import { HarmonyDiagnosticsCheckpoint } from '../platform/diagnostics/HarmonyDiagnosticsCheckpoint'
import { HarmonyDiagnosticEvents, DiagnosticField } from '../platform/diagnostics/HarmonyDiagnosticEvents'
import { HarmonyAutoSyncScheduler } from '../feature/settings/presentation/HarmonyAutoSyncScheduler'
import { TabletWindowPolicy } from '../platform/window/TabletWindowPolicy'
import i18n from '@ohos.i18n'
import bundleManager from '@ohos.bundle.bundleManager'

// Issue #670 评论 5652500781：构建身份（命名 interface，避免对象字面量类型）。
interface BuildIdentity {
  appVersion: string
  buildKey: string
}

export default class EntryAbility extends UIAbility {
  private lifecycle: HarmonyLifecycle = HarmonyLifecycle.getInstance()
  private continuationEntrypoint: ContinuationEntrypoint = ContinuationEntrypoint.getInstance()

  onCreate(want: Want, launchParam: AbilityConstant.LaunchParam): void {
    hilog.info(0x0000, 'EntryAbility', 'onCreate')
    getAppContext().setAbilityContext(this.context)

    // Issue #906 评论：共享 Rust diagnostics 必须在记录 ability.created 之前初始化。
    // writer_diagnostics::enqueue 在 initialized=false 时直接丢弃事件，先 init 再记，
    // 这条启动事件才会真正进入 JSONL；否则 NAPI/Rust 层返回 0 也落不了盘。
    // hilog 作为系统实时输出可以在前面；Harmony 起 ArkTS → NAPI → C ABI → Rust。
    this.initDiagnostics()
    HarmonyDiagnosticEvents.record('Info', 'App', 'ability.created', 'EntryAbility', [])

    // Issue #767 评论 5831321551：把 UIAbilityContext 注入 HarmonyColorModeController，
    // 使其能在页面 loadContent 后调用 setColorMode 下发用户选择的深浅色模式。
    HarmonyColorModeController.getInstance().bindContext(this.context)

    // Issue #767：首次启动注入当前系统 colorMode，避免一直靠默认值 0 先按亮色启动。
    this.initSystemColorMode()

    // Issue #773 评论 5844589231：订阅系统 HiAppEvent 崩溃事件，
    // 在下一次启动时把上一轮 crash fault log 补进 diagnostics/crashes/。
    HarmonyCrashEventWatcher.getInstance().init(this.context)

    // Issue #773 评论 5845398115：初始化诊断 checkpoint，首次启动立即排一轮，不等 5 秒 timer。
    // 消除"首启 5 秒空窗期内闪退文件管理里只有 live 目录没有 latest.zip"的问题。
    const checkpoint = HarmonyDiagnosticsCheckpoint.getInstance()
    checkpoint.init(this.context)
    checkpoint.persistRuntimeSnapshot()
    checkpoint.requestCheckpoint()

    this.lifecycle.notifyCreate()

    // Issue #925：Ability 级装配唯一自动同步调度器——注册生命周期与网络恢复监听，
    // 并从 Core 载入已提交配置，保证不打开设置页也能按配置前台自动同步。
    // attach 只做装配与到期判断，不在装配时凭空发同步。
    HarmonyAutoSyncScheduler.getInstance().attach(this.lifecycle).catch((e: Object): void => {
      hilog.warn(0x0000, 'EntryAbility', 'auto sync scheduler attach failed: %{public}s', String(e))
    })

    // 接续恢复：检查 launchReason === CONTINUATION，从 want.parameters 读取 payload。
    if (launchParam.launchReason === AbilityConstant.LaunchReason.CONTINUATION) {
      this.continuationEntrypoint.restoreFromWant(want)
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
  // Issue #773 评论 5844589231：logDir 从 cacheDir/log 改为 filesDir/diagnostics/live/log。
  // cacheDir 属于应用私有缓存，文件管理器看不到且系统可清理；filesDir 是持久化目录，进程崩溃后文件仍然存在。
  // Issue #911：把 appDataRoot 一并下发，Rust 启动时从同一 Core LocalSettings 决定是否启用日志，
  // 而不是先强开、等用户打开设置页才补关。appDataRoot 此时已是 filesDir/sujian-data
  // （setAbilityContext 已设置），调用顺序仍是先 initDiagnostics 再 ability.created。
  private initDiagnostics(): void {
    try {
      const filesDir = this.context.filesDir
      const logDir = `${filesDir}/diagnostics/live/log`
      const appDataRoot = getAppDataRoot()
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
        logDir, appDataRoot, deviceId, appVersion, buildKey, locale, timezone
      )
      if (result !== 0) {
        hilog.error(0x0000, 'EntryAbility', 'initDiagnostics failed with code %{public}d', result)
        // Issue #906：init 失败时日志后端未就绪，这条事件大概率落不了盘（enqueue 在未初始化时丢弃），
        // 仅作 best-effort 记录，不视为已持久化。
        const resultField: DiagnosticField = { key: 'result', value: String(result) }
        HarmonyDiagnosticEvents.record('Error', 'App', 'ability.diagnostics_init_failed', 'EntryAbility', [resultField])
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

  // Issue #773 评论 5845398115：Ability 销毁时解绑诊断 checkpoint 和 crash watcher，
  // 避免同一进程里 UIAbility 重建后重复订阅 observer / 重复开 timer / 重复注册 HiAppEvent watcher。
  onDestroy() {
    hilog.info(0x0000, 'EntryAbility', 'onDestroy')
    HarmonyDiagnosticEvents.record('Info', 'App', 'ability.destroyed', 'EntryAbility', [])
    HarmonyDiagnosticsCheckpoint.getInstance().destroy()
    HarmonyCrashEventWatcher.getInstance().destroy()
    // Issue #925：Ability 级解绑自动同步调度器（移除生命周期与网络恢复监听）。
    HarmonyAutoSyncScheduler.getInstance().detach(this.lifecycle)
    this.lifecycle.notifyDestroy()
  }

  onWindowStageCreate(windowStage: window.WindowStage) {
    hilog.info(0x0000, 'EntryAbility', 'onWindowStageCreate')
    // Issue #906：窗口舞台创建事件，放在异步 loadContent 之前，保证首帧前就有记录。
    HarmonyDiagnosticEvents.record('Info', 'App', 'ability.window_stage_created', 'EntryAbility', [])

    // Issue #880：在 loadContent 之前应用平板专用横屏窗口政策。
    // applyTabletWindowPolicy 是异步的，必须 await 完成后再 loadContent，
    // 否则方向和系统栏的设置可能晚于页面加载，导致首帧方向错误。
    this.initWindowPolicy(windowStage).then(() => {
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
    })

    this.lifecycle.notifyForeground()
  }

  // 接续恢复阶段的目标设备窗口初始化：与 onWindowStageCreate 等价的根页面装载。
  // SUPPORT_CONTINUE_PAGE_STACK_KEY=false 禁用了自动页面栈迁移，这里只装载根页 pages/Index，
  // AppNavigation 在 aboutToAppear 注册恢复回调，由 NavigationTransactionCoordinator 转交
  // AppNavigation.applyContinuation 手动重建 NavPathStack；回调或 homeStack 未就绪时先暂存，
  // 待 PrimaryTabShell 注册 homeStack 后由 tryConsumePendingRestore() 消费。
  onWindowStageRestore(windowStage: window.WindowStage) {
    hilog.info(0x0000, 'EntryAbility', 'onWindowStageRestore')

    // Issue #880：与 onWindowStageCreate 等价，在 loadContent 之前应用平板专用横屏窗口政策。
    this.initWindowPolicy(windowStage).then(() => {
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
    })

    this.lifecycle.notifyForeground()
  }

  onWindowStageDestroy() {
    hilog.info(0x0000, 'EntryAbility', 'onWindowStageDestroy')
    // Issue #927：与 onBackground 可能连续触发，这里仍只派发一次全局事件；
    // 后台保存事务的重复由 AutoSaveLifecycleObserver 的 in-flight 标志在 observer 层去重，
    // 不删除任何生命周期事件。保存由平台短时任务托管，这里不 await 保存 Promise。
    this.lifecycle.notifyBackground()
  }

  onForeground() {
    hilog.info(0x0000, 'EntryAbility', 'onForeground')
    HarmonyDiagnosticEvents.record('Info', 'App', 'ability.foreground', 'EntryAbility', [])
    this.lifecycle.notifyForeground()
  }

  onBackground() {
    hilog.info(0x0000, 'EntryAbility', 'onBackground')
    HarmonyDiagnosticEvents.record('Info', 'App', 'ability.background', 'EntryAbility', [])
    // Issue #927：退后台保存由 AutoSaveLifecycleObserver → HarmonyBackgroundSaveTask.runSave()
    // 申请短时任务托管（申请到 requestId 后才开始异步落盘）。这里只派发一次全局事件，
    // 不 await 不受系统等待保证的保存 Promise。
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

  // 应用接续：源设备触发接续时调用。委托给 ContinuationEntrypoint，
  // EntryAbility 仅保留 Ability 生命周期调用。
  onContinue(wantParam: Record<string, Object>): AbilityConstant.OnContinueResult {
    return this.continuationEntrypoint.onContinue(wantParam)
  }

  // 新 want 到达（接续恢复在目标设备的二次入口）。
  onNewWant(want: Want, launchParam: AbilityConstant.LaunchParam): void {
    hilog.info(0x0000, 'EntryAbility', 'onNewWant')
    if (launchParam.launchReason === AbilityConstant.LaunchReason.CONTINUATION) {
      this.continuationEntrypoint.restoreFromWant(want)
    }
  }

  // —— private ——

  // Issue #880：在 loadContent 之前应用平板专用横屏窗口政策。
  // 冷启动（onWindowStageCreate）和接续恢复（onWindowStageRestore）共用同一初始化路径，
  // 避免两者表现不同。applyTabletWindowPolicy 是异步的，必须 await 完成后再 loadContent，
  // 否则方向和系统栏的设置可能晚于页面加载，导致首帧方向错误。
  private async initWindowPolicy(windowStage: window.WindowStage): Promise<void> {
    try {
      const mainWindow: window.Window = windowStage.getMainWindowSync()
      await TabletWindowPolicy.applyTabletWindowPolicy(mainWindow)
    } catch (e) {
      hilog.error(0x0000, 'EntryAbility', 'initWindowPolicy exception: %{public}s', String(e))
    }
  }
}
