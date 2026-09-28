/**
 * StarMapMotion — 星图节点运动逻辑纯计算模块。
 *
 * 从 StarMapScreen.ets 抽出的运动逻辑，不依赖 ArkUI 组件，不依赖 corebridge。
 * 只提供计算函数，不启动定时器；调用方应使用 ArkUI Animator 的 onFrame 逐帧回调驱动动画。
 */

// ── Motion 配置 ──────────────────────────────────────────────

/**
 * 平台运动配置接口。
 * 与 StarMapMotionPolicy DTO 字段一一对应，但此处不 import corebridge。
 */
export interface PlatformMotionConfig {
  /** 是否启用运动效果 */
  enabled: boolean
  /** 是否减少动画（无障碍） */
  reduceMotion: boolean
  /** 是否启用空闲漂浮 */
  idleWobbleEnabled: boolean
  /** 空闲漂浮振幅（vp） */
  idleAmplitudeVp: number
  /** 空闲漂浮周期（ms） */
  idlePeriodMs: number
  /** 拖动时节点放大倍数 */
  dragLiftScale: number
  /** 拖动结束后回弹动画时长（ms） */
  settleDurationMs: number
}

/**
 * 默认运动配置。
 */
export function defaultMotionConfig(): PlatformMotionConfig {
  return {
    enabled: true,
    reduceMotion: false,
    idleWobbleEnabled: true,
    idleAmplitudeVp: 2.0,
    idlePeriodMs: 4200,
    dragLiftScale: 1.04,
    settleDurationMs: 220,
  }
}

// ── Idle wobble phase / amplitude ────────────────────────────

/**
 * 用 nodeId hash 算稳定 phase（弧度）。
 * 与 StarMapScreen.ets 中的实现完全一致。
 *
 * hash 算法：经典 DJB2 变体，`hash = hash * 31 + charCode`。
 * 结果映射到 [0, 2π) 区间，保证同一 nodeId 始终得到相同 phase。
 */
export function stablePhase(nodeId: string): number {
  let hash = 0
  for (let i = 0; i < nodeId.length; i++) {
    hash = ((hash << 5) - hash) + nodeId.charCodeAt(i)
    hash = hash & hash // Convert to 32bit integer
  }
  return (Math.abs(hash) % 360) * Math.PI / 180
}

/**
 * 计算节点的视觉水平偏移（idle wobble），不修改 node.x。
 *
 * 当 motion 未启用、idleWobble 未启用、reduceMotion 开启、或该节点正在被拖动时返回 0。
 *
 * @param nodeId       节点 ID
 * @param tick         当前时间戳（ms），由调用方通过 ArkUI animation 提供
 * @param config       运动配置
 * @param draggingNodeId 当前正在拖动的节点 ID（空字符串表示无拖动）
 */
export function wobbleX(
  nodeId: string,
  tick: number,
  config: PlatformMotionConfig,
  draggingNodeId: string,
): number {
  if (!config.enabled || !config.idleWobbleEnabled || config.reduceMotion || draggingNodeId === nodeId) {
    return 0
  }
  const phase = stablePhase(nodeId)
  const period = config.idlePeriodMs
  const amplitude = config.idleAmplitudeVp
  return Math.sin((tick / period) * 2 * Math.PI + phase) * amplitude
}

/**
 * 计算节点的视觉垂直偏移（idle wobble），不修改 node.y。
 * Y 方向频率略低（0.7x）、振幅更小（0.6x），营造自然漂浮感。
 *
 * 当 motion 未启用、idleWobble 未启用、reduceMotion 开启、或该节点正在被拖动时返回 0。
 *
 * @param nodeId       节点 ID
 * @param tick         当前时间戳（ms），由调用方通过 ArkUI animation 提供
 * @param config       运动配置
 * @param draggingNodeId 当前正在拖动的节点 ID（空字符串表示无拖动）
 */
