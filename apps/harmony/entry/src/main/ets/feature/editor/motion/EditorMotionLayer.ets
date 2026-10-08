// EditorMotionLayer — 运动层绘制。
//
// Issue #879：使用 clipShape/maskShape 对真实 ArkUI Text 绘制结果做遮罩。
//
// 核心原则：
// 1. 不用透明字色伪装遮罩——用真实像素裁切（clipShape/maskShape）。
// 2. 不用 Canvas.fillText 临摹——对真实 ArkUI Text 组件做遮罩。
// 3. 同一字形在每一帧只能由静态正文或运动层绘制一次。
// 4. 光标位置 = 裁切前沿 front。
//
// 各运动类型的绘制策略：
// - 吐字（Reveal）：新字完整排版，静态目标先不画，运动层用裁切窗口限制在 [x0, front]。
// - 吞字（Conceal）：旧字源留在动画层，可见窗口 [x0, front]，front 从 x1 退到 x0。
// - Forward Delete：逻辑光标不移动，被删字贴向固定 caret 缩回。
// - 纯光标移动：不建立字形裁切。
// - 跨行：按真实视觉行拆段。
// - 自动换行：目标静态字形让位，运动层平移。

import { EditorVisualFrame, MotionKind, MotionGlyphWindow, StaticVisibleRange } from './EditorVisualFrame'
import { CaretRect, LineLayout } from '../render/editor_render_geometry'

/**
 * 裁切矩形——用于 clipShape/maskShape 的矩形区域。
 */
export interface ClipRect {
  /** 左边界（vp） */
  x: number
  /** 上边界（vp） */
  y: number
  /** 宽度（vp） */
  width: number
  /** 高度（vp） */
  height: number
}

/**
 * 运动层绘制工具。
 *
 * 提供 clipShape/maskShape 所需的裁切矩形计算、静态层可见区域计算、
 * 光标位置计算等纯函数。ArkUI 组件层调用这些函数获取裁切参数，
 * 再用 Stack + clipShape 组合实现真实像素裁切效果。
 *
 * 本类只做数学计算，不直接操作 ArkUI 组件——组件组装由调用方完成。
 */
export class EditorMotionLayer {
  /**
   * 构建裁切矩形。
   *
   * 从 MotionGlyphWindow 的 clipLeft/clipRight/clipTop/clipBottom
   * 构建一个 ClipRect，用于 clipShape/maskShape。
   *
   * 裁切矩形的语义：
   * - clipLeft → x
   * - clipTop → y
   * - clipRight - clipLeft → width
   * - clipBottom - clipTop → height
   *
   * width/height 不允许为负——clipRight < clipLeft 或 clipBottom < clipTop
   * 时返回零尺寸矩形（该字形在本帧不可见）。
   */
  static buildClipRect(window: MotionGlyphWindow): ClipRect {
    const x = window.clipLeft
    const y = window.clipTop
    const width = Math.max(0, window.clipRight - window.clipLeft)
    const height = Math.max(0, window.clipBottom - window.clipTop)
    return { x, y, width, height }
  }

  /**
   * 计算静态层可见区域。
   *
   * 从 EditorVisualFrame 的 staticVisibleRanges 和行布局信息出发，
   * 计算静态正文层在本帧应该绘制的字形范围。
   *
   * 运动层负责的字形范围（motionGlyphWindows 中的 utf16Start/utf16End）
   * 不包含在静态层可见范围内——同一字形每帧只画一次。
   *
   * 如果 frame.isAnimating 为 false，所有字形都由静态层绘制。
   */
  static computeStaticVisibleRanges(
    frame: EditorVisualFrame,
    lineLayouts: LineLayout[]
  ): StaticVisibleRange[] {
    // 非动画状态：所有字形归静态层
    if (!frame.isAnimating || frame.motionKind === MotionKind.None) {
      if (lineLayouts.length === 0) {
        return []
      }
      return [{
        utf16Start: lineLayouts[0].startUtf16,
        utf16End: lineLayouts[lineLayouts.length - 1].endUtf16
      }]
    }

    // 动画状态：从 staticVisibleRanges 中排除运动层负责的范围
    const motionRanges: StaticVisibleRange[] = frame.motionGlyphWindows.map(
      (w: MotionGlyphWindow): StaticVisibleRange => ({
        utf16Start: w.utf16Start,
        utf16End: w.utf16End
      })
    )

    return EditorMotionLayer.subtractRanges(
      frame.staticVisibleRanges,
      motionRanges
    )
  }

