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
 * 组级身份缓存——从 groupKey 到 {renderNodeKey, windowInstanceId} 的映射。
 *
 * 在 EditorMotionCoordinator 实例上维护，跨 retarget 保持稳定：
 * - 同一组跨帧/retarget → 查到已有身份，复用同一 renderNodeKey 和 windowInstanceId
 * - 新组 → 通过 WindowInstanceIdAllocator.allocateFresh 分配新身份
 *
 * 这确保了：
 * - 同一候选多次 retarget 不漂移（身份不变）
 * - 不同物理组拥有不同且固定的身份
 * - windowInstanceId 在分配器中登记，支持正确的租约管理
 */
export type IdentityCache = Map<string, { renderNodeKey: string; windowInstanceId: string }>

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
 * Issue #879 评论6153434157 问题3+4：carry-over 物理窗口规划入口。
 *
 * 将拆片后的片段按物理 Text 分组，为每组分配唯一且固定的身份：
 * - 同源、同运动通道、同轨迹 → 合并为一个组（一张 Text）
 * - 不同轨迹 → 分配不同且固定的 PhysicalWindowIdentity
 *
 * 身份分配通过 WindowInstanceIdAllocator 进行：
 * - 新组 → allocateFresh 分配新 windowInstanceId
 * - 同一组 retarget → 从 identityCache 查到已有身份，复用
 *
 * 合并后的组拥有完整的目标信息（所有 piece 的并集），不只取第一个 piece 的值。
 *
 * @param piecesWithContext 带有父窗口上下文和运动通道的片段列表
 * @param allocator 窗口实例 ID 分配器
 * @param identityCache 组级身份缓存（跨 retarget 保持稳定）
 * @returns 物理组列表
 */
export function planCarryOverPhysicalGroups(
  piecesWithContext: CarryOverPieceWithContext[],
  allocator: WindowInstanceIdAllocator,
  identityCache: IdentityCache
): CarryOverPhysicalGroupResult[] {
  const groupMap = new Map<string, CarryOverPhysicalGroupResult>()

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

    // 生成或复用稳定身份
    let renderNodeKey: string
    let windowInstanceId: string
    const cached = identityCache.get(groupKey)
    if (cached !== undefined) {
      // 同一组 retarget → 复用已有身份
      renderNodeKey = cached.renderNodeKey
      windowInstanceId = cached.windowInstanceId
    } else {
      // Issue #879 评论6158342861 问题2：cache miss 时先调用 allocator.allocateFresh
      // 得到真正的子窗口实例 ID，再用它生成 renderNodeKey。
      // 不把没有全局唯一性的父 windowInstanceId 当子节点 key 的唯一依据。
      windowInstanceId = allocator.allocateFresh(piece.glyphIds)
      renderNodeKey = generateRenderNodeKey(
        parent.sourceKind,
        parent.sourceRevision,
        motionChannel,
        windowInstanceId
      )
      identityCache.set(groupKey, { renderNodeKey, windowInstanceId })
    }

    // 获取 piece 的 fullWindowGlyphUtf16Ranges（按 piece.glyphIds 过滤父窗口映射）
    let pieceFullRanges: Map<string, [number, number]> | null = null
    if (parent.fullWindowGlyphUtf16Ranges !== null) {
      pieceFullRanges = filterGlyphUtf16Ranges(
        parent.fullWindowGlyphUtf16Ranges, piece.glyphIds
      )
    }

    // 合并到已有组或创建新组
    const existing = groupMap.get(groupKey)
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
      // Issue #879 评论6158342861 问题3：合并时保留每个岛的源完整矩形和起始 clip
      existing.islandSourceRects.push({
        x: piece.glyphX,
        y: piece.glyphY,
        width: piece.glyphWidth,
        height: piece.glyphHeight
      })
      existing.islandClipRects.push([...piece.clipRects])
      // Issue #879 评论6153434157 问题4：合并目标信息时形成完整、连续的组级目标
      // 不只取第一个 piece 的目标值，也不只在 targetResolved=false 时替换
      if (piece.targetResolved) {
        if (!existing.targetResolved) {
          // 之前未解析，现在有了解析结果——直接采用
          existing.targetUtf16Start = piece.targetUtf16Start
          existing.targetUtf16End = piece.targetUtf16End
          existing.targetResolved = true
          existing.targetRect = piece.targetRect
        } else {
          // 之前已有解析结果——合并为完整连续范围
          existing.targetUtf16Start = Math.min(existing.targetUtf16Start, piece.targetUtf16Start)
          existing.targetUtf16End = Math.max(existing.targetUtf16End, piece.targetUtf16End)
          // targetRect 取包围盒（同组 reflow 片段目标位置相同，包围盒 = 并集）
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
      // 创建新组
      groupMap.set(groupKey, {
        renderNodeKey: renderNodeKey,
        windowInstanceId: windowInstanceId,
        sourceKind: parent.sourceKind,
        sourceRevision: parent.sourceRevision,
        motionChannel: motionChannel,
        parentWindowId: parent.windowId,
        sourceRunId: `${parent.sourceKind}-${parent.sourceRevision}`,
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
        // Issue #879 评论6158342861 问题3：保留每个视觉岛的源完整矩形和起始 clip
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

  return Array.from(groupMap.values())
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
