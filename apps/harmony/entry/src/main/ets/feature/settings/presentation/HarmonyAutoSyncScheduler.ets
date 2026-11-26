// HarmonyAutoSyncScheduler — 进程唯一自动同步调度器（Issue #925 / #1058）。
//
// 职责：按 Core 已提交的 SyncConfig 判断"是否到点"，到点才经
// SyncOperationController.performSync()（#923 的唯一同步事务入口，内含保存屏障/会话协调/去重）
// 触发全量同步。调度器不自己发同步请求、不复制同步状态机，也不写第二套链路。
//
// 两类同步原因（Issue #1058）：
// - 周期 timer：前台周期唤醒，只有距最近成功同步 >= 用户设置的间隔才同步（runIfDue）；
// - 交接（handoff）：首次 Core 就绪、退后台保存完成、真实后台→前台切换、网络恢复重试。
//   交接请求绕过周期 timer 的间隔到期判断，但同样受 enabled && autoSync、单一运行任务
//   与 SyncOperationController 的唯一事务约束。
//
// 关键约束：
// - 最近一次成功同步时刻的唯一真源是 Core 持久化状态（app-meta/sync/full_state.local.json，
//   经 NativeSyncBridge.loadFullSyncState() 读取）；ArkTS 侧只做缓存，每次决策前重读 Core。
// - 非成功终态绝不更新 lastSuccess（Core 失败时保留旧 last_success_time），不制造假成功时间。
// - reconfigure 只记录配置、启停前台唤醒 timer，不触发同步；是否 due 一律由 runIfDue 决定，
//   打开设置页不会额外产生一笔同步。
// - 退后台的交接同步只在全部本地保存完成后的 post-save 阶段发起（HarmonyLifecycle），
//   不在 onBackground 里与保存抢跑；失败/被系统挂起时保留待同步代次，恢复后重试。
// - 前台 setInterval 只是前台唤醒，不代表能在后台可靠周期执行；后台续跑需系统调度，另行接入。
//
// 注意（Issue #1058 核对）：Core 全量同步的 per-target LWW 入口自带 min_interval
// debounce（core/writer_core/src/sync/lww/engine.rs）。鸿蒙 C ABI 新增
// `writer_core_perform_full_sync_with_options(trigger, force_sync)`：交接类触发把
// 真实原因传给 Core 诊断并以 force_sync=true 只跳过周期防抖；周期 timer 仍以
// force_sync=false 进入 Core debounce。force 不改变对象级 LWW 与正文三路比较。
//
// 桥接依赖（Issue #925 的 Rust FFI / NAPI / decode 工作流并行提供）：
// - corebridge/dto/SyncDtos.ets 的 FullSyncState（snake_case 字段：
//   overall_status / last_attempt_time / last_success_time / failed_targets）；
// - NativeSyncBridge.loadFullSyncState(): Promise<ResultEnvelope<FullSyncState | null>>。
// 尚未落库时按该契约编码，待桥接落地后统一验证。

