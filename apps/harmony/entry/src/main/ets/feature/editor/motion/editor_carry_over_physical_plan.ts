// editor_carry_over_physical_plan.ts — carry-over 物理窗口规划的纯逻辑模块。
//
// Issue #879 评论6153434157 问题2+3+4+5：
// 将组级身份分配、分组判断提成生产与测试共用的纯逻辑模块。
// 真实 Planner/Coordinator 调用它，测试直接使用它。
//
// 修复内容：
// - 问题2：renderNodeKey 包含目标行/位置信息，不同目标行的 reflow 组不会碰撞
// - 问题3：windowInstanceId 通过 WindowInstanceIdAllocator 分配，登记进反向映射
// - 问题4：groupKey 包含 sourceKind/sourceRevision/parentRenderNodeKey/轨迹信息，
//          不同轨迹的片段不会被错误合并；合并后的组拥有完整目标信息
// - 问题5：测试直接引用本模块的正式实现，不再手写拼 key 逻辑
//
// 纯逻辑：不依赖 ArkUI、不 import .ets，生产由 motion 层 .ets 调用，Node 单测直接 import。

import { WindowInstanceIdAllocator } from './editor_glyph_identity.ts'
import type { GlyphIdentityTable } from './editor_glyph_identity.ts'
import { isFullyCovered } from './editor_commit_start.ts'

// ── 最小类型定义（与 .ets 中的 ClipRect/GlyphRect 兼容） ──

/** 裁切矩形——与 EditorVisualFrame.ets 的 ClipRect 结构兼容。 */
export interface ClipRect {
  x: number
  y: number
  width: number
  height: number
}

/** 字形矩形几何——与 EditorVisualFrame.ets 的 GlyphRect 结构兼容。 */
export interface GlyphRect {
  x: number
  y: number
  width: number
  height: number
}

// ── 输入类型 ──

/**
 * carry-over 规划的片段输入——来自 splitFrozenWindowByGlyphOwnership 的输出。
 *
 * 每个片段携带独立的源/目标范围和几何，不再共享母窗口的整段范围。
 */
export interface CarryOverPieceInput {
  /** 该片段覆盖的字符簇身份列表 */
  glyphIds: string[]
  /** 该片段在源正文中的 UTF-16 起始 offset */
  sourceUtf16Start: number
  /** 该片段在源正文中的 UTF-16 结束 offset（exclusive） */
  sourceUtf16End: number
  /** 该片段在冻结帧的多岛裁切矩形 */
  clipRects: ClipRect[]
  /** 冻结裁切左边界 */
  clipLeft: number
  /** 冻结裁切右边界 */
  clipRight: number
  /** 冻结裁切上边界 */
  clipTop: number
  /** 冻结裁切下边界 */
  clipBottom: number
  /** 绘制偏移 x */
  offsetX: number
  /** 绘制偏移 y */
  offsetY: number
  /** 当前位置 x */
  currentPositionX: number
  /** 当前位置 y */
  currentPositionY: number
  /** 该片段在新版正文中的 UTF-16 起始 offset（-1 表示未解析） */
  targetUtf16Start: number
  /** 该片段在新版正文中的 UTF-16 结束 offset（exclusive，-1 表示未解析） */
  targetUtf16End: number
  /** 目标范围是否已解析 */
  targetResolved: boolean
  /** 该片段在新版布局中的矩形（null 表示未解析） */
  targetRect: GlyphRect | null
  /** 字形排版位置 x */
  glyphX: number
  /** 字形排版位置 y */
  glyphY: number
  /** 字形宽度 */
  glyphWidth: number
  /** 字形高度 */
  glyphHeight: number
}

/**
 * 父窗口上下文——提供分组和身份生成所需的父窗口信息。
 */
export interface ParentWindowContext {
  /** 父窗口的算法身份（windowIdForGlyphIds 的输出） */
  windowId: string
  /** 父窗口的渲染节点身份标识 */
  renderNodeKey: string
  /** 父窗口的实例 ID（win-<number> 格式） */
  windowInstanceId: string
  /** 字形来源：'old' 或 'new' */
  sourceKind: 'old' | 'new'
  /** 字形源对应的正文 revision */
  sourceRevision: number
  /** 父窗口的运动所有权类型 */
  motionOwnerKind: 'deleted' | 'reflow' | 'insert' | 'carryGhost'
  /** 窗口透明度 */
  opacity: number | undefined
  /** 父窗口的完整逐字 glyphId→UTF-16 映射 */
  fullWindowGlyphUtf16Ranges: Map<string, [number, number]> | null
}

/**
 * 带有父窗口上下文和运动通道的片段。
 *
 * 运动通道由 determineMotionChannel 确定，在调用 planCarryOverPhysicalGroups 前计算。
 */
export interface CarryOverPieceWithContext {
  /** 片段几何和范围信息 */
  piece: CarryOverPieceInput
  /** 父窗口上下文 */
  parent: ParentWindowContext
  /** 运动通道：ghost（消退）/ insert（吐字扩展）/ reflow（位置移动） */
  motionChannel: 'ghost' | 'insert' | 'reflow'
}

// ── 输出类型 ──

/**
 * carry-over 物理窗口规划组——与 EditorMotionCoordinator.ets 的 CarryOverPhysicalGroup 兼容。
 *
 * 每个组代表一个独立的物理 Text 节点，拥有唯一且固定的身份。
 */
