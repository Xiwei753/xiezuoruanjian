// TabletWindowPolicy — 普通平板专用横屏窗口政策。
//
// 判定逻辑：
// 1. deviceType === 'tablet' 是普通平板候选
// 2. display.isFoldable() 排除所有可折叠设备（阔折叠、普通折叠、三折叠）
// 3. display.getDefaultDisplaySync() 宽高比 >= 1.25 区分明显宽于正方形的常规平板
// 4. 仅命中普通平板时强制横屏 + 铺满屏幕 + 隐藏系统栏
//
// 普通手机、阔直板、阔折叠、其他折叠形态不执行强制政策。
// 读取设备特征失败默认不强制。
// 只算一次设备能力，窗口变化只重新判实际可用尺寸。

import window from '@ohos.window'
import display from '@ohos.display'
import deviceInfo from '@ohos.deviceInfo'
import hilog from '@ohos.hilog'

const TAG = 'TabletWindowPolicy'
const DOMAIN = 0x0000

// 宽高比阈值：max(width, height) / min(width, height) >= 1.25 视为明显宽于正方形
const ASPECT_RATIO_THRESHOLD: number = 1.25

export class TabletWindowPolicy {
  // 设备能力只算一次
  private static deviceTypeResolved: boolean = false
  private static cachedDeviceType: string = ''
  private static cachedIsFoldable: boolean = false
  private static cachedIsDedicatedLandscapeTablet: boolean = false

  /**
   * 判断当前设备是否是专用横屏平板。
   *
   * 判定条件（全部满足）：
   * - deviceType === 'tablet'
   * - display.isFoldable() === false（排除所有可折叠设备）
   * - display 宽高比 >= 1.25（排除接近正方形的设备）
   *
   * 设备特征只读取一次并缓存；窗口尺寸变化不影响此判定。
   * 读取设备特征失败时默认返回 false（不强制）。
   */
  static isDedicatedLandscapeTablet(): boolean {
    if (TabletWindowPolicy.deviceTypeResolved) {
      return TabletWindowPolicy.cachedIsDedicatedLandscapeTablet
    }

    try {
      TabletWindowPolicy.cachedDeviceType = deviceInfo.deviceType
      hilog.info(DOMAIN, TAG, 'deviceType: %{public}s', TabletWindowPolicy.cachedDeviceType)
    } catch (err) {
      hilog.error(DOMAIN, TAG, `Failed to read deviceType: ${JSON.stringify(err)}`)
      TabletWindowPolicy.cachedDeviceType = ''
    }

    try {
      TabletWindowPolicy.cachedIsFoldable = display.isFoldable()
      hilog.info(DOMAIN, TAG, 'isFoldable: %{public}s', `${TabletWindowPolicy.cachedIsFoldable}`)
    } catch (err) {
      hilog.error(DOMAIN, TAG, `Failed to read isFoldable: ${JSON.stringify(err)}`)
      // 读取失败默认视为可折叠（保守，不强制）
      TabletWindowPolicy.cachedIsFoldable = true
    }

    // 候选条件 1：deviceType === 'tablet'
    const isTabletCandidate: boolean = TabletWindowPolicy.cachedDeviceType === 'tablet'
    // 候选条件 2：不是可折叠设备
    const isNotFoldable: boolean = !TabletWindowPolicy.cachedIsFoldable

    if (isTabletCandidate && isNotFoldable) {
      // 候选条件 3：宽高比 >= 1.25
      try {
        const displayInfo = display.getDefaultDisplaySync()
        const width: number = displayInfo.width
        const height: number = displayInfo.height
        const maxDim: number = Math.max(width, height)
        const minDim: number = Math.min(width, height)
        if (minDim > 0) {
          const aspectRatio: number = maxDim / minDim
          hilog.info(DOMAIN, TAG, 'Display: %{public}sx%{public}s, aspectRatio: %{public}s', `${width}`, `${height}`, `${aspectRatio}`)
          TabletWindowPolicy.cachedIsDedicatedLandscapeTablet = aspectRatio >= ASPECT_RATIO_THRESHOLD
        } else {
          hilog.info(DOMAIN, TAG, 'Display dimensions invalid, not forcing landscape')
          TabletWindowPolicy.cachedIsDedicatedLandscapeTablet = false
        }
      } catch (err) {
        hilog.error(DOMAIN, TAG, `Failed to read display info: ${JSON.stringify(err)}`)
        TabletWindowPolicy.cachedIsDedicatedLandscapeTablet = false
      }
    } else {
      hilog.info(DOMAIN, TAG, 'Not a dedicated landscape tablet candidate: tablet=%{public}s, notFoldable=%{public}s', `${isTabletCandidate}`, `${isNotFoldable}`)
      TabletWindowPolicy.cachedIsDedicatedLandscapeTablet = false
    }

    TabletWindowPolicy.deviceTypeResolved = true
    hilog.info(DOMAIN, TAG, 'isDedicatedLandscapeTablet: %{public}s', `${TabletWindowPolicy.cachedIsDedicatedLandscapeTablet}`)
    return TabletWindowPolicy.cachedIsDedicatedLandscapeTablet
  }