import hilog from '@ohos.hilog'
import emitter from '@ohos.events.emitter'
import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'
import { FullSyncState, SyncConfig } from '../../../corebridge/dto/SyncDtos'
import { NativeSyncBridge } from '../../../corebridge/native/NativeSyncBridge'
import { NativeCoreModule } from '../../../corebridge/native/NativeCoreModule'
import { HarmonyLifecycle, SyncLifecycleObserver } from '../../../platform/lifecycle/HarmonyLifecycle'
import { NetworkStatusService } from '../../../platform/network/NetworkStatusService'
import { SyncOperationController } from './SyncOperationController'
import { SYNC_DURABLE_COMMIT_EVENT_ID, SyncOutcome, SyncTriggerReason } from './SyncRunTypes'
import { NearbyDeviceSyncCoordinator } from '../../../platform/peer/NearbyDeviceSyncCoordinator'

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
  /**
   * 待同步事实（Issue #1058 复审）：最近一次交接请求的代次，0 表示没有待同步请求。
   * 每次请求分配一个递增代次；运行中排队的后续请求会拿到更大的代次。
   * 只有"覆盖最新代次"的成功终态才清除待同步事实，旧代次的成功/失败都不动它——
   * 这样 A 正在同步、B 被排成后续任务时，A 先成功不会把 B 的待同步事实清掉，
   * B 失败也仍然保持待同步，由网络恢复/下次真实前台切换重试。
   */
  private handoffPendingGeneration: number = 0
  /** 最近一次分配的交接请求代次（单调递增，不因完成而回退）。 */
  private handoffRequestGeneration: number = 0
  /**
   * 待上传的 durable 提交事实（Issue #1065）：多次业务事务提交只合并成一次上传
   * 唤醒；实际发起仍按用户设置的同步间隔，不按保存次数同步。
   */
  private uploadWakePending: boolean = false
  /** 合并上传唤醒的一次性 timer（-1 表示未挂）。 */
  private uploadWakeTimerId: number = -1
  /** durable 提交事件订阅的注销函数（null 表示未订阅）。 */
  private unsubscribeDurableCommit: (() => void) | null = null

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
      },
      (): Promise<void> => this.onBackgroundSaved())
    lifecycle.addObserver(this.observer)
    this.unsubscribeNetwork = NetworkStatusService.getInstance().onNetworkRestored((): void => {
      this.onNetworkRestored()
    })
    const durableCommitHandler: ((data: emitter.EventData) => void) = (_data: emitter.EventData): void => {
      this.onDurableCommit()
    }
    emitter.on({ eventId: SYNC_DURABLE_COMMIT_EVENT_ID }, durableCommitHandler)
    this.unsubscribeDurableCommit = (): void => {
      emitter.off(SYNC_DURABLE_COMMIT_EVENT_ID, durableCommitHandler)
    }
    this.isForeground = lifecycle.isForeground()
    hilog.info(DOMAIN, TAG, 'attach: lifecycle and network listeners registered, awaiting core ready')
  }

  /**
   * Native Core 初始化成功后的唯一通知入口（首页初始化路径在 initialize 成功后调用）。
   * 此时才从 Core 读已提交配置，并按真实前台状态挂前台唤醒。
   * Issue #1058：首次就绪额外发起一次交接同步（绕过周期 timer 的间隔判断），
   * 让刚启动的设备主动检查远端，而不是等用户设置的间隔到期。
   * 读取失败不当作"自动同步关闭"：configLoaded 保持 false，由后续前台/网络恢复重试。
   */
  async onCoreReady(): Promise<void> {
    const firstReady: boolean = !this.coreReady
    this.coreReady = true
    await this.reloadConfigFromCore()
    const startupTrigger: SyncTriggerReason | null = firstReady ? 'startup' : null
    this.continueForegroundAfterConfig(startupTrigger)
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
    const unsubscribeDurable: (() => void) | null = this.unsubscribeDurableCommit
    if (unsubscribeDurable !== null) {
      unsubscribeDurable()
      this.unsubscribeDurableCommit = null
    }
    this.clearUploadWake()
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
      // 自动同步被关闭：不再保留待同步事实（用户已明确不做自动同步）。
      this.handoffPendingGeneration = 0
      this.clearUploadWake()
      hilog.info(DOMAIN, TAG, 'reconfigure: auto sync inactive, foreground timer stopped')
      return
    }
    hilog.info(DOMAIN, TAG, 'reconfigure: auto sync active, intervalSeconds=%{public}s',
      config.syncIntervalSeconds.toString())
    if (this.isForeground) {
      this.startForegroundTimer()
    }
  }

  /**
   * 进入前台：必要时重试读取配置，然后挂前台唤醒 timer。
   * Issue #1058：只有真实的后台→前台切换才发起一次交接同步；重复的 foreground 通知
   * （onWindowStageCreate + onForeground）不重复请求同步。上次未完成的退后台交接也在这里重试。
   */
  onForeground(): void {
    const wasForeground: boolean = this.isForeground
    this.isForeground = true
    // Issue #988：附近设备会话的唤醒与云备份到期判断互不依赖；即使本机没有
    // GitHub 配置，已连接的可信设备仍可主动同步。Core 内部继续使用同一事务锁。
    NearbyDeviceSyncCoordinator.getInstance().requestAllSessionsSync()
    const trigger: SyncTriggerReason | null = wasForeground ? null : 'foreground_handoff'
    if (!this.configLoaded) {
      // 配置尚未成功读过（Core 未就绪或上次读取失败）：本次前台重试一次真实读取，
      // 不把读取失败记忆成"自动同步关闭"；Core 未就绪时等 onCoreReady()。
      if (!this.coreReady) {
        hilog.info(DOMAIN, TAG, 'onForeground: core not ready, defer to onCoreReady')
        return
      }
      this.reloadConfigFromCore().then((): void => {
        this.continueForegroundAfterConfig(trigger)
      })
      return
    }
    this.continueForegroundAfterConfig(trigger)
  }

  /**
   * 配置已确定时的前台编排：挂前台唤醒 timer。
   * 交接类触发（首次 Core 就绪 / 真实前台切换 / 待同步重试）立即发起一次交接同步，
   * 不经过 runIfDue 的间隔判断；周期 timer 仍按用户设置间隔到期才同步。
   */
  private continueForegroundAfterConfig(trigger: SyncTriggerReason | null): void {
    if (!this.isForeground) {
      return
    }
    if (!this.isAutoSyncActive()) {
      hilog.info(DOMAIN, TAG, 'foreground: auto sync inactive')
      return
    }
    this.startForegroundTimer()
    if (this.uploadWakePending) {
      // 回前台后重新按间隔安排合并上传（后台期间到达的 durable 提交）。
      this.scheduleUploadWake()
    }
    if (trigger !== null) {
      this.requestHandoffSync(trigger)
      return
    }
    if (this.handoffPendingGeneration !== 0) {
      hilog.info(DOMAIN, TAG, 'foreground: retrying pending handoff sync')
      this.requestHandoffSync('handoff_retry')
    }
  }

  /**
   * 进入后台：只停止前台唤醒 timer。
   * 交接同步由 HarmonyLifecycle 在全部 observer 保存完成后的 post-save 阶段调用
   * onBackgroundSaved() 发起，这里不与仍在进行的本地保存抢跑。
   */
  onBackground(): void {
    this.isForeground = false
    // 待上传事实保留；只取消后台无法执行的一次性唤醒，回前台后重新安排。
    this.cancelUploadWakeTimer()
    this.stopForegroundTimer()
    hilog.info(DOMAIN, TAG, 'onBackground: foreground timer stopped, awaiting post-save handoff')
  }

  /**
   * 退后台保存完成后的交接同步（HarmonyLifecycle post-save 阶段调用）。
   * 返回 Promise 以便纳入同一个短时任务事务：保存之后、诊断压缩收尾之前完成同步。
   * 仍使用 Core 已提交的 enabled && autoSync；配置未知时先真实读取一次，
   * 读不到就不把"不知道"当成"已开启自动同步"。
   * 失败/被系统挂起时保留待同步代次，由网络恢复或下次真实前台切换重试。
   */
  async onBackgroundSaved(): Promise<void> {
    if (!this.coreReady) {
      hilog.info(DOMAIN, TAG, 'onBackgroundSaved: core not ready, skip handoff sync')
      return
    }
    if (!this.configLoaded) {
      await this.reloadConfigFromCore()
    }
    if (!this.isAutoSyncActive()) {
      hilog.info(DOMAIN, TAG, 'onBackgroundSaved: auto sync inactive, skip handoff sync')
      return
    }
    if (!this.uploadWakePending) {
      // 没有已落盘的业务变更就没有上传内容：退后台保存结束不做空闲交接，
      // 远端变化交给前台的轻量探测（Issue #1065 复审 #6）。
      hilog.info(DOMAIN, TAG, 'onBackgroundSaved: no durable mutation pending, skip handoff')
      return
    }
    hilog.info(DOMAIN, TAG, 'onBackgroundSaved: background save finished, starting handoff sync')
    const generation: number = this.nextHandoffGeneration()
    await this.performHandoffSync('background_handoff', generation)
  }

  /** 网络恢复：必要时重试读取配置；优先重试未完成的交接，否则仍立即做一次到期判断。 */
  onNetworkRestored(): void {
    // Issue #988：网络恢复先唤醒附近设备会话，再走 GitHub 的到期判断。
    NearbyDeviceSyncCoordinator.getInstance().requestAllSessionsSync()
    if (!this.configLoaded) {
      if (!this.coreReady) {
        hilog.info(DOMAIN, TAG, 'onNetworkRestored: core not ready, defer to onCoreReady')
        return
      }
      this.reloadConfigFromCore().then((): void => {
        this.continueAfterNetworkRestored()
      })
      return
    }
    this.continueAfterNetworkRestored()
  }

  /** 网络恢复后的编排：有未完成交接就先重试交接（绕过间隔），否则按原有到期判断。 */
  private continueAfterNetworkRestored(): void {
    if (this.handoffPendingGeneration !== 0 && this.isAutoSyncActive()) {
      hilog.info(DOMAIN, TAG, 'onNetworkRestored: retrying pending handoff sync')
      this.requestHandoffSync('handoff_retry')
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

  // ── durable 提交上传唤醒（Issue #1065）──

  /**
   * 业务事务已 durable 落盘（正文/设置/统计数据提交成功）后的上传唤醒。
   *
   * 这里只声明"有新的已落盘事实待上传"，不直接同步：多次提交合并成一个待上传
   * 标记，并只在距最近成功同步满足用户设置的间隔时发起；同步仍经
   * SyncOperationController 的唯一事务入口，不绕过保存屏障与单一运行任务。
   */
  onDurableCommit(): void {
    if (!this.coreReady || !this.isAutoSyncActive()) {
      return
    }
    this.uploadWakePending = true
    this.scheduleUploadWake()
  }

  /** 按用户间隔安排一次合并后的上传唤醒（已挂 timer 时不再重复挂）。 */
  private scheduleUploadWake(): void {
    if (this.uploadWakeTimerId !== -1) {
      return
    }
    const intervalMs: number = this.effectiveIntervalMs()
    const lastSuccessMs: number | null = this.lastSuccessMs
    const elapsedMs: number = lastSuccessMs === null
      ? intervalMs
      : Math.max(0, Date.now() - lastSuccessMs)
    const delayMs: number = Math.max(0, intervalMs - elapsedMs)
    if (delayMs === 0) {
      this.fireUploadWake()
      return
    }
    hilog.info(DOMAIN, TAG, 'upload wake armed: delayMs=%{public}s', delayMs.toString())
    this.uploadWakeTimerId = setTimeout((): void => {
      this.uploadWakeTimerId = -1
      this.fireUploadWake()
    }, delayMs)
  }

  /** 到点后发起一次合并上传；后台或自动同步已关闭时保留待上传事实。 */
  private fireUploadWake(): void {
    this.uploadWakeTimerId = -1
    if (!this.uploadWakePending) {
      return
    }
    if (!this.isForeground || !this.isAutoSyncActive()) {
      hilog.info(DOMAIN, TAG, 'upload wake deferred: foreground=%{public}s autoSync=%{public}s',
        this.isForeground.toString(), this.isAutoSyncActive().toString())
      return
    }
    this.uploadWakePending = false
    this.requestHandoffSync('durable_commit')
  }

  /** 丢弃待上传事实并取消合并 timer（自动同步被关闭或解绑时）。 */
  private clearUploadWake(): void {
    this.uploadWakePending = false
    this.cancelUploadWakeTimer()
  }

  private cancelUploadWakeTimer(): void {
    if (this.uploadWakeTimerId !== -1) {
      clearTimeout(this.uploadWakeTimerId)
      this.uploadWakeTimerId = -1
    }
  }

  // ── 交接同步（Issue #1058）──

  /** 发起一次交接同步（不等待结果）；失败保留待同步事实，由前台/网络恢复重试。 */
  private requestHandoffSync(trigger: SyncTriggerReason): void {
    const generation: number = this.nextHandoffGeneration()
    this.performHandoffSync(trigger, generation).catch(() => {})
  }

  /**
   * 分配新的交接代次，并把待同步事实推进到该代次。
   * 运行中排队的后续请求会拿到更大的代次，从而覆盖旧请求的待同步事实。
   */
  private nextHandoffGeneration(): number {
    this.handoffRequestGeneration += 1
    const generation: number = this.handoffRequestGeneration
    if (generation > this.handoffPendingGeneration) {
      this.handoffPendingGeneration = generation
    }
    return generation
  }

  /**
   * 交接同步实体：绕过周期 timer 的间隔到期判断，但保留 enabled && autoSync、
   * 单一同步事务与保存屏障——仍然只经 SyncOperationController.performSync 进入。
   *
   * 待同步事实按代次裁决：只有覆盖最新代次（handoffRequestGeneration）的成功终态
   * 才清除；旧代次的成功/失败都不动它。这样 A 正在同步、B 被排成后续任务时，
   * A 先成功不会清掉 B 的待同步事实，B 失败也仍保持待同步，不假报完成。
   */
  private async performHandoffSync(trigger: SyncTriggerReason, generation: number): Promise<void> {
    hilog.info(DOMAIN, TAG, 'handoff sync: started trigger=%{public}s generation=%{public}s',
      trigger, generation.toString())
    try {
      const outcome: SyncOutcome = await SyncOperationController.getInstance().performSync('auto', trigger)
      const isLatest: boolean = generation === this.handoffRequestGeneration
      if (outcome.phase === 'success') {
        // Core 成功即表示本轮待发布事实已经确认或确实无内容可发布。
        this.uploadWakePending = false
        if (isLatest) {
          this.handoffPendingGeneration = 0
        }
        hilog.info(DOMAIN, TAG,
          'handoff sync: finished trigger=%{public}s generation=%{public}s latest=%{public}s ' +
          'overallStatus=%{public}s',
          trigger, generation.toString(), isLatest.toString(), outcome.overallStatus ?? 'unknown')
      } else {
        hilog.warn(DOMAIN, TAG,
          'handoff sync: not completed, keeping pending trigger=%{public}s generation=%{public}s ' +
          'latest=%{public}s errorCode=%{public}s failureStage=%{public}s',
          trigger, generation.toString(), isLatest.toString(),
          outcome.errorCode ?? 'UNKNOWN', outcome.failureStage ?? 'unknown')
      }
    } catch (e) {
      hilog.warn(DOMAIN, TAG,
        'handoff sync: threw, keeping pending trigger=%{public}s generation=%{public}s errorType=%{public}s',
        trigger, generation.toString(), e instanceof Error ? e.name : 'Unknown')
    }
    // 同步结束（无论终态）后重读 Core：成功类由 Core 写入新的 last_success_time，
    // 失败类 Core 保留旧值；这里绝不自行把当前时间当成成功时间。
    await this.refreshLastSuccessFromCore()
    if (this.uploadWakePending) {
      // 本轮同步期间又有新的 durable 提交：留待下一轮，不把旧快照当最新。
      this.scheduleUploadWake()
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
    const outcome: SyncOutcome = await SyncOperationController.getInstance().performSync('auto', 'auto_timer')
    hilog.info(DOMAIN, TAG, 'runIfDue: sync finished, phase=%{public}s overallStatus=%{public}s',
      outcome.phase, outcome.overallStatus ?? 'unknown')
    // 同步结束（无论终态）后重读 Core：成功类由 Core 写入新的 last_success_time，
    // 失败类 Core 保留旧值；这里绝不自行把当前时间当成成功时间。
    await this.refreshLastSuccessFromCore()
    if (this.uploadWakePending) {
      this.scheduleUploadWake()
    }
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

  /**
   * 自动同步是否生效。
   *
   * 间隔 <= 0 按"不自动同步"处理（Issue #1065 复审 #6）：0 间隔表示只用
   * 手动入口，平台不能自行把 0 当成 60 秒周期去唤醒 Core。
   */
  private isAutoSyncActive(): boolean {
    const config: SyncConfig | null = this.config
    return config !== null && config.enabled && config.autoSync &&
      config.syncIntervalSeconds > 0
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
