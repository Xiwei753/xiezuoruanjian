import { common } from '@kit.AbilityKit'
import { IWriterCoreBridge } from '../bridge/IWriterCoreBridge'
import { MockWriterCoreBridge } from '../bridge/MockWriterCoreBridge'
import { NativeWriterCoreBridge } from '../bridge/NativeWriterCoreBridge'
import { WorkspaceFileAccess, MockFileAccess, NativeFileAccess } from '../system/HarmonyFileAccess'
import { KVStore, MockKVStore, NativeKVStore } from '../system/HarmonySecureStorage'
import { NetworkManager, MockNetworkManager, NativeNetworkManager } from '../system/HarmonyNetworkState'
import { HarmonyLifecycle } from '../system/HarmonyLifecycle'
import { HarmonyThemeAdapter } from '../system/HarmonyThemeAdapter'

export type Environment = 'mock' | 'native'

export interface AppContextConfig {
  environment: Environment
  workspacePath: string
  enableAutoSave: boolean
  autoSaveIntervalMs: number
  enableSync: boolean
  enableLogging: boolean
}

const DEFAULT_WORKSPACE_DIR_NAME = 'writer-workspace'

export class AppContext {
  private static instance: AppContext
  private config: AppContextConfig
  private bridge: IWriterCoreBridge
  private fileAccess: WorkspaceFileAccess
  private kvStore: KVStore
  private networkManager: NetworkManager
  private lifecycle: HarmonyLifecycle
  private themeAdapter: HarmonyThemeAdapter
  private isInitialized: boolean = false
  private abilityContext: common.UIAbilityContext | null = null

  private constructor(config: AppContextConfig) {
    this.config = config
    this.lifecycle = HarmonyLifecycle.getInstance()
    this.themeAdapter = HarmonyThemeAdapter.getInstance()

    if (config.environment === 'mock') {
      this.bridge = new MockWriterCoreBridge()
      this.fileAccess = new MockFileAccess()
      this.kvStore = new MockKVStore()
      this.networkManager = new MockNetworkManager()
    } else {
      this.bridge = NativeWriterCoreBridge.getInstance()
      this.fileAccess = new NativeFileAccess()
      this.kvStore = new NativeKVStore()
      this.networkManager = new NativeNetworkManager()
    }
  }

  setAbilityContext(context: common.UIAbilityContext): void {
    this.abilityContext = context
    const filesDir = context.filesDir
    this.config.workspacePath = `${filesDir}/${DEFAULT_WORKSPACE_DIR_NAME}`
  }

  static getInstance(config?: AppContextConfig): AppContext {
    if (!AppContext.instance) {
      if (!config) {
        config = {
          environment: 'native',
          workspacePath: DEFAULT_WORKSPACE_DIR_NAME,
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
      environment: this.config.environment,
      workspacePath: this.config.workspacePath,
      enableAutoSave: this.config.enableAutoSave,
      autoSaveIntervalMs: this.config.autoSaveIntervalMs,
      enableSync: this.config.enableSync,
      enableLogging: this.config.enableLogging
    }
    return c
  }

  getWorkspacePath(): string {
    return this.config.workspacePath
  }

  getBridge(): IWriterCoreBridge {
    return this.bridge
  }

  getFileAccess(): WorkspaceFileAccess {
    return this.fileAccess
  }

  getKVStore(): KVStore {
    return this.kvStore
  }

  getNetworkManager(): NetworkManager {
    return this.networkManager
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

  isReady(): boolean {
    return this.isInitialized
  }

  async switchToNative(workspacePath?: string): Promise<void> {
    if (this.config.environment === 'native') {
      return
    }

    this.config.environment = 'native'
    if (workspacePath) {
      this.config.workspacePath = workspacePath
    }
    this.bridge = NativeWriterCoreBridge.getInstance()
    this.fileAccess = new NativeFileAccess()
    this.kvStore = new NativeKVStore()
    this.networkManager = new NativeNetworkManager()
  }

  async switchToMock(): Promise<void> {
    if (this.config.environment === 'mock') {
      return
    }

    this.config.environment = 'mock'
    this.bridge = new MockWriterCoreBridge()
    this.fileAccess = new MockFileAccess()
    this.kvStore = new MockKVStore()
    this.networkManager = new MockNetworkManager()
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

export function getWorkspacePath(): string {
  return getAppContext().getWorkspacePath()
}

export function getFileAccess(): WorkspaceFileAccess {
  return getAppContext().getFileAccess()
}

export function getKVStore(): KVStore {
  return getAppContext().getKVStore()
}

export function getNetworkManager(): NetworkManager {
  return getAppContext().getNetworkManager()
}

export function getLifecycle(): HarmonyLifecycle {
  return getAppContext().getLifecycle()
}

export function getThemeAdapter(): HarmonyThemeAdapter {
  return getAppContext().getThemeAdapter()
}
