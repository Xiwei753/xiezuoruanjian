// HarmonyAutoSyncScheduler — 进程唯一自动同步调度器（Issue #925）。
//
// 职责：按 Core 已提交的 SyncConfig 判断"是否到点"，到点才经
// SyncOperationController.performSync()（#923 的唯一同步事务入口，内含保存屏障/会话协调/去重）
// 触发全量同步。调度器不自己发同步请求、不复制同步状态机，也不写第二套链路。
//
// 关键约束：
// - 最近一次成功同步时刻的唯一真源是 Core 持久化状态（app-meta/sync/full_state.local.json，
//   经 NativeSyncBridge.loadFullSyncState() 读取）；ArkTS 侧只做缓存，每次决策前重读 Core。
// - 非成功终态绝不更新 lastSuccess（Core 失败时保留旧 last_success_time），不制造假成功时间。
// - reconfigure 只记录配置、启停前台唤醒 timer，不触发同步；是否 due 一律由 runIfDue 决定，
//   打开设置页不会额外产生一笔同步。
// - 前台 setInterval 只是前台唤醒，不代表能在后台可靠周期执行；后台续跑需系统调度，另行接入。
//
// 桥接依赖（Issue #925 的 Rust FFI / NAPI / decode 工作流并行提供）：
// - corebridge/dto/SyncDtos.ets 的 FullSyncState（snake_case 字段：
//   overall_status / last_attempt_time / last_success_time / failed_targets）；
// - NativeSyncBridge.loadFullSyncState(): Promise<ResultEnvelope<FullSyncState | null>>。
// 尚未落库时按该契约编码，待桥接落地后统一验证。

import hilog from '@ohos.hilog'
import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'
import { FullSyncState, SyncConfig } from '../../../corebridge/dto/SyncDtos'
import { NativeSyncBridge } from '../../../corebridge/native/NativeSyncBridge'
import { NativeCoreModule } from '../../../corebridge/native/NativeCoreModule'
import { HarmonyLifecycle, SyncLifecycleObserver } from '../../../platform/lifecycle/HarmonyLifecycle'
import { NetworkStatusService } from '../../../platform/network/NetworkStatusService'
import { SyncOperationController } from './SyncOperationController'
import { SyncOutcome } from './SyncRunTypes'

const TAG = 'SyncScheduler'
const DOMAIN = 0x0000

/** 自动同步的最小间隔（秒）——与设置页滑块下限/需求一致。 */
const MIN_INTERVAL_SECONDS = 60

/** 秒/毫秒线格式分界：小于该值的 epoch 视为秒（1e11 秒 ≈ 公元 5138 年；毫秒现值约 1.8e12）。 */
const EPOCH_MS_THRESHOLD = 100000000000

export class HarmonyAutoSyncScheduler {
  private static instance: HarmonyAutoSyncScheduler | null = null

  private bridge: NativeSyncBridge = new NativeSyncBridge(new NativeCoreModule())
  /** 最近一次 reconfigure 记录的 Core 已提交配置；null 表示尚未加载。 */
  private config: SyncConfig | null = null
  /** Core 持久化状态中的最近成功同步时刻（毫秒）；null 表示尚无成功记录或当前读不到。 */
  private lastSuccessMs: number | null = null
  /** 进程当前是否前台：仅前台挂周期唤醒 timer。 */
  private isForeground: boolean = false
  /** 前台周期唤醒 timer（只是唤醒，不承诺后台执行）。 */
  private timerId: number = -1
  private timerIntervalMs: number = -1
  /** 单一 running 检查任务——任意时刻至多一个 runIfDue 在推进。 */
  private running: Promise<void> | null = null
  /** Ability 级装配后的生命周期监听（null 表示未装配）。 */
  private observer: SyncLifecycleObserver | null = null
  /** 网络恢复监听注销函数（null 表示未订阅）。 */
  private unsubscribeNetwork: (() => void) | null = null
  /** Native Core 是否已初始化（由唯一初始化路径经 onCoreReady() 通知）。 */
  private coreReady: boolean = false
  /**
   * 配置是否已成功从 Core 读过（或由设置页按 Core 提交结果写入）。
   * 读取失败时保持 false 并在下一次前台/网络恢复时重试，绝不把"读不到配置"
   * 记忆成"自动同步关闭"（Issue #925 复核）。
   */
  private configLoaded: boolean = false