  /**
   * 从视觉帧计算光标位置。
   *
   * 光标位置 = 裁切前沿 front。
   * - 吐字：front 在新字右侧（从 x0 推进到 x1）。
   * - 吞字：front 在旧字左侧（从 x1 退到 x0）。
   * - Forward Delete：front 在固定 caret 位置（逻辑光标不动）。
   * - 纯光标移动：front = 光标插值位置。
   * - 无动画：front = 静态光标位置。
   *
   * 返回 CaretRect 供 ArkUI 绘制光标矩形。
   */
  static computeCaretFromFrame(frame: EditorVisualFrame): CaretRect {
    return {
      x: frame.caretX,
      y: frame.caretY,
      width: 2, // CARET_WIDTH_VP
      height: frame.caretHeight
    }
  }

  /**
   * 判断某字形范围是否在运动层窗口中。
   *
   * 用于静态层决定是否跳过某个字形的绘制——
   * 如果该字形在运动层窗口中，静态层不画（避免同一字形画两次）。
   */
  static isGlyphInMotionWindow(
    utf16Start: number,
    utf16End: number,
    frame: EditorVisualFrame
  ): boolean {
    for (const window of frame.motionGlyphWindows) {
      // 范围有交集 → 在运动层窗口中
      if (utf16Start < window.utf16End && utf16End > window.utf16Start) {
        return true
      }
    }
    return false
  }

  /**
   * 计算运动层需要绘制的字形窗口的裁切矩形列表。
   *
   * 每个 MotionGlyphWindow 对应一个 ClipRect，
   * ArkUI 组件层用 Stack + 多个 clipShape 层叠实现多窗口裁切。
   */
  static buildAllClipRects(frame: EditorVisualFrame): ClipRect[] {
    return frame.motionGlyphWindows.map(
      (w: MotionGlyphWindow): ClipRect => EditorMotionLayer.buildClipRect(w)
    )
  }

  /**
   * 判断当前帧是否需要运动层绘制。
   *
   * isAnimating 且 motionGlyphWindows 非空时才需要运动层。
   * 纯光标移动（CaretMove）不需要运动层——光标直接在静态层上方绘制。
   */
  static needsMotionLayer(frame: EditorVisualFrame): boolean {
    if (!frame.isAnimating) {
      return false
    }
    if (frame.motionKind === MotionKind.CaretMove) {
      return false
    }
    return frame.motionGlyphWindows.length > 0
  }

  /**
   * 判断当前帧是否需要单独绘制光标。
   *
   * 动画中（isAnimating）时，光标由运动层绘制（绑定裁切前沿）。
   * 非动画时，光标由静态层绘制。
   */
  static needsSeparateCaret(frame: EditorVisualFrame): boolean {
    return frame.isAnimating
  }

  // ── 私有方法 ──

  /**
   * 从可见范围列表中减去排除范围列表。
   * 与 EditorMotionCoordinator.subtractRanges 逻辑一致，独立维护避免循环依赖。
   */
  private static subtractRanges(
    visible: StaticVisibleRange[],
    excluded: StaticVisibleRange[]
  ): StaticVisibleRange[] {
    if (excluded.length === 0) {
      return visible
    }
    const result: StaticVisibleRange[] = []
    for (const vis of visible) {
      let start = vis.utf16Start
      const end = vis.utf16End
      for (const exc of excluded) {
        if (exc.utf16End <= start || exc.utf16Start >= end) {
          continue
        }
        if (exc.utf16Start > start) {
          result.push({ utf16Start: start, utf16End: exc.utf16Start })
        }
        start = Math.max(start, exc.utf16End)
      }
      if (start < end) {
        result.push({ utf16Start: start, utf16End: end })
      }
    }
    return result
  }
}