export interface CarryOverPhysicalGroupResult {
  /** 稳定的物理身份——在 retarget 时保持不变 */
  renderNodeKey: string
  /** 通过 WindowInstanceIdAllocator 分配的实例 ID */
  windowInstanceId: string
  /** 物理属性 */
  sourceKind: 'old' | 'new'
  sourceRevision: number
  /** 运动通道 */
  motionChannel: 'ghost' | 'insert' | 'reflow'
  /** 父窗口信息 */
  parentWindowId: string
  sourceRunId: string
  /** 合并后的 glyphIds（同组所有 piece 的并集） */
  glyphIds: string[]
  /** 合并后的源 UTF-16 范围 */
  sourceUtf16Start: number
  sourceUtf16End: number
  /** 合并后的 clipRects（同组所有 piece 的并集） */
  clipRects: ClipRect[]
  /** 合并后的逐字映射 */
  fullWindowGlyphUtf16Ranges: Map<string, [number, number]> | null
  /** 目标信息（仅 motion runs 有意义） */
  targetUtf16Start: number
  targetUtf16End: number
  targetResolved: boolean
  targetRect: GlyphRect | null
  /** 几何信息——取自组内第一个 piece */
  glyphX: number
  glyphY: number
  glyphWidth: number
  glyphHeight: number
  clipLeft: number
  clipRight: number
  clipTop: number
  clipBottom: number
  currentPositionX: number
  currentPositionY: number
  offsetX: number
  offsetY: number
  opacity: number | undefined
  /**
   * Issue #879 评论6158342861 问题3：每个视觉岛的源完整矩形列表。
   *
   * insert 合组时，各岛的源完整矩形必须单独保留，不能只取首个 piece 的 glyphX/Width。
   * identifyCarryOverMotionRuns 的 insert 分支需要对每个岛单独做 rectDifference，
   * 再把结果合并为同一个 Text 的裁切并集。
   */
  islandSourceRects: GlyphRect[]
  /**
   * Issue #879 评论6158342861 问题3：每个视觉岛对应的起始 clip 矩形列表。
   *
   * 与 islandSourceRects 一一对应（按 piece 顺序），每个岛有自己的起始裁切状态。
   * identifyCarryOverMotionRuns 用它和 islandSourceRects 一起计算各岛的显露动画。
   */
  islandClipRects: ClipRect[][]
}

// ── 身份缓存 ──

/**
 * Issue #879 评论6159920940：组级身份缓存条目。
 *
 * 在 EditorMotionCoordinator 实例上维护，跨 retarget 保持稳定：
 * - 同一组跨帧/retarget → 查到已有身份，复用同一 renderNodeKey 和 windowInstanceId
 * - 新组 → 通过 WindowInstanceIdAllocator.allocateFresh 分配新身份
 *
 * Issue #879 评论6159920940：owner 不再作为 entry 字段，而是作为内层 Map 的 key。
 * 多 owner 并存结构允许同一 groupKey 下不同 owner 的身份条目同时存在，
 * 避免 owner 切换时覆盖旧条目导致孤儿 instanceId。
 */
export interface IdentityCacheEntry {
  renderNodeKey: string
  windowInstanceId: string
}

/**
 * Issue #879 评论6159920940：组级身份缓存——多 owner 并存结构。
 *
 * 外层 key 是 groupKey，内层 key 是 owner（如 'candidate' 或 'displayed'）。
 * 同一 groupKey 下不同 owner 的条目可以并存，避免 owner 切换时覆盖旧条目。
 */
export type IdentityCache = Map<string, Map<string, IdentityCacheEntry>>

// ── 岛几何/完成判定共享逻辑 ──

/**
 * Issue #879 评论6158702656 缺口1：多岛 insert 的完成判定。
 *
 * 逐一检查每个岛的源完整矩形（加 offset 后）是否被对应的 clip 矩形完全覆盖。
 * 只有**所有岛**都完整覆盖才返回 true；任何一个岛有未覆盖区域就返回 false。
 *
 * 这解决了旧逻辑只看第一个岛的 glyphX/Y/Width/Height 做 isFullyCovered，
 * 导致后面的字没吐完也跳过整组的问题。
 *
 * @param islandSourceRects 每个岛的源完整字形矩形列表
 * @param islandClipRects 每个岛对应的起始 clip 矩形列表（与 islandSourceRects 一一对应）
 * @param offsetX 绘制偏移 x
 * @param offsetY 绘制偏移 y
 * @returns true 表示所有岛都已完全覆盖（组已完成），false 表示至少有一个岛未完成
 */
export function isInsertGroupComplete(
  islandSourceRects: GlyphRect[],
  islandClipRects: ClipRect[][],
  offsetX: number,
  offsetY: number
): boolean {
  for (let i = 0; i < islandSourceRects.length; i++) {
    const islandRect = islandSourceRects[i]
    const islandClip = islandClipRects[i] ?? []
    const fullRect: ClipRect = {
      x: islandRect.x + offsetX,
      y: islandRect.y + offsetY,
      width: islandRect.width,
      height: islandRect.height
    }
    if (!isFullyCovered(fullRect, islandClip)) {
      return false
    }
  }
  return true
}