  private constructor() {
  }

  static getInstance(): HarmonyAutoSyncScheduler {
    if (HarmonyAutoSyncScheduler.instance === null) {
      HarmonyAutoSyncScheduler.instance = new HarmonyAutoSyncScheduler()
    }
    return HarmonyAutoSyncScheduler.instance
  }

  /**
   * Ability 级装配（EntryAbility.onCreate）：只注册唯一生命周期监听与网络恢复监听。
   * 不在装配时抢读 Core——NativeWriterCoreBridge.initialize() 由首页初始化路径执行，
   * 未初始化时 loadSyncConfig 必然失败。真实配置的首次读取与到期判断由
   * onCoreReady() 在 Core 初始化成功后触发（Issue #925 复核）。
   * 幂等；重复装配直接返回。
   */
  attach(lifecycle: HarmonyLifecycle): void {
    if (this.observer !== null) {
      return
    }
    this.observer = new SyncLifecycleObserver(
      (): void => {
        this.onForeground()
      },
      (): void => {
        this.onBackground()
      })
    lifecycle.addObserver(this.observer)
    this.unsubscribeNetwork = NetworkStatusService.getInstance().onNetworkRestored((): void => {
      this.onNetworkRestored()
    })
    this.isForeground = lifecycle.isForeground()
    hilog.info(DOMAIN, TAG, 'attach: lifecycle and network listeners registered, awaiting core ready')
  }

  /**
   * Native Core 初始化成功后的唯一通知入口（首页初始化路径在 initialize 成功后调用）。
   * 此时才从 Core 读已提交配置，并按真实前台状态挂前台唤醒、做一次到期判断。
   * 读取失败不当作"自动同步关闭"：configLoaded 保持 false，由后续前台/网络恢复重试。
   */
  async onCoreReady(): Promise<void> {
    this.coreReady = true
    await this.reloadConfigFromCore()
    this.continueForegroundAfterConfig()
  }

  /** Ability 级解绑（EntryAbility.onDestroy）：移除监听并停止前台唤醒。 */
  detach(lifecycle: HarmonyLifecycle): void {
    const observer: SyncLifecycleObserver | null = this.observer
    if (observer === null) {
      return
    }
    lifecycle.removeObserver(observer)
    this.observer = null
    const unsubscribe: (() => void) | null = this.unsubscribeNetwork
    if (unsubscribe !== null) {
      unsubscribe()
      this.unsubscribeNetwork = null
    }
    this.stopForegroundTimer()
    this.isForeground = false
  }

  /** 从 Core 载入当前已提交配置（真源），按同一 reconfigure 语义更新调度状态。 */
  private async reloadConfigFromCore(): Promise<void> {
    try {
      const result: ResultEnvelope<SyncConfig | null> = await this.bridge.loadSyncConfig()
      if (!result.success) {
        // 读取失败不记为"自动同步关闭"：configLoaded 保持 false，后续前台/网络恢复重试。
        hilog.warn(DOMAIN, TAG, 'reloadConfigFromCore failed: errorCode=%{public}s', result.errorCode ?? 'UNKNOWN')
        return
      }
      this.reconfigure(result.data ?? null)
    } catch (e) {
      hilog.warn(DOMAIN, TAG, 'reloadConfigFromCore threw: %{public}s', e instanceof Error ? e.name : 'Unknown')
    }
  }

