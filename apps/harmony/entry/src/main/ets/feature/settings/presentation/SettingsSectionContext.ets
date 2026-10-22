// SettingsSectionContext — 设置页当前分类的只读上下文。
// 全局搜索从设置页发起时把当前分类放进 SearchContext.currentSettingsSection，
// 只影响 Core 排序，不在平台端另做设置结果过滤。

export class SettingsSectionContext {
  private static currentSection: string = ''
  private static listeners: Array<(section: string) => void> = []

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
    for (const listener of SettingsSectionContext.listeners) {
      listener(section)
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
}
