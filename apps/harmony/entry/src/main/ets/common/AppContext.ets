import { IWriterCoreBridge } from '../bridge/IWriterCoreBridge'
import { MockWriterCoreBridge } from '../bridge/MockWriterCoreBridge'
import { NativeWriterCoreBridge } from '../bridge/NativeWriterCoreBridge'
import { WorkspaceFileAccess, MockFileAccess, NativeFileAccess } from '../system/HarmonyFileAccess'
import { TokenStore, MockTokenStore, NativeTokenStore, EncryptionProvider, MockEncryptionProvider } from '../system/HarmonySecureStorage'
import { NetworkManager, MockNetworkManager, NativeNetworkManager } from '../system/HarmonyNetworkState'
import { HarmonyLifecycle } from '../system/HarmonyLifecycle'
import { HarmonyThemeAdapter } from '../system/HarmonyThemeAdapter'

export type Environment = 'mock' | 'native'

export interface AppContextConfig {
  environment: Environment
  enableAutoSave: boolean
  autoSaveIntervalMs: number
  enableSync: boolean
  enableLogging: boolean
}

export class AppContext {
  private static instance: AppContext
  private config: AppContextConfig
  private bridge: IWriterCoreBridge
  private fileAccess: WorkspaceFileAccess
  private tokenStore: TokenStore
  private encryptionProvider: EncryptionProvider
  private networkManager: NetworkManager
  private lifecycle: HarmonyLifecycle
  private themeAdapter: HarmonyThemeAdapter
  private isInitialized: boolean = false

  private constructor(config: AppContextConfig) {
    this.config = config
    this.lifecycle = HarmonyLifecycle.getInstance()
    this.themeAdapter = HarmonyThemeAdapter.getInstance()

    if (config.environment === 'mock') {
      this.bridge = new MockWriterCoreBridge()
      this.fileAccess = new MockFileAccess()
      this.tokenStore = new MockTokenStore()
      this.encryptionProvider = new MockEncryptionProvider()
      this.networkManager = new MockNetworkManager()
    } else {
      this.bridge = NativeWriterCoreBridge.getInstance()
      this.fileAccess = new NativeFileAccess()
      this.tokenStore = new NativeTokenStore()
      this.encryptionProvider = new MockEncryptionProvider() // TODO: Native encryption
      this.networkManager = new NativeNetworkManager()
    }
  }

  static getInstance(config?: AppContextConfig): AppContext {
    if (!AppContext.instance) {
      if (!config) {
        config = {
          environment: 'mock',
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
    return { ...this.config }
  }

  getBridge(): IWriterCoreBridge {
    return this.bridge
  }

  getFileAccess(): WorkspaceFileAccess {
    return this.fileAccess
  }

  getTokenStore(): TokenStore {
    return this.tokenStore
  }

  getEncryptionProvider(): EncryptionProvider {
    return this.encryptionProvider
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

  async switchToNative(): Promise<void> {
    if (this.config.environment === 'native') {
      return
    }

    this.config.environment = 'native'
    this.bridge = NativeWriterCoreBridge.getInstance()
    this.fileAccess = new NativeFileAccess()
    this.tokenStore = new NativeTokenStore()
    this.networkManager = new NativeNetworkManager()
  }

  async switchToMock(): Promise<void> {
    if (this.config.environment === 'mock') {
      return
    }

    this.config.environment = 'mock'
    this.bridge = new MockWriterCoreBridge()
    this.fileAccess = new MockFileAccess()
    this.tokenStore = new MockTokenStore()
    this.networkManager = new MockNetworkManager()
  }

  dispose(): void {
    this.isInitialized = false
  }
}

// Global convenience functions
export function getAppContext(): AppContext {
  return AppContext.getInstance()
}

export function getBridge(): IWriterCoreBridge {
  return getAppContext().getBridge()
}

export function getFileAccess(): WorkspaceFileAccess {
  return getAppContext().getFileAccess()
}

export function getTokenStore(): TokenStore {
  return getAppContext().getTokenStore()
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