/**
 * Issue #879 评论6159166760 问题2：reflow 组的完成判定。
 *
 * reflow 组已完成 = 当前位置已到达目标位置 + 透明度已到达终态 + clip 已完全覆盖目标矩形。
 * 此函数是规划阶段与 Coordinator 采样阶段共用的生产逻辑——
 * 规划阶段用它筛除已完成组（不分配身份），Coordinator 用它跳过已完成组（不生成 motion run）。
 *
 * @param currentPositionX 当前位置 x
 * @param currentPositionY 当前位置 y
 * @param targetRect 目标矩形（null 表示未解析，不算完成）
 * @param opacity 当前透明度
 * @param sourceKind 源类型（影响透明度终态判定）
 * @param clipRects 当前裁切矩形列表
 * @returns true 表示 reflow 组已完成，false 表示未完成
 */
export function isReflowGroupComplete(
  currentPositionX: number,
  currentPositionY: number,
  targetRect: GlyphRect | null,
  opacity: number | undefined,
  sourceKind: 'old' | 'new',
  clipRects: ClipRect[]
): boolean {
  if (targetRect === null) {
    return false
  }
  const positionDone = Math.abs(currentPositionX - targetRect.x) < 0.01 &&
      Math.abs(currentPositionY - targetRect.y) < 0.01
  const opacityDone = opacity === undefined ||
      (sourceKind === 'old' ? opacity < 0.01 : opacity > 0.99)
  const targetFullRect: ClipRect = {
    x: targetRect.x,
    y: targetRect.y,
    width: targetRect.width,
    height: targetRect.height
  }
  const clipDone = isFullyCovered(targetFullRect, clipRects)
  return positionDone && opacityDone && clipDone
}

/**
 * Issue #879 评论6160902258 问题3：精确移除指定 groupKey + owner 的身份条目。
 *
 * 先解除该 owner 的逻辑引用（removeLogicalOwner），再检查是否仍有其他 owner 或 lease
 * 引用同一实例——只有既无逻辑 owner 也无 lease 时才真正 allocator.release。
 * 内层 Map 为空时删除整个 groupKey。
 *
 * 这解决了旧 `releaseCarryOverIdentity` 对同一 groupKey 的全部实例无条件 release 的问题：
 * 同一 groupKey 下其他仍有逻辑 owner 或 ArkUI lease 的实例不会被误释放。
 *
 * @param identityCache 组级身份缓存（groupKey→owner→entry）
 * @param allocator 窗口实例 ID 分配器
 * @param groupKey 组键
 * @param owner 要移除的 owner 标识
 * @param expectedInstanceId 预期的实例 ID（防止并发修改后误删）
 */
export function removeIdentityEntry(
  identityCache: IdentityCache,
  allocator: WindowInstanceIdAllocator,
  groupKey: string,
  owner: string,
  expectedInstanceId: string
): void {
  const ownerMap = identityCache.get(groupKey)
  if (ownerMap === undefined) {
    return
  }
  const entry = ownerMap.get(owner)
  if (entry === undefined || entry.windowInstanceId !== expectedInstanceId) {
    return
  }
  // 先解除该 owner 的逻辑引用
  allocator.removeLogicalOwner(owner, entry.windowInstanceId)
  // 只有既无逻辑 owner 也无 lease 才真正释放
  if (!allocator.hasLogicalOwners(entry.windowInstanceId) &&
      !allocator.hasLeases(entry.windowInstanceId)) {
    allocator.release(entry.windowInstanceId)
  }
  // 删除内层 Map 中该 owner 的条目
  ownerMap.delete(owner)
  // 内层为空才删 groupKey
  if (ownerMap.size === 0) {
    identityCache.delete(groupKey)
  }
}

/**
 * Issue #879 评论6158702656 缺口3 + 评论6159920940：释放 carry-over 身份缓存中的条目。
 *
 * 当物理窗口最后租约被释放时，同步解除 groupKey→instanceId 关联，
 * 防止已释放的 instanceId 被后续 cache hit 错误复用。
 *
 * Issue #879 评论6160902258 问题3：不再对同一 groupKey 的全部实例无条件 allocator.release。
 * 改为逐个 owner 先 removeLogicalOwner，再检查 hasLogicalOwners/hasLeases 决定是否释放。
 *
 * @param identityCache 组级身份缓存（groupKey→owner→entry）
 * @param allocator 窗口实例 ID 分配器
 * @param groupKey 要释放的组键
 */
export function releaseCarryOverIdentity(
  identityCache: IdentityCache,
  allocator: WindowInstanceIdAllocator,
  groupKey: string
): void {
  const ownerMap = identityCache.get(groupKey)
  if (ownerMap !== undefined) {
    // 逐个 owner 安全释放：先解除逻辑引用，再检查是否仍有其他引用
    for (const [owner, entry] of ownerMap) {
      allocator.removeLogicalOwner(owner, entry.windowInstanceId)
      if (!allocator.hasLogicalOwners(entry.windowInstanceId) &&
          !allocator.hasLeases(entry.windowInstanceId)) {
        allocator.release(entry.windowInstanceId)
      }
    }
    identityCache.delete(groupKey)
  }
}

