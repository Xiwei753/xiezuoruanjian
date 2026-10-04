// LocalSettingsStore — 端内唯一一份"最近一次落盘的 LocalSettings"广播点。
//
// 背景（Issue #825 复核8 第 3 点）：宽屏写作页和宽屏设置浮层同时活着，
// 两边各持一份 settings 会长期互不认知：
//   - SettingsViewModel.saveSettings() 改了字号 / 主题并落盘，
//     底下活着的 WritingScreen 不知道，正文仍按旧排版渲染；
//   - WritingScreen 只在进页面时 loadSettings() 一次，用户在浮层里改过的
//     其它字段它是不知道的。
// Linux 侧已有 settingsBackend.onSettings_changed 这条实时回流，
// Harmony 之前缺同样的语义，这里补上。
//
// 边界：这里只负责"把刚落盘的那份设置广播出去"，不持有 UI 状态、不读磁盘、
// 不做节流。真正的落盘动作仍由各自的 owner 走 IWriterCoreBridge 完成，
// 本 store 只在 saveLocalSettings 成功之后被 publish。

import { LocalSettings } from '../../../corebridge/dto/SettingsDtos'

/** 设置变更监听器：收到刚落盘的那份 LocalSettings */
export type LocalSettingsListener = (settings: LocalSettings) => void

export class LocalSettingsStore {
  private static instance: LocalSettingsStore | null = null

  /** 最近一次 publish 的设置；还没有任何写入时为 null */
  private latest: LocalSettings | null = null
  private listeners: LocalSettingsListener[] = []

  private constructor() {
  }

  static getInstance(): LocalSettingsStore {
    if (LocalSettingsStore.instance === null) {
      LocalSettingsStore.instance = new LocalSettingsStore()
    }
    return LocalSettingsStore.instance
  }

  /**
   * 广播"刚落盘的那份设置"。只允许在 saveLocalSettings 成功后调用。
   * @param settings 本次实际落盘的内容（不是提交前的旧副本）
   */
  publish(settings: LocalSettings): void {
    this.latest = settings
    // 复制一份再遍历：监听器可能在回调里退订，直接遍历会跳过元素。
    const snapshot: LocalSettingsListener[] = this.listeners.slice()
    for (const listener of snapshot) {
      listener(settings)
    }
  }

  /**
   * 订阅设置变更。
   * @returns 退订函数，调用后不再收到回调
   */
  subscribe(listener: LocalSettingsListener): () => void {
    this.listeners.push(listener)
    return (): void => {
      const index: number = this.listeners.indexOf(listener)
      if (index >= 0) {
        this.listeners.splice(index, 1)
      }
    }
  }

  /** 最近一次 publish 的设置（只读快照引用），没有则 null */
  getLatest(): LocalSettings | null {
    return this.latest
  }
}