  /**
   * 按 Core 已提交的配置更新调度状态。
   * 只记录配置并按 enabled && autoSync 启停前台唤醒 timer；不触发同步——
   * 是否 due 由 runIfDue 决定（打开设置页/仅改配置不产生额外同步）。
   * 调用方只传 Core 真源（reloadConfigFromCore 读取结果或设置页持久化成功后的配置），
   * 因此这里同时标记配置已成功加载。
   */
  reconfigure(config: SyncConfig | null): void {
    this.config = config
    this.configLoaded = true
    if (config === null || !config.enabled || !config.autoSync) {
      this.stopForegroundTimer()
      hilog.info(DOMAIN, TAG, 'reconfigure: auto sync inactive, foreground timer stopped')
      return
    }
    hilog.info(DOMAIN, TAG, 'reconfigure: auto sync active, intervalSeconds=%{public}s',
      config.syncIntervalSeconds.toString())
    if (this.isForeground) {
      this.startForegroundTimer()
    }
  }

  /** 进入前台：必要时重试读取配置，然后挂前台唤醒 timer 并立即做一次到期判断。 */
  onForeground(): void {
    this.isForeground = true
    if (!this.configLoaded) {
      // 配置尚未成功读过（Core 未就绪或上次读取失败）：本次前台重试一次真实读取，
      // 不把读取失败记忆成"自动同步关闭"；Core 未就绪时等 onCoreReady()。
      if (!this.coreReady) {
        hilog.info(DOMAIN, TAG, 'onForeground: core not ready, defer to onCoreReady')
        return
      }
      this.reloadConfigFromCore().then((): void => {
        this.continueForegroundAfterConfig()
      })
      return
    }
    this.continueForegroundAfterConfig()
  }

  /** 配置已确定时的前台编排：挂前台唤醒 timer 并立即做一次到期判断。 */
  private continueForegroundAfterConfig(): void {
    if (!this.isForeground) {
      return
    }
    if (!this.isAutoSyncActive()) {
      hilog.info(DOMAIN, TAG, 'foreground: auto sync inactive')
      return
    }
    this.startForegroundTimer()
    hilog.info(DOMAIN, TAG, 'foreground: immediate due check')
    this.runIfDue()
  }

  /** 进入后台：只停止前台唤醒 timer；后台周期执行不由本调度器承诺。 */
  onBackground(): void {
    this.isForeground = false
    this.stopForegroundTimer()
    hilog.info(DOMAIN, TAG, 'onBackground: foreground timer stopped')
  }

  /** 网络恢复：必要时重试读取配置，然后立即做一次到期判断（不等待 timer）。 */
  onNetworkRestored(): void {
    if (!this.configLoaded) {
      if (!this.coreReady) {
        hilog.info(DOMAIN, TAG, 'onNetworkRestored: core not ready, defer to onCoreReady')
        return
      }
      this.reloadConfigFromCore().then((): void => {
        hilog.info(DOMAIN, TAG, 'onNetworkRestored: immediate due check after config reload')
        this.runIfDue()
      })
      return
    }
    hilog.info(DOMAIN, TAG, 'onNetworkRestored: immediate due check')
    this.runIfDue()
  }

  /**
   * 到期才同步：仅当 enabled && autoSync 且
   * now - lastSuccess >= max(60, syncIntervalSeconds) * 1000 时进入
   * SyncOperationController.performSync()。单一 running 任务互斥，重复触发合并为跳过。
   */
  async runIfDue(): Promise<void> {
    if (!this.isAutoSyncActive()) {
      return
    }
    if (this.running !== null) {
      hilog.info(DOMAIN, TAG, 'runIfDue: skipped, previous check still running')
      return
    }
    // 先占位再 await：并发调用在第一个 await 之前就被互斥挡下。
    const task: Promise<void> = this.evaluateAndSync()
    this.running = task
    try {
      await task
    } finally {
      this.running = null
    }
  }

  // ── 内部实现 ──

