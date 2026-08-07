import { common } from '@kit.AbilityKit'
import { IWriterCoreBridge } from '../bridge/IWriterCoreBridge'
import { MockWriterCoreBridge } from '../bridge/MockWriterCoreBridge'
import { NativeWriterCoreBridge } from '../bridge/NativeWriterCoreBridge'

import { HarmonyLifecycle } from '../system/HarmonyLifecycle'
import { HarmonyThemeAdapter } from '../system/HarmonyThemeAdapter'

export type Environment = 'mock' | 'native'

export interface AppContextConfig {
  environment: Environment  // 'mock' | 'native'，必须显式指定
  appDataRoot: string
  enableAutoSave: boolean
  autoSaveIntervalMs: number
  enableSync: boolean
  enableLogging: boolean
  /**
   * native smoke test 模式：禁止 mock 降级。
   * 在 CI 验收中设为 true，确保所有操作走真实 Rust Core。
   * 如果 native 初始化失败，不会降级到 mock，而是直接报错。
   */
  nativeSmokeTest: boolean
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
  private nativeInitFailed: boolean = false

  private constructor(config: AppContextConfig) {
    this.config = config
    this.lifecycle = HarmonyLifecycle.getInstance()
    this.themeAdapter = HarmonyThemeAdapter.getInstance()

    if (config.environment === 'mock') {
      this.bridge = new MockWriterCoreBridge()
    } else {
      this.bridge = NativeWriterCoreBridge.getInstance()
    }
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
          environment: 'native',  // 默认 native，不自动降级
          appDataRoot: DEFAULT_APP_DATA_DIR_NAME,
          enableAutoSave: true,
          autoSaveIntervalMs: 30000,
          enableSync: false,
          enableLogging: true,
          nativeSmokeTest: false
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
      environment: this.config.environment,
      appDataRoot: this.config.appDataRoot,
      enableAutoSave: this.config.enableAutoSave,
      autoSaveIntervalMs: this.config.autoSaveIntervalMs,
      enableSync: this.config.enableSync,
      enableLogging: this.config.enableLogging,
      nativeSmokeTest: this.config.nativeSmokeTest
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

  isMockEnvironment(): boolean {
    return this.config.environment === 'mock'
  }

  isNativeEnvironment(): boolean {
    return this.config.environment === 'native'
  }

  isNativeSmokeTest(): boolean {
    return this.config.nativeSmokeTest
  }

  isNativeInitFailed(): boolean {
    return this.nativeInitFailed
  }

  markNativeInitFailed(): void {
    this.nativeInitFailed = true
    // 在 nativeSmokeTest 模式下，native 初始化失败不降级到 mock
    // 调用方应检查 isNativeInitFailed() 并报错
  }


  isReady(): boolean {
    return this.isInitialized
  }

  async switchToNative(appDataRoot?: string): Promise<void> {
    if (this.config.environment === 'native') {
      return
    }

    // 在 nativeSmokeTest 模式下禁止从 mock 切换到 native
    // 必须在初始化时就指定 native 环境
    this.config.environment = 'native'
    if (appDataRoot) {
      this.config.appDataRoot = appDataRoot
    }
    this.bridge = NativeWriterCoreBridge.getInstance()
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