  /**
   * 在主窗口上应用平板专用横屏政策。
   *
   * 仅当 isDedicatedLandscapeTablet() 返回 true 时执行：
   * - setPreferredOrientation(window.Orientation.LANDSCAPE) 强制横屏
   * - setWindowLayoutFullScreen(true) 铺满屏幕
   * - setWindowSystemBarEnable([]) 隐藏状态栏和系统导航栏
   *
   * 非专用横屏平板不执行任何操作。
   * 应在 EntryAbility.onWindowStageCreate / onWindowStageRestore 中
   * loadContent 之前调用。
   */
  static async applyTabletWindowPolicy(mainWindow: window.Window): Promise<void> {
    if (!TabletWindowPolicy.isDedicatedLandscapeTablet()) {
      hilog.info(DOMAIN, TAG, 'Not a dedicated landscape tablet, skipping window policy')
      return
    }

    // 分屏 / 悬浮 / 最大化下窗口不是独占全屏，不套用强制横屏 + 隐藏系统栏；
    // 这些形态按系统给出的真实可用窗口布局（见 AdaptiveContext 的 windowSizeChange）。
    if (!TabletWindowPolicy.isExclusiveFullScreenState(mainWindow)) {
      hilog.info(DOMAIN, TAG, 'Window is not in exclusive fullscreen, skipping landscape policy')
      return
    }

    try {
      await mainWindow.setPreferredOrientation(window.Orientation.LANDSCAPE)
      hilog.info(DOMAIN, TAG, 'Set preferred orientation to LANDSCAPE')
    } catch (err) {
      hilog.error(DOMAIN, TAG, `Failed to set preferred orientation: ${JSON.stringify(err)}`)
    }

    try {
      await mainWindow.setWindowLayoutFullScreen(true)
      hilog.info(DOMAIN, TAG, 'Set window layout full screen')
    } catch (err) {
      hilog.error(DOMAIN, TAG, `Failed to set window layout full screen: ${JSON.stringify(err)}`)
    }

    try {
      await mainWindow.setWindowSystemBarEnable([])
      hilog.info(DOMAIN, TAG, 'Hid system bars (status bar and navigation bar)')
    } catch (err) {
      hilog.error(DOMAIN, TAG, `Failed to hide system bars: ${JSON.stringify(err)}`)
    }
  }

  /**
   * 恢复窗口配置（从专用横屏平板状态退出时调用）。
   *
   * 恢复操作：
   * - setPreferredOrientation(window.Orientation.UNSPECIFIED) 恢复默认方向
   * - setWindowLayoutFullScreen(false) 恢复非全屏布局
   * - setWindowSystemBarEnable(['status', 'navigation']) 恢复状态栏和导航栏
   *
   * 非专用横屏平板不执行任何操作。
   */
  static async restoreWindowPolicy(mainWindow: window.Window): Promise<void> {
    if (!TabletWindowPolicy.isDedicatedLandscapeTablet()) {
      return
    }

    try {
      await mainWindow.setPreferredOrientation(window.Orientation.UNSPECIFIED)
      hilog.info(DOMAIN, TAG, 'Restored preferred orientation to UNSPECIFIED')
    } catch (err) {
      hilog.error(DOMAIN, TAG, `Failed to restore preferred orientation: ${JSON.stringify(err)}`)
    }

    try {
      await mainWindow.setWindowLayoutFullScreen(false)
      hilog.info(DOMAIN, TAG, 'Restored window layout to non-full-screen')
    } catch (err) {
      hilog.error(DOMAIN, TAG, `Failed to restore window layout full screen: ${JSON.stringify(err)}`)
    }

    try {
      await mainWindow.setWindowSystemBarEnable(['status', 'navigation'])
      hilog.info(DOMAIN, TAG, 'Restored system bars (status and navigation)')
    } catch (err) {
      hilog.error(DOMAIN, TAG, `Failed to restore system bars: ${JSON.stringify(err)}`)
    }
  }

  /**
   * 套用前判断窗口是否处于独占全屏。
   *
   * WindowStatusType：UNDEFINED=0, FULL_SCREEN=1, MAXIMIZE=2, MINIMIZE=3, FLOATING=4, SPLIT_SCREEN=5。
   * 只有 FULL_SCREEN 是独占全屏；分屏 / 悬浮 / 最大化都会保留系统栏与其它窗口，
   * 套上强制横屏 + 隐藏系统栏等于把全屏 UI 硬压到一块非全屏窗口上。
   * UNDEFINED 是冷启动窗口状态还没就绪，不套用强制政策（避免在未确认全屏时
   * 强制横屏 + 隐藏系统栏）；读取失败（窗口尚未创建）保守返回 true 不阻断。
   */
  private static isExclusiveFullScreenState(mainWindow: window.Window): boolean {
    try {
      const status: window.WindowStatusType = mainWindow.getWindowStatus()
      return status === window.WindowStatusType.FULL_SCREEN
    } catch (err) {
      hilog.info(DOMAIN, TAG, `getWindowStatus failed, assume exclusive fullscreen: ${JSON.stringify(err)}`)
      return true
    }
  }
}
