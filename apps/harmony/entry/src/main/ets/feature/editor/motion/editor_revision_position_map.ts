// editor_revision_position_map.ts — 显示版坐标 → Core 最新版坐标的位置映射。
//
// Issue #879 最新复核评论问题2：两阶段显示期间，主 Text / 命中测试用的还是
// 屏幕上的旧版本，而 Core 已经推进到新版本。点击得到的 UTF-16 offset 是**显示版**
// 坐标，直接发给 dispatcher 会被按 Core 最新正文解释，长文中部删除/换行后必然错位。
//
// 本模块按事务链逐笔搬运一个位置（caret / 选区端点）：
// - 位置落在替换范围之前：不受影响。
// - 位置落在替换范围之后：加上该笔的净长度变化。
// - 位置落在被替换范围内部：贴到替换文本的对应位置（纯删除时贴到删除点），
//   与 Core 的 caret 语义一致（caret 不会停在不存在的字符里）。
//
// 纯逻辑：不依赖 ArkUI、不 import .ets，生产由 SujianEditor 调用，Node 单测直接 import。

import type { GlyphCarryPatch, Utf16CarryPatch } from './editor_patch_carry.ts'
import { applyPatchesToText, toUtf16CarryPatches } from './editor_patch_carry.ts'

/** 事务链中的一笔——该笔前后的正文和它的 patch。 */
export interface PositionMapStep {
  /** 该笔事务开始前的正文 */
  beforeText: string
  /** 该笔事务结束后的正文 */
  afterText: string
  /** 该笔事务的 patch（坐标对应该笔开始前的正文） */
  patches: GlyphCarryPatch[]
}

/**
 * 显示版 → Core 最新版的 UTF-16 位置映射。
 *
 * 只描述位置搬运，不改正文；映射失败（链不连续、patch 越界、文本不符）时
 * build 返回 null，调用方必须放弃映射而不是猜一个偏移。
 */
export class RevisionPositionMap {
  /** 起始 revision（屏幕显示版） */
  readonly fromRevision: number
  /** 结束 revision（Core 最新版） */
  readonly toRevision: number
  /** 每笔事务的 UTF-16 指令（已校验） */
  private readonly steps: Utf16CarryPatch[][]

  private constructor(fromRevision: number, toRevision: number, steps: Utf16CarryPatch[][]) {
    this.fromRevision = fromRevision
    this.toRevision = toRevision
    this.steps = steps
  }

  /**
   * 校验并建立位置映射。
   *
   * 每一笔都要求 patch 落在合法 UTF-8 字符边界、且重放结果与该笔给出的 afterText 完全一致。
   * 任何一笔不满足就返回 null——不允许用“长度相同”证明是同一份文字。
   */
  static build(
    steps: PositionMapStep[],
    fromRevision: number,
    toRevision: number
  ): RevisionPositionMap | null {
    if (steps.length === 0) {
      return null
    }
    const compiled: Utf16CarryPatch[][] = []
    for (const step of steps) {
      const ops = toUtf16CarryPatches(step.beforeText, step.patches)
      if (ops === null) {
        return null
      }
      if (applyPatchesToText(step.beforeText, step.patches) !== step.afterText) {
        return null
      }
      compiled.push(ops)
    }
    return new RevisionPositionMap(fromRevision, toRevision, compiled)
  }

  /**
   * 把一个 UTF-16 位置从显示版映射到 Core 最新版。
   *
   * @param offsetUtf16 显示版坐标下的位置
   * @returns Core 最新版坐标下的位置；越界会被夹到合法范围
   */
  mapOffset(offsetUtf16: number): number {
    let pos = offsetUtf16 < 0 ? 0 : offsetUtf16
    for (const ops of this.steps) {
      pos = RevisionPositionMap.mapThroughStep(ops, pos)
    }
    return pos
  }

  /**
   * 在一笔事务内搬运位置。
   */
  private static mapThroughStep(ops: Utf16CarryPatch[], offsetUtf16: number): number {
    let delta = 0
    for (const op of ops) {
      const removed = op.replaceEndUtf16 - op.replaceStartUtf16
      const inserted = op.insertedText.length
      if (offsetUtf16 <= op.replaceStartUtf16) {
        // 位置在这个替换之前——后面的替换不再影响它
        break
      }
      if (offsetUtf16 >= op.replaceEndUtf16) {
        delta += inserted - removed
        continue
      }
      // 位置落在被替换范围内部：贴到替换文本里（纯删除时贴到删除点）
      const inside = offsetUtf16 - op.replaceStartUtf16
      return op.replaceStartUtf16 + delta + Math.min(inside, inserted)
    }
    return offsetUtf16 + delta
  }
}
