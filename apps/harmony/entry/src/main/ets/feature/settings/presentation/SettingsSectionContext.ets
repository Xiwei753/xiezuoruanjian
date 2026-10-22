// SettingsSectionContext — 设置页当前分类与搜索定位意图的只读上下文。
// 全局搜索从设置页发起时把当前分类放进 SearchContext.currentSettingsSection，
// 只影响 Core 排序，不在平台端另做设置结果过滤。
//
// Issue #982：搜索结果跳转设置时，定位意图从「分类」扩展为一次性的
// (section, settingKey)。分类仍用于展开卡片，settingKey 用于展开后滚动到实际设置项并短暂高亮。
// 意图只在设置内容消费后清除，页面尚未创建/数据尚未加载时不会丢。

export interface SettingsLocateRequest {
  section: string
  settingKey: string
}

export class SettingsSectionContext {
  private static currentSection: string = ''
  private static pendingLocate: SettingsLocateRequest | null = null
  private static listeners: Array<(section: string) => void> = []
  private static locateListeners: Array<(section: string, settingKey: string) => void> = []

  static setCurrentSection(section: string): void {
    SettingsSectionContext.currentSection = section
  }

  static getCurrentSection(): string {
    return SettingsSectionContext.currentSection
  }

  /** 搜索结果要求跳到某分类时通知当前设置内容展开；普通页面进入不触发。 */
  static requestSection(section: string): void {
    if (section.length === 0) {
      return
    }
    SettingsSectionContext.currentSection = section
    // 只跳分类的请求取代之前尚未消费的精确设置项定位。
    SettingsSectionContext.pendingLocate = null
    for (const listener of SettingsSectionContext.listeners) {
      listener(section)
    }
    for (const listener of SettingsSectionContext.locateListeners) {
      listener(section, '')
    }
  }

  /**
   * 搜索结果要求定位到具体设置项。
   * settingKey 为空时退化为只展开分类；settingKey 不会在通知时清除，
   * 由 SettingsContent 在真正滚动/高亮（或确认设置项已不存在）后消费。
   */
  static requestLocate(section: string, settingKey: string): void {
    if (settingKey.length === 0) {
      SettingsSectionContext.requestSection(section)
      return
    }
    if (section.length > 0) {
      SettingsSectionContext.currentSection = section
    }
    const request: SettingsLocateRequest = {
      section: section.length > 0 ? section : SettingsSectionContext.currentSection,
      settingKey: settingKey
    }
    SettingsSectionContext.pendingLocate = request
    for (const listener of SettingsSectionContext.locateListeners) {
      listener(request.section, request.settingKey)
    }
  }

  /** 读取尚未消费的定位意图，不改变状态。设置内容创建时可先取一次。 */
  static peekPendingLocate(): SettingsLocateRequest | null {
    return SettingsSectionContext.pendingLocate
  }

  /** 精确设置项已处理（滚动/高亮或确认不存在）后消费；只清除同一个 key，避免吞掉更新的请求。 */
  static consumePendingLocate(settingKey: string): void {
    if (SettingsSectionContext.pendingLocate !== null &&
      SettingsSectionContext.pendingLocate.settingKey === settingKey) {
      SettingsSectionContext.pendingLocate = null
    }
  }

  static subscribe(listener: (section: string) => void): void {
    if (SettingsSectionContext.listeners.indexOf(listener) < 0) {
      SettingsSectionContext.listeners.push(listener)
    }
  }

  static unsubscribe(listener: (section: string) => void): void {
    const index: number = SettingsSectionContext.listeners.indexOf(listener)
    if (index >= 0) {
      SettingsSectionContext.listeners.splice(index, 1)
    }
  }

  static subscribeLocate(listener: (section: string, settingKey: string) => void): void {
    if (SettingsSectionContext.locateListeners.indexOf(listener) < 0) {
      SettingsSectionContext.locateListeners.push(listener)
    }
  }

  static unsubscribeLocate(listener: (section: string, settingKey: string) => void): void {
    const index: number = SettingsSectionContext.locateListeners.indexOf(listener)
    if (index >= 0) {
      SettingsSectionContext.locateListeners.splice(index, 1)
    }
  }
}