/**
 * Issue #879 评论6159166760 问题1+3 + 评论6159920940 + 评论6160902258 问题2：
 * 按 owner 释放 carry-over 身份。
 *
 * 候选作废时调用——释放属于该 owner 且从未挂载（无租约）的实例，
 * 并删除对应的 cache 关联。已挂载（有租约）的身份不在此释放——
 * 它们随真实 ArkUI 卸载及最后一个 lease 释放，避免提早回收。
 *
 * Issue #879 评论6160902258 问题2：先调用 removeLogicalOwner 解除逻辑引用，
 * 再检查 hasLogicalOwners/hasLeases 决定是否释放。
 * 旧代码不调用 removeLogicalOwner，导致 hasLogicalOwners 恒为 true（因为 planCarryOverPhysicalWindows
 * 已对每个候选 ID 调用 addLogicalOwner），实例永远无法释放，造成候选实例泄漏。
 *
 * 与 releaseOwner 的区别：releaseOwner 是正式的生命周期入口，语义更明确。
 * 本函数保留为兼容入口，内部委托 releaseOwner。
 *
 * @param identityCache 组级身份缓存（groupKey→owner→entry）
 * @param allocator 窗口实例 ID 分配器
 * @param owner 要释放的身份生命周期归属（如 'candidate'）
 */
export function releaseCarryOverIdentitiesByOwner(
  identityCache: IdentityCache,
  allocator: WindowInstanceIdAllocator,
  owner: string
): void {
  releaseOwner(identityCache, allocator, owner)
}

/**
 * Issue #879 评论6159920940 + 评论6160902258 问题4：将 groupKey 下属于 fromOwner 的身份条目转交给 toOwner。
 *
 * 候选提交后调用——把 carry-over 物理身份从 candidate owner 转交到 displayed owner，
 * 保持 renderNodeKey 和 windowInstanceId 不变，只更新逻辑 owner 引用。
 *
 * Issue #879 评论6160902258 问题4：转交前检查是否已有旧 toOwner entry。
 * 连续两次提交相同 group key、但两次生成的实例不同时，直接 set 会覆盖旧 displayed entry，
 * 旧实例的 displayed 逻辑 owner 仍在 allocator 中，无法回收。
 * 修复：先移除旧 toOwner entry 的逻辑引用并可能释放，再写入新 entry。
 *
 * - 在内层 Map 中用 toOwner 创建新条目（保持 renderNodeKey 和 windowInstanceId 不变）
 * - 删除 fromOwner 的条目
 * - 在 allocator 中更新逻辑 owner 引用（addLogicalOwner(toOwner, instanceId),
 *   removeLogicalOwner(fromOwner, instanceId)）
 * - 如果 fromOwner 条目不存在，无副作用返回
 *
 * @param identityCache 组级身份缓存（groupKey→owner→entry）
 * @param allocator 窗口实例 ID 分配器
 * @param groupKey 组键
 * @param fromOwner 源 owner（如 'candidate'）
 * @param toOwner 目标 owner（如 'displayed'）
 */
export function transferIdentityOwner(
  identityCache: IdentityCache,
  allocator: WindowInstanceIdAllocator,
  groupKey: string,
  fromOwner: string,
  toOwner: string
): void {
  const ownerMap = identityCache.get(groupKey)
  if (ownerMap === undefined) {
    return
  }
  const entry = ownerMap.get(fromOwner)
  if (entry === undefined) {
    // fromOwner 条目不存在——无副作用返回
    return
  }
  // Issue #879 评论6160902258 问题4：检查是否已有旧 toOwner entry
  // 连续两次提交相同 group key 但实例不同时，旧 displayed entry 会被覆盖，
  // 旧实例的 displayed 逻辑 owner 仍在 allocator 中无法回收。
  const oldToOwnerEntry = ownerMap.get(toOwner)
  if (oldToOwnerEntry !== undefined) {
    // 先移除旧 toOwner 的逻辑引用
    allocator.removeLogicalOwner(toOwner, oldToOwnerEntry.windowInstanceId)
    // 只有既无逻辑 owner 也无 lease 才真正释放旧实例
    if (!allocator.hasLogicalOwners(oldToOwnerEntry.windowInstanceId) &&
        !allocator.hasLeases(oldToOwnerEntry.windowInstanceId)) {
      allocator.release(oldToOwnerEntry.windowInstanceId)
    }
    ownerMap.delete(toOwner)
  }
  // 在内层 Map 中用 toOwner 创建新条目（保持 renderNodeKey 和 windowInstanceId 不变）
  ownerMap.set(toOwner, {
    renderNodeKey: entry.renderNodeKey,
    windowInstanceId: entry.windowInstanceId
  })
  // 删除 fromOwner 的条目
  ownerMap.delete(fromOwner)
  // 在 allocator 中更新逻辑 owner 引用
  allocator.addLogicalOwner(toOwner, entry.windowInstanceId)
  allocator.removeLogicalOwner(fromOwner, entry.windowInstanceId)
}

/**
 * Issue #879 评论6159920940：释放属于特定 owner 的所有身份条目。
 *
 * 遍历所有 groupKey 的内层 Map，删除该 owner 的条目。
 * 在 allocator 中移除该 owner 的逻辑引用（removeLogicalOwner）。
 * 只有既没有逻辑 owner（hasLogicalOwners 返回 false）也没有 lease
 * （hasLeases 返回 false）的实例才真正 allocator.release。
 *
 * @param identityCache 组级身份缓存（groupKey→owner→entry）
 * @param allocator 窗口实例 ID 分配器
 * @param owner 要释放的 owner
 */
