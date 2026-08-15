import { common } from '@kit.AbilityKit'
import { IWriterCoreBridge } from '../corebridge/IWriterCoreBridge'
import { NativeWriterCoreBridge } from '../corebridge/NativeWriterCoreBridge'

import { HarmonyLifecycle } from '../platform/lifecycle/HarmonyLifecycle'
import { HarmonyThemeAdapter } from '../ui/theme/HarmonyThemeAdapter'

export interface AppContextConfig {
  appDataRoot: string
  enableAutoSave: boolean
  autoSaveIntervalMs: number
  enableSync: boolean
  enableLogging: boolean
}

const DEFAULT_APP_DATA_DIR_NAME = 'sujian-data'

export class AppContext {
  private static instance: AppContext
  private config: AppContextConfig
  private bridge: IWriterCoreBridge

  private lifecycle: HarmonyLifecycle
  private themeAdapter: HarmonyThemeAdapter
  private isInitialized: boolean = false
  private abilityContext: common.UIAbilityContext | null = null

  private constructor(config: AppContextConfig) {
    this.config = config
    this.lifecycle = HarmonyLifecycle.getInstance()
    this.themeAdapter = HarmonyThemeAdapter.getInstance()
    // 生产上下文只装配 Native Core，不再有 mock/native 切换。
    this.bridge = NativeWriterCoreBridge.getInstance()
  }

  setAbilityContext(context: common.UIAbilityContext): void {
    this.abilityContext = context
    const filesDir = context.filesDir
    this.config.appDataRoot = `${filesDir}/${DEFAULT_APP_DATA_DIR_NAME}`
  }

  static getInstance(config?: AppContextConfig): AppContext {
    if (!AppContext.instance) {
      if (!config) {
        config = {
          appDataRoot: DEFAULT_APP_DATA_DIR_NAME,
          enableAutoSave: true,
          autoSaveIntervalMs: 30000,
          enableSync: false,
          enableLogging: true
        }
      }
      AppContext.instance = new AppContext(config)
    }
    return AppContext.instance
  }

  static initialize(config: AppContextConfig): AppContext {
    AppContext.instance = new AppContext(config)
    AppContext.instance.isInitialized = true
    return AppContext.instance
  }

  getConfig(): AppContextConfig {
    const c: AppContextConfig = {
      appDataRoot: this.config.appDataRoot,
      enableAutoSave: this.config.enableAutoSave,
      autoSaveIntervalMs: this.config.autoSaveIntervalMs,
      enableSync: this.config.enableSync,
      enableLogging: this.config.enableLogging
    }
    return c
  }

  getAppDataRoot(): string {
    return this.config.appDataRoot
  }

  getBridge(): IWriterCoreBridge {
    return this.bridge
  }

  getLifecycle(): HarmonyLifecycle {
    return this.lifecycle
  }

  getThemeAdapter(): HarmonyThemeAdapter {
    return this.themeAdapter
  }

  isReady(): boolean {
    return this.isInitialized
  }

  dispose(): void {
    this.isInitialized = false
  }
}

export function getAppContext(): AppContext {
  return AppContext.getInstance()
}

export function getBridge(): IWriterCoreBridge {
  return getAppContext().getBridge()
}

export function getAppDataRoot(): string {
  return getAppContext().getAppDataRoot()
}

export function getLifecycle(): HarmonyLifecycle {
  return getAppContext().getLifecycle()
}

export function getThemeAdapter(): HarmonyThemeAdapter {
  return getAppContext().getThemeAdapter()
}