export function wobbleY(
  nodeId: string,
  tick: number,
  config: PlatformMotionConfig,
  draggingNodeId: string,
): number {
  if (!config.enabled || !config.idleWobbleEnabled || config.reduceMotion || draggingNodeId === nodeId) {
    return 0
  }
  const phase = stablePhase(nodeId)
  const period = config.idlePeriodMs
  const amplitude = config.idleAmplitudeVp
  return Math.cos((tick / period) * 0.7 * 2 * Math.PI + phase) * amplitude * 0.6
}

// ── Idle wobble（progress 驱动）─────────────────────────────

/**
 * 计算节点的视觉水平偏移（idle wobble），基于动画 progress ∈ [0, 1]。
 *
 * progress 由 ArkUI Animator 的 onFrame 逐帧回调提供，每帧更新。
 * progress 直接映射到 sin 的完整周期：progress=0 → sin(phase)，progress=1 → sin(2π+phase)。
 *
 * 当 motion 未启用、idleWobble 未启用、reduceMotion 开启、或该节点正在被拖动时返回 0。
 *
 * @param nodeId         节点 ID
 * @param progress       动画进度 ∈ [0, 1]，由 Animator onFrame 提供
 * @param config         运动配置
 * @param draggingNodeId 当前正在拖动的节点 ID（空字符串表示无拖动）
 */
export function wobbleXFromProgress(
  nodeId: string,
  progress: number,
  config: PlatformMotionConfig,
  draggingNodeId: string,
): number {
  if (!config.enabled || !config.idleWobbleEnabled || config.reduceMotion || draggingNodeId === nodeId) {
    return 0
  }
  return Math.sin(progress * Math.PI * 2 + stablePhase(nodeId)) * config.idleAmplitudeVp
}

/**
 * 计算节点的视觉垂直偏移（idle wobble），基于动画 progress ∈ [0, 1]。
 * Y 方向频率略低（0.7x）、振幅更小（0.6x），营造自然漂浮感。
 *
 * 当 motion 未启用、idleWobble 未启用、reduceMotion 开启、或该节点正在被拖动时返回 0。
 *
 * @param nodeId         节点 ID
 * @param progress       动画进度 ∈ [0, 1]，由 Animator onFrame 提供
 * @param config         运动配置
 * @param draggingNodeId 当前正在拖动的节点 ID（空字符串表示无拖动）
 */
export function wobbleYFromProgress(
  nodeId: string,
  progress: number,
  config: PlatformMotionConfig,
  draggingNodeId: string,
): number {
  if (!config.enabled || !config.idleWobbleEnabled || config.reduceMotion || draggingNodeId === nodeId) {
    return 0
  }
  return Math.cos(progress * 0.7 * Math.PI * 2 + stablePhase(nodeId)) * config.idleAmplitudeVp * 0.6
}

// ── 拖动/拉线暂停追踪 ───────────────────────────────────────

/**
 * 追踪哪些节点当前被拖动或拉线，应暂停动画。
 * 调用方在拖动开始时 pauseNode，拖动结束后 resumeNode。
 */
export class MotionPauseTracker {
  private pausedNodes: Set<string> = new Set()

  /** 标记节点为暂停状态（正在拖动或拉线） */
  pauseNode(nodeId: string): void {
    this.pausedNodes.add(nodeId)
  }

  /** 恢复节点动画 */
  resumeNode(nodeId: string): void {
    this.pausedNodes.delete(nodeId)
  }

  /** 查询节点是否处于暂停状态 */
  isPaused(nodeId: string): boolean {
    return this.pausedNodes.has(nodeId)
  }

  /** 清空所有暂停状态 */
  clear(): void {
    this.pausedNodes.clear()
  }
}

// ── 辅助函数 ────────────────────────────────────────────────

/**
 * 判断当前配置下是否应该运行动画。
 * 调用方可用此函数决定是否启动 ArkUI animation 闭环。
 */
export function shouldAnimate(config: PlatformMotionConfig): boolean {
  return config.enabled && !config.reduceMotion
}