export function releaseOwner(
  identityCache: IdentityCache,
  allocator: WindowInstanceIdAllocator,
  owner: string
): void {
  const groupKeysToDelete: string[] = []
  for (const [groupKey, ownerMap] of identityCache) {
    const entry = ownerMap.get(owner)
    if (entry === undefined) {
      continue
    }
    // 在 allocator 中移除该 owner 的逻辑引用
    allocator.removeLogicalOwner(owner, entry.windowInstanceId)
    // 只有既没有逻辑 owner 也没有 lease 的实例才真正释放
    if (!allocator.hasLogicalOwners(entry.windowInstanceId) &&
        !allocator.hasLeases(entry.windowInstanceId)) {
      allocator.release(entry.windowInstanceId)
    }
    // 删除内层 Map 中该 owner 的条目
    ownerMap.delete(owner)
    // 如果内层 Map 为空，删除整个 groupKey 的条目
    if (ownerMap.size === 0) {
      groupKeysToDelete.push(groupKey)
    }
  }
  for (const key of groupKeysToDelete) {
    identityCache.delete(key)
  }
}

// ── 核心函数 ──

/**
 * Issue #879 评论6153434157 问题4：确定一个片段的运动通道。
 *
 * 规则：
 * - deleted/carryGhost → ghost（消退）
 * - insert + 新版已删除 → ghost；insert + 新版仍存在 → insert
 * - reflow + 目标未解析 → ghost；reflow + 目标已解析 → reflow
 * - 其他 → ghost（安全默认）
 *
 * @param parentMotionOwnerKind 父窗口的运动所有权类型
 * @param pieceTargetResolved 片段的目标范围是否已解析
 * @param pieceGlyphIds 片段的字形身份列表
 * @param newIdentityTable 新版字形身份表（null 表示不可信）
 */
export function determineMotionChannel(
  parentMotionOwnerKind: string,
  pieceTargetResolved: boolean,
  pieceGlyphIds: string[],
  newIdentityTable: GlyphIdentityTable | null
): 'ghost' | 'insert' | 'reflow' {
  if (parentMotionOwnerKind === 'deleted' || parentMotionOwnerKind === 'carryGhost') {
    return 'ghost'
  }
  if (parentMotionOwnerKind === 'insert') {
    // insert 窗口中，新版已删除的字形转为 ghost
    if (newIdentityTable === null || newIdentityTable.entryByGlyphId(pieceGlyphIds[0]) === null) {
      return 'ghost'
    }
    return 'insert'
  }
  if (parentMotionOwnerKind === 'reflow') {
    // reflow 窗口中，目标未解析的片段转为 ghost
    if (!pieceTargetResolved) {
      return 'ghost'
    }
    return 'reflow'
  }
  return 'ghost'
}

/**
 * Issue #879 评论6153434157 问题2+4：生成组级分组键。
 *
 * groupKey 包含所有决定片段能否合并为同一物理 Text 的因素：
 * - parentRenderNodeKey：原始物理 Text 身份（不只是 windowId）
 * - sourceKind/sourceRevision：源版本信息
 * - motionChannel：运动通道
 * - 目标位置（仅 reflow）：不同目标位置 = 不同轨迹 = 不能合并
 *
 * 同一 groupKey 的片段可以合并为一个物理组（同一张 Text）；
 * 不同 groupKey 的片段必须分配独立的物理身份。
 *
 * 对于同一父窗口、不同目标行的两个 reflow group，groupKey 不同 → 不会合并。
 * 对于同一候选 retarget，groupKey 不变 → 身份不变。
 */
export function generateGroupKey(
  parentRenderNodeKey: string,
  sourceKind: string,
  sourceRevision: number,
  motionChannel: 'ghost' | 'insert' | 'reflow',
  targetRect: GlyphRect | null
): string {
  // ghost 和 insert 的所有片段遵循同一消退/扩展轨迹，不需要额外区分
  if (motionChannel === 'ghost' || motionChannel === 'insert') {
    return `${parentRenderNodeKey}|${sourceKind}|${sourceRevision}|${motionChannel}`
  }
  // reflow：不同目标位置 = 不同轨迹，必须包含精确目标坐标
  if (targetRect !== null) {
    return `${parentRenderNodeKey}|${sourceKind}|${sourceRevision}|reflow|${targetRect.x}|${targetRect.y}`
  }
  // targetRect 为 null 的 reflow 不应出现（determineMotionChannel 会将其归为 ghost）
  // 但防御性地给出一个唯一键
  return `${parentRenderNodeKey}|${sourceKind}|${sourceRevision}|reflow|null`
}

/**
 * Issue #879 评论6158342861 问题2：生成稳定的渲染节点键。
 *
 * renderNodeKey 由已分配的唯一物理子窗口实例 ID 决定：
 * - sourceKind/sourceRevision：源版本
 * - 通道类型（carryGhost/carryInsert/carryReflow）
 * - childWindowInstanceId：通过 WindowInstanceIdAllocator.allocateFresh 分配的子窗口实例 ID
 *
 * 不再把没有全局唯一性的父 windowInstanceId 当子节点 key 的唯一依据。
 * 目标位置只参与 groupKey 的分组，最终 Text key 应由已分配的唯一物理实例决定。
 */