  private async evaluateAndSync(): Promise<void> {
    const nowMs: number = Date.now()
    // 决策前重读 Core 真源：进程内存里的 lastSuccessMs 只是缓存。
    await this.refreshLastSuccessFromCore()
    const intervalMs: number = this.effectiveIntervalMs()
    const lastSuccessMs: number | null = this.lastSuccessMs
    if (lastSuccessMs !== null && nowMs - lastSuccessMs < intervalMs) {
      hilog.info(DOMAIN, TAG, 'runIfDue: not due, elapsedMs=%{public}s intervalMs=%{public}s',
        (nowMs - lastSuccessMs).toString(), intervalMs.toString())
      return
    }
    hilog.info(DOMAIN, TAG, 'runIfDue: due, entering performSync')
    const outcome: SyncOutcome = await SyncOperationController.getInstance().performSync('auto')
    hilog.info(DOMAIN, TAG, 'runIfDue: sync finished, phase=%{public}s overallStatus=%{public}s',
      outcome.phase, outcome.overallStatus ?? 'unknown')
    // 同步结束（无论终态）后重读 Core：成功类由 Core 写入新的 last_success_time，
    // 失败类 Core 保留旧值；这里绝不自行把当前时间当成成功时间。
    await this.refreshLastSuccessFromCore()
  }

  /** 从 Core 当前持久化状态刷新最近成功时刻；读失败时保留上次读到的缓存值。 */
  private async refreshLastSuccessFromCore(): Promise<void> {
    try {
      const result: ResultEnvelope<FullSyncState | null> = await this.bridge.loadFullSyncState()
      if (!result.success) {
        // 读不到 Core 状态：保留缓存（可能来自更早一次成功的 Core 读取），不伪造成功时间。
        hilog.warn(DOMAIN, TAG, 'loadFullSyncState failed: errorCode=%{public}s',
          result.errorCode ?? 'UNKNOWN')
        return
      }
      const state: FullSyncState | null = result.data ?? null
      this.lastSuccessMs = state === null
        ? null
        : HarmonyAutoSyncScheduler.normalizeEpochMs(state.last_success_time)
    } catch (e) {
      hilog.warn(DOMAIN, TAG, 'loadFullSyncState threw: %{public}s',
        e instanceof Error ? e.name : 'Unknown')
    }
  }

  /**
   * Core FullSyncStateDto 的 last_*_time 线格式为 Unix 秒
   * （core/writer_core/src/api/types/sync.rs），统一归一到毫秒；
   * 若上游 FFI 某天改发毫秒，阈值判断同样成立。
   */
  private static normalizeEpochMs(epoch: number | null): number | null {
    if (epoch === null) {
      return null
    }
    return epoch < EPOCH_MS_THRESHOLD ? epoch * 1000 : epoch
  }

  private effectiveIntervalMs(): number {
    const configured: number = this.config !== null
      ? this.config.syncIntervalSeconds
      : MIN_INTERVAL_SECONDS
    const seconds: number = Number.isFinite(configured) && configured >= MIN_INTERVAL_SECONDS
      ? configured
      : MIN_INTERVAL_SECONDS
    return seconds * 1000
  }

  private isAutoSyncActive(): boolean {
    const config: SyncConfig | null = this.config
    return config !== null && config.enabled && config.autoSync
  }

  private startForegroundTimer(): void {
    const intervalMs: number = this.effectiveIntervalMs()
    if (this.timerId !== -1 && this.timerIntervalMs === intervalMs) {
      // 已是同一周期的前台唤醒：保持现有计时相位，不因 reconfigure 反复重置。
      return
    }
    this.stopForegroundTimer()
    this.timerIntervalMs = intervalMs
    this.timerId = setInterval(() => {
      this.runIfDue()
    }, intervalMs)
    hilog.info(DOMAIN, TAG, 'foreground wake timer armed: intervalMs=%{public}s',
      intervalMs.toString())
  }

  private stopForegroundTimer(): void {
    if (this.timerId !== -1) {
      clearInterval(this.timerId)
      this.timerId = -1
    }
    this.timerIntervalMs = -1
  }
}
