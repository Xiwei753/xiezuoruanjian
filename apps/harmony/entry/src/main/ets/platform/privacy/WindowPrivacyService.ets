// WindowPrivacyService — 窗口隐私/截图录屏保护服务。
// 与 ShoulderSurfingService 分开：本服务管窗口内容保护（截屏黑屏/录屏屏蔽），
// ShoulderSurfing 管身后有人窥视。

export class WindowPrivacyService {
  private static instance: WindowPrivacyService
  private screenshotProtection: boolean = false
  private screenRecordingProtection: boolean = false

  static getInstance(): WindowPrivacyService {
    if (!WindowPrivacyService.instance) {
      WindowPrivacyService.instance = new WindowPrivacyService()
    }
    return WindowPrivacyService.instance
  }

  isScreenshotProtectionEnabled(): boolean {
    return this.screenshotProtection
  }

  isScreenRecordingProtectionEnabled(): boolean {
    return this.screenRecordingProtection
  }

  // 设置截屏保护：开启后系统截屏/分享会得到黑屏或隐私遮罩。
  setScreenshotProtection(enabled: boolean): void {
    this.screenshotProtection = enabled
  }

  setScreenRecordingProtection(enabled: boolean): void {
    this.screenRecordingProtection = enabled
  }
}