export function generateRenderNodeKey(
  sourceKind: string,
  sourceRevision: number,
  motionChannel: 'ghost' | 'insert' | 'reflow',
  childWindowInstanceId: string
): string {
  const channelLabel = motionChannel === 'ghost' ? 'carryGhost'
    : motionChannel === 'insert' ? 'carryInsert'
    : 'carryReflow'
  return `${sourceKind}-${sourceRevision}-${channelLabel}-${childWindowInstanceId}`
}

/**
 * Issue #879 评论6158702656 缺口2+3：carry-over 物理窗口规划入口（两阶段组装）。
 *
 * **阶段1**：按 groupKey 完整收集并合并所有片段的 glyphIds、clip、源/目标映射与每岛几何。
 *   组确定后才按完整 glyph 集合 allocateFresh(group.glyphIds)、生成 key、填入缓存。
 *   这修复了旧逻辑在组还没聚合完就 allocateFresh(piece.glyphIds)，
 *   导致分配器反向映射永远只有首片段 glyphIds 的问题。
 *
 * **阶段2**：对完整组装后的组做 cache hit 验证和身份分配。
 *   - cache hit 时验证 allocator.resolve(cached.windowInstanceId) 与组 glyphIds 一致；
 *     不一致则按规范重新规划身份，而非让 Map 与 allocator 各说各话。
 *   - 已完成的 insert 组在规划阶段就被筛除，不分配/注册身份，
 *     避免未挂载的实例被长驻缓存持有。
 *
 * @param piecesWithContext 带有父窗口上下文和运动通道的片段列表
 * @param allocator 窗口实例 ID 分配器
 * @param identityCache 组级身份缓存（跨 retarget 保持稳定）
 * @param owner 身份的生命周期归属（'candidate' 或 'displayed'）
 * @returns 物理组列表（已完成的 insert/reflow 组被筛除）
 */
