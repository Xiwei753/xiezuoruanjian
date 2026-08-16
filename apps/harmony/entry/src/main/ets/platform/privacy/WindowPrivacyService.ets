// WindowPrivacyService — 窗口隐私/截图录屏保护服务。
// 与 ShoulderSurfingService 分开：本服务管窗口内容保护（截屏黑屏/录屏屏蔽），
// ShoulderSurfing 管身后有人窥视。
//
// 真实事件源：@kit.ArkUI 的 window 模块。
// setScreenshotProtection(enabled) 调用 win.setSnapshotSkip(enabled)。
// 需要传入 UIAbilityContext 来获取 window 实例。

import { window } from '@kit.ArkUI'
import { common } from '@kit.AbilityKit'

export class WindowPrivacyService {
  private static instance: WindowPrivacyService
  private screenshotProtection: boolean = false
  private screenRecordingProtection: boolean = false
  private abilityContext: common.UIAbilityContext | null = null
  private cachedWindow: window.Window | null = null

  static getInstance(): WindowPrivacyService {
    if (!WindowPrivacyService.instance) {
      WindowPrivacyService.instance = new WindowPrivacyService()
    }
    return WindowPrivacyService.instance
  }

  // 注入 UIAbilityContext，用于获取 window 实例。
  // 由 AppContext.setAbilityContext() 在启动时调用。
  setAbilityContext(context: common.UIAbilityContext): void {
    this.abilityContext = context
    this.cachedWindow = null
  }

  isScreenshotProtectionEnabled(): boolean {
    return this.screenshotProtection
  }

  isScreenRecordingProtectionEnabled(): boolean {
    return this.screenRecordingProtection
  }

  // 查询系统是否支持窗口隐私保护（setSnapshotSkip API）。
  // 通过尝试获取窗口并检查 API 存在性判断。
  isSupported(): boolean {
    return this.abilityContext !== null
  }

  // 设置截屏保护：开启后系统截屏/分享会得到黑屏或隐私遮罩。
  // 调用 win.setSnapshotSkip(enabled)。
  async setScreenshotProtection(enabled: boolean): Promise<void> {
    const win: window.Window | null = await this.getWindow()
    if (win === null) {
      // 无可用窗口；不伪造状态。
      return
    }
    try {
      await win.setSnapshotSkip(enabled)
      this.screenshotProtection = enabled
    } catch (e) {
      // 系统调用失败；保持当前状态不伪造。
    }
  }

  // 设置录屏保护：录屏时同样通过 setSnapshotSkip 跳过窗口内容。
  // HarmonyOS 中 setSnapshotSkip 同时影响截屏和录屏。
  async setScreenRecordingProtection(enabled: boolean): Promise<void> {
    const win: window.Window | null = await this.getWindow()
    if (win === null) {
      return
    }
    try {
      await win.setSnapshotSkip(enabled)
      this.screenRecordingProtection = enabled
    } catch (e) {
      // 系统调用失败；保持当前状态不伪造。
    }
  }

  // —— private ——
  private async getWindow(): Promise<window.Window | null> {
    if (this.cachedWindow !== null) {
      return this.cachedWindow
    }
    if (this.abilityContext === null) {
      return null
    }
    try {
      const win: window.Window = await window.getLastWindow(this.abilityContext)
      this.cachedWindow = win
      return win
    } catch (e) {
      return null
    }
  }
}
