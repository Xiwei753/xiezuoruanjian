import { common } from '@kit.AbilityKit'
import { IWriterCoreBridge } from '../corebridge/IWriterCoreBridge'
import { NativeWriterCoreBridge } from '../corebridge/NativeWriterCoreBridge'

import { HarmonyLifecycle } from '../platform/lifecycle/HarmonyLifecycle'
import { HarmonyThemeAdapter } from '../ui/theme/HarmonyThemeAdapter'
import { CapabilityRegistry } from '../platform/capability/CapabilityRegistry'
import { InputDeviceService } from '../platform/input/InputDeviceService'
import { GripPostureService } from '../platform/awareness/GripPostureService'
import { ShoulderSurfingService } from '../platform/privacy/ShoulderSurfingService'
import { WindowPrivacyService } from '../platform/privacy/WindowPrivacyService'
import { HarmonyShareService } from '../platform/share/HarmonyShareService'

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
  private capabilityRegistry: CapabilityRegistry
  private isInitialized: boolean = false
  private abilityContext: common.UIAbilityContext | null = null

  private constructor(config: AppContextConfig) {
    this.config = config
    this.lifecycle = HarmonyLifecycle.getInstance()
    this.themeAdapter = HarmonyThemeAdapter.getInstance()
    this.capabilityRegistry = CapabilityRegistry.getInstance()
    // 生产上下文只装配 Native Core，不再有 mock/native 切换。
    this.bridge = NativeWriterCoreBridge.getInstance()
  }

  setAbilityContext(context: common.UIAbilityContext): void {
    this.abilityContext = context
    const filesDir = context.filesDir
    this.config.appDataRoot = `${filesDir}/${DEFAULT_APP_DATA_DIR_NAME}`

    // 把 UIAbilityContext 注入 WindowPrivacyService，使其能获取 window 实例。
    WindowPrivacyService.getInstance().setAbilityContext(context)

    // 把 UIAbilityContext 注入 HarmonyShareService（进而注入 SystemShareService），
    // 使其能调 systemShare.ShareController.show 和获取 windowId（隔空传送）。
    // feature/page 不直接 import Share Kit，统一通过 HarmonyShareService。
    HarmonyShareService.getInstance().setAbilityContext(context)

    // 启动各平台 service 真实监听，并刷新 CapabilityRegistry 真实探测。
    // 不靠默认 bool 假装能力存在；各 service 报告真实 support 状态。
    this.startPlatformServices()
  }

  // 启动平台系统能力 service 并刷新 CapabilityRegistry。
  // 异步启动；失败不阻塞主流程，各 service 内部已 try/catch。
  private startPlatformServices(): void {
    // 异步启动，不 await（setAbilityContext 是同步入口）。
    const inputDevice: InputDeviceService = InputDeviceService.getInstance()
    const grip: GripPostureService = GripPostureService.getInstance()
    const shoulder: ShoulderSurfingService = ShoulderSurfingService.getInstance()

    // 启动设备枚举/监听（异步，不阻塞）。
    inputDevice.startListening().then(() => {
      // 设备列表就绪后刷新 capability（stylus 探测依赖设备表）。
      this.capabilityRegistry.refresh()
    }).catch(() => {
      // 启动失败不伪造状态；refresh 仍执行以填充其余能力。
      this.capabilityRegistry.refresh()
    })

    // 启动握姿感知监听（异步）。
    grip.startListening().catch(() => {})

    // 启动防窥检测监听（异步）。
    shoulder.startListening().catch(() => {})

    // 首次刷新：填充不依赖异步监听的能力（shoulderSurfing/windowPrivacy/continuation/share）。
    this.capabilityRegistry.refresh()
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

  getCapabilityRegistry(): CapabilityRegistry {
    return this.capabilityRegistry
  }

  isReady(): boolean {
    return this.isInitialized
  }

  dispose(): void {
    this.isInitialized = false
    // 停止平台 service 监听，释放系统资源。
    InputDeviceService.getInstance().stopListening()
    GripPostureService.getInstance().stopListening()
    ShoulderSurfingService.getInstance().stopListening()
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

export function getCapabilityRegistry(): CapabilityRegistry {
  return getAppContext().getCapabilityRegistry()
}