export function planCarryOverPhysicalGroups(
  piecesWithContext: CarryOverPieceWithContext[],
  allocator: WindowInstanceIdAllocator,
  identityCache: IdentityCache,
  owner: string
): CarryOverPhysicalGroupResult[] {
  // ── 阶段1：按 groupKey 完整收集并合并所有片段 ──
  //
  // 不在此阶段分配身份——先完整收集所有片段，合并 glyphIds/clip/岛几何等，
  // 组确定后才在阶段2用完整 glyph 集合做 allocateFresh。

  /** 阶段1的中间组装结果——不含身份信息 */
  interface GroupAssembly {
    groupKey: string
    sourceKind: 'old' | 'new'
    sourceRevision: number
    motionChannel: 'ghost' | 'insert' | 'reflow'
    parentWindowId: string
    parentRenderNodeKey: string
    glyphIds: string[]
    sourceUtf16Start: number
    sourceUtf16End: number
    clipRects: ClipRect[]
    fullWindowGlyphUtf16Ranges: Map<string, [number, number]> | null
    targetUtf16Start: number
    targetUtf16End: number
    targetResolved: boolean
    targetRect: GlyphRect | null
    glyphX: number
    glyphY: number
    glyphWidth: number
    glyphHeight: number
    clipLeft: number
    clipRight: number
    clipTop: number
    clipBottom: number
    currentPositionX: number
    currentPositionY: number
    offsetX: number
    offsetY: number
    opacity: number | undefined
    islandSourceRects: GlyphRect[]
    islandClipRects: ClipRect[][]
  }

  const assemblyMap = new Map<string, GroupAssembly>()

  for (const { piece, parent, motionChannel } of piecesWithContext) {
    // 跳过完全不可见的片段
    if (piece.clipRects.length === 0) {
      continue
    }

    // 确定目标行索引（reflow 用实际目标位置区分不同轨迹）
    let targetRectForGrouping: GlyphRect | null = null
    if (motionChannel === 'reflow' && piece.targetResolved && piece.targetRect !== null) {
      targetRectForGrouping = piece.targetRect
    }

    // 生成 groupKey（包含所有区分信息）
    const groupKey = generateGroupKey(
      parent.renderNodeKey,
      parent.sourceKind,
      parent.sourceRevision,
      motionChannel,
      targetRectForGrouping
    )

    // 获取 piece 的 fullWindowGlyphUtf16Ranges（按 piece.glyphIds 过滤父窗口映射）
    let pieceFullRanges: Map<string, [number, number]> | null = null
    if (parent.fullWindowGlyphUtf16Ranges !== null) {
      pieceFullRanges = filterGlyphUtf16Ranges(
        parent.fullWindowGlyphUtf16Ranges, piece.glyphIds
      )
    }

    // 合并到已有组装结果或创建新组装结果（不分配身份）
    const existing = assemblyMap.get(groupKey)
    if (existing !== undefined) {
      // 合并：glyphIds 并集
      existing.glyphIds = [...existing.glyphIds, ...piece.glyphIds]
      // sourceUtf16Start/End 取 min/max
      existing.sourceUtf16Start = Math.min(existing.sourceUtf16Start, piece.sourceUtf16Start)
      existing.sourceUtf16End = Math.max(existing.sourceUtf16End, piece.sourceUtf16End)
      // clipRects 并集
      existing.clipRects = [...existing.clipRects, ...piece.clipRects]
      // clipLeft/Right/Top/Bottom 取 min/max
      existing.clipLeft = Math.min(existing.clipLeft, piece.clipLeft)
      existing.clipRight = Math.max(existing.clipRight, piece.clipRight)
      existing.clipTop = Math.min(existing.clipTop, piece.clipTop)
      existing.clipBottom = Math.max(existing.clipBottom, piece.clipBottom)
      // fullWindowGlyphUtf16Ranges 并集
      if (pieceFullRanges !== null) {
        if (existing.fullWindowGlyphUtf16Ranges === null) {
          existing.fullWindowGlyphUtf16Ranges = new Map(pieceFullRanges)
        } else {
          for (const [k, v] of pieceFullRanges) {
            existing.fullWindowGlyphUtf16Ranges.set(k, v)
          }
        }
      }
      // 保留每个岛的源完整矩形和起始 clip
      existing.islandSourceRects.push({
        x: piece.glyphX,
        y: piece.glyphY,
        width: piece.glyphWidth,
        height: piece.glyphHeight
      })
      existing.islandClipRects.push([...piece.clipRects])
      // 合并目标信息时形成完整、连续的组级目标
      if (piece.targetResolved) {
        if (!existing.targetResolved) {
          existing.targetUtf16Start = piece.targetUtf16Start
          existing.targetUtf16End = piece.targetUtf16End
          existing.targetResolved = true
          existing.targetRect = piece.targetRect
        } else {
          existing.targetUtf16Start = Math.min(existing.targetUtf16Start, piece.targetUtf16Start)
          existing.targetUtf16End = Math.max(existing.targetUtf16End, piece.targetUtf16End)
          if (existing.targetRect !== null && piece.targetRect !== null) {
            const minX = Math.min(existing.targetRect.x, piece.targetRect.x)
            const minY = Math.min(existing.targetRect.y, piece.targetRect.y)
            const maxX = Math.max(
              existing.targetRect.x + existing.targetRect.width,
              piece.targetRect.x + piece.targetRect.width
            )
            const maxY = Math.max(
              existing.targetRect.y + existing.targetRect.height,
              piece.targetRect.y + piece.targetRect.height
            )
            existing.targetRect = {
              x: minX, y: minY,
              width: maxX - minX, height: maxY - minY
            }
          } else if (piece.targetRect !== null) {
            existing.targetRect = piece.targetRect
          }
        }
      }
    } else {
      // 创建新组装结果（不含身份信息）
      assemblyMap.set(groupKey, {
        groupKey: groupKey,
        sourceKind: parent.sourceKind,
        sourceRevision: parent.sourceRevision,
        motionChannel: motionChannel,
        parentWindowId: parent.windowId,
        parentRenderNodeKey: parent.renderNodeKey,
        glyphIds: [...piece.glyphIds],
        sourceUtf16Start: piece.sourceUtf16Start,
        sourceUtf16End: piece.sourceUtf16End,
        clipRects: [...piece.clipRects],
        fullWindowGlyphUtf16Ranges: pieceFullRanges,
        targetUtf16Start: piece.targetUtf16Start,
        targetUtf16End: piece.targetUtf16End,
        targetResolved: piece.targetResolved,
        targetRect: piece.targetRect,
        glyphX: piece.glyphX,
        glyphY: piece.glyphY,
        glyphWidth: piece.glyphWidth,
        glyphHeight: piece.glyphHeight,
        clipLeft: piece.clipLeft,
        clipRight: piece.clipRight,
        clipTop: piece.clipTop,
        clipBottom: piece.clipBottom,
        currentPositionX: piece.currentPositionX,
        currentPositionY: piece.currentPositionY,
        offsetX: piece.offsetX,
        offsetY: piece.offsetY,
        opacity: parent.opacity,
        islandSourceRects: [{
          x: piece.glyphX,
          y: piece.glyphY,
          width: piece.glyphWidth,
          height: piece.glyphHeight
        }],
        islandClipRects: [[...piece.clipRects]],
      })
    }
  }

  // ── 阶段2：筛除已完成的 insert/reflow 组，为剩余组分配身份 ──
  //
  // Issue #879 评论6158702656 缺口3：规划阶段先筛除确实已完成、不会输出的组，
  // 再分配/注册身份。避免从未进入输出 run、没有 ArkUI onAppear 租约的实例
  // 被长驻缓存持有，无明确释放路径。
  //
  // Issue #879 评论6159166760 问题2：reflow 完成判定也移到身份分配前。
  // 旧逻辑只筛除已完成的 insert 组，reflow 组无论是否完成都先分配身份，
  // 然后 Coordinator 又跳过已完成的 reflow 组，留下从未挂载的实例。

  const results: CarryOverPhysicalGroupResult[] = []

  for (const assembly of assemblyMap.values()) {
    // 筛除已完成的 insert 组——它们的 clipRects 已完全覆盖所有岛的源完整矩形
    if (assembly.motionChannel === 'insert' &&
        isInsertGroupComplete(
          assembly.islandSourceRects,
          assembly.islandClipRects,
          assembly.offsetX,
          assembly.offsetY
        )) {
      // 已完成的 insert 组不分配身份、不输出——静态正文已接管这些字形
      continue
    }

    // Issue #879 评论6159166760 问题2：筛除已完成的 reflow 组——
    // 当前位置已到达目标、透明度已到终态、clip 已完全覆盖。
    // 使用与 Coordinator 共用的 isReflowGroupComplete 函数，确保判定一致。
    if (assembly.motionChannel === 'reflow' &&
        isReflowGroupComplete(
          assembly.currentPositionX,
          assembly.currentPositionY,
          assembly.targetRect,
          assembly.opacity,
          assembly.sourceKind,
          assembly.clipRects
        )) {
      // 已完成的 reflow 组不分配身份、不输出——静态正文已接管这些字形
      continue
    }

    // 为剩余组分配/复用稳定身份
    let renderNodeKey: string
    let windowInstanceId: string

    // Issue #879 评论6159920940：IdentityCache 改为 groupKey→owner→entry 多 owner 并存结构。
    // cache hit 时先查内层 Map 中当前 owner 的条目；如果当前 owner 有条目且有效（glyphIds 一致），
    // 复用；如果当前 owner 没有条目但其他 owner 有，不能覆盖——需要为新 owner 分配新实例，
    // 两个条目并存。这修复了旧逻辑中 owner 不同直接覆盖 groupKey 导致旧 instanceId 成为孤儿的问题。
    let ownerMap = identityCache.get(assembly.groupKey)
    const cached = ownerMap?.get(owner)
    if (cached !== undefined) {
      // Issue #879 评论6158702656 缺口3：cache hit 时验证实例仍有效。
      // 分配器是唯一的活跃实例真相，cache hit 不能单靠字符串存在就宣称实例仍有效。
      // 检查 allocator.resolve(cached.windowInstanceId) 返回的 glyphIds
      // 是否与当前组的 glyphIds 一致；不一致说明实例已被释放并重新分配给其他组，
      // 必须重新规划身份。
      const resolvedGlyphIds = allocator.resolve(cached.windowInstanceId)
      if (resolvedGlyphIds !== null &&
          resolvedGlyphIds.length === assembly.glyphIds.length &&
          resolvedGlyphIds.every((gid, i) => gid === assembly.glyphIds[i])) {
        // 实例仍有效、glyphIds 一致 → 复用
        renderNodeKey = cached.renderNodeKey
        windowInstanceId = cached.windowInstanceId
      } else {
        // 实例已释放或 glyphIds 不一致 → 重新分配
        windowInstanceId = allocator.allocateFresh(assembly.glyphIds)
        renderNodeKey = generateRenderNodeKey(
          assembly.sourceKind,
          assembly.sourceRevision,
          assembly.motionChannel,
          windowInstanceId
        )
        ownerMap!.set(owner, { renderNodeKey, windowInstanceId })
      }
    } else {
      // Issue #879 评论6159920940：当前 owner 没有条目（可能是 cache miss，
      // 也可能是其他 owner 有条目）。不能覆盖其他 owner 的条目——需要为新 owner
      // 分配新实例，两个条目并存。
      windowInstanceId = allocator.allocateFresh(assembly.glyphIds)
      renderNodeKey = generateRenderNodeKey(
        assembly.sourceKind,
        assembly.sourceRevision,
        assembly.motionChannel,
        windowInstanceId
      )
      if (ownerMap === undefined) {
        // cache miss → 创建新的内层 Map
        ownerMap = new Map<string, IdentityCacheEntry>()
        identityCache.set(assembly.groupKey, ownerMap)
      }
      ownerMap.set(owner, { renderNodeKey, windowInstanceId })
    }

    // 输出完整物理组
    results.push({
      renderNodeKey: renderNodeKey,
      windowInstanceId: windowInstanceId,
      sourceKind: assembly.sourceKind,
      sourceRevision: assembly.sourceRevision,
      motionChannel: assembly.motionChannel,
      parentWindowId: assembly.parentWindowId,
      sourceRunId: `${assembly.sourceKind}-${assembly.sourceRevision}`,
      glyphIds: assembly.glyphIds,
      sourceUtf16Start: assembly.sourceUtf16Start,
      sourceUtf16End: assembly.sourceUtf16End,
      clipRects: assembly.clipRects,
      fullWindowGlyphUtf16Ranges: assembly.fullWindowGlyphUtf16Ranges,
      targetUtf16Start: assembly.targetUtf16Start,
      targetUtf16End: assembly.targetUtf16End,
      targetResolved: assembly.targetResolved,
      targetRect: assembly.targetRect,
      glyphX: assembly.glyphX,
      glyphY: assembly.glyphY,
      glyphWidth: assembly.glyphWidth,
      glyphHeight: assembly.glyphHeight,
      clipLeft: assembly.clipLeft,
      clipRight: assembly.clipRight,
      clipTop: assembly.clipTop,
      clipBottom: assembly.clipBottom,
      currentPositionX: assembly.currentPositionX,
      currentPositionY: assembly.currentPositionY,
      offsetX: assembly.offsetX,
      offsetY: assembly.offsetY,
      opacity: assembly.opacity,
      islandSourceRects: assembly.islandSourceRects,
      islandClipRects: assembly.islandClipRects,
    })
  }

  return results
}

// ── 辅助函数 ──

/**
 * 按 glyphIds 过滤父窗口的 glyphId→UTF-16 映射。
 * 子片段只携带属于自己的 glyph 身份所有权，不再复制原父窗口全表。
 */
function filterGlyphUtf16Ranges(
  ranges: Map<string, [number, number]>,
  glyphIds: string[]
): Map<string, [number, number]> | null {
  const filtered = new Map<string, [number, number]>()
  for (const gid of glyphIds) {
    const r = ranges.get(gid)
    if (r !== undefined) {
      filtered.set(gid, r)
    }
  }
  return filtered.size > 0 ? filtered : null
}
