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
// Issue #879 复核评论修复：
// - 添加 MotionTextOverlayBuilder() Builder 函数，在 ArkUI 组件树中渲染运动层
// - 每个 MotionGlyphWindow 对应一个真实 ArkUI Text + clipShape 裁切
// - 吐字：新字源完整排版，clipShape 限制在 [x0, front] 裁切窗口
// - 吞字：旧字源完整排版，clipShape 限制在 [x0, front] 裁切窗口
// - 非动画状态：不渲染任何内容（静态层负责）
//
// ArkTS 限制：@Component struct 不能在非页面 .ets 文件中定义，
// 所以 MotionTextOverlay 以 Builder 函数形式提供，由 SujianEditor 在 build() 中调用。

import { EditorVisualFrame, MotionKind, MotionGlyphWindow, StaticVisibleRange } from './EditorVisualFrame'
import { CaretRect, LineLayout } from '../render/editor_render_geometry'
import { RectShape } from '@ohos.arkui.shape'

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
 * 光标位置计算等纯函数，以及 MotionTextOverlayBuilder() Builder 函数
 * 用于在 ArkUI 组件树中渲染运动层 Text + clipShape。
 *
 * 纯函数部分不直接操作 ArkUI 组件——组件组装由 Builder 函数完成。
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

// ── ArkUI Builder 函数 ──

/**
 * MotionTextOverlayBuilder 的参数类型。
 * ArkTS 要求 @Builder 函数的参数类型必须是显式声明的 interface 或 class，
 * 不能用对象字面量类型。
 */
export interface MotionTextOverlayParams {
  /** 旧排版源纯文本（吞字时显示旧字），null 表示不需要旧字源 */
  oldTextContent: string | null
  /** 新排版源纯文本（吐字时显示新字），null 表示不需要新字源 */
  newTextContent: string | null
  /** 当前视觉帧 */
  visualFrame: EditorVisualFrame
  /** 裁切矩形列表（与 motionGlyphWindows 一一对应） */
  clipRects: ClipRect[]
  /** 字体大小（vp） */
  fontSize: number
  /** 行高（vp） */
  lineHeight: number
}

/**
 * 运动层文字遮罩 Builder。
 *
 * 在 ArkUI 组件树中渲染运动层——对真实 ArkUI Text 组件做 clipShape 裁切，
 * 实现吐字/吞字的像素级遮罩效果。
 *
 * 调用方式：在 SujianEditor 的 build() 中，当 EditorMotionLayer.needsMotionLayer(frame) 为 true 时调用：
 * ```
 * if (EditorMotionLayer.needsMotionLayer(this.visualFrame)) {
 *   MotionTextOverlayBuilder({
 *     oldTextContent: this.previousSnapshot?.text ?? null,
 *     newTextContent: this.currentSnapshot?.text ?? null,
 *     visualFrame: this.visualFrame,
 *     clipRects: EditorMotionLayer.buildAllClipRects(this.visualFrame),
 *     fontSize: this.currentSnapshot?.fontSize ?? 16,
 *     lineHeight: (this.currentSnapshot?.fontSize ?? 16) * (this.currentSnapshot?.lineSpacingMultiplier ?? 1.8)
 *   })
 * }
 * ```
 *
 * 渲染策略：
 * - 吐字（Reveal）：新字源完整排版，clipShape 限制在 [x0, front] 裁切窗口
 * - 吞字（Conceal）：旧字源完整排版，clipShape 限制在 [x0, front] 裁切窗口
 * - Forward Delete：旧字源完整排版，clipShape 限制在 [caret, front] 裁切窗口
 * - 非动画：不渲染任何内容（静态层负责）
 *
 * ArkTS 限制：@Component struct 不能在非页面 .ets 文件中定义，
 * 所以以 Builder 函数形式提供，由 SujianEditor 在 build() 中调用。
 *
 * @param params.oldTextContent 旧排版源纯文本（吞字时显示旧字），null 表示不需要旧字源
 * @param params.newTextContent 新排版源纯文本（吐字时显示新字），null 表示不需要新字源
 * @param params.visualFrame 当前视觉帧
 * @param params.clipRects 裁切矩形列表（与 motionGlyphWindows 一一对应）
 * @param params.fontSize 字体大小（vp）
 * @param params.lineHeight 行高（vp）
 */
@Builder
export function MotionTextOverlayBuilder(params: MotionTextOverlayParams) {
  // 条件渲染：非动画状态或无运动字形窗口时不渲染任何内容
  if (params.visualFrame.isAnimating && params.visualFrame.motionGlyphWindows.length > 0) {
    // 用 Stack 层叠多个 Text 组件，每个对应一个 MotionGlyphWindow
    Stack() {
      // 遍历 motionGlyphWindows，为每个窗口创建一个带 clipShape 的 Text
      ForEach(
        params.visualFrame.motionGlyphWindows,
        (window: MotionGlyphWindow, index: number) => {
          // 根据 sourceKind 选择对应的文字源
          // 吐字（Reveal）：sourceKind='new'，用 newTextContent
          // 吞字（Conceal/ConcealForward）：sourceKind='old'，用 oldTextContent
          // Reflow：sourceKind='new'，用 newTextContent
          if (window.sourceKind === 'new' && params.newTextContent !== null) {
            // 新字源：完整排版，用 Stack + offset + clipShape 限制可见区域
            // RectShape 不支持 x/y 偏移，用 Stack 包裹 Text 并用 offset 移动 Text 使裁切窗口对齐 Stack 左上角
            Stack() {
              Text(params.newTextContent)
                .fontSize(params.fontSize)
                .lineHeight(params.lineHeight)
                .offset({ x: -params.clipRects[index].x, y: -params.clipRects[index].y })
            }
            .width(params.clipRects[index].width)
            .height(params.clipRects[index].height)
            .clipShape(new RectShape({
              width: params.clipRects[index].width,
              height: params.clipRects[index].height
            }))
            .position({
              x: params.clipRects[index].x,
              y: params.clipRects[index].y
            })
          } else if (window.sourceKind === 'old' && params.oldTextContent !== null) {
            // 旧字源：完整排版，用 Stack + offset + clipShape 限制可见区域
            Stack() {
              Text(params.oldTextContent)
                .fontSize(params.fontSize)
                .lineHeight(params.lineHeight)
                .offset({ x: -params.clipRects[index].x, y: -params.clipRects[index].y })
            }
            .width(params.clipRects[index].width)
            .height(params.clipRects[index].height)
            .clipShape(new RectShape({
              width: params.clipRects[index].width,
              height: params.clipRects[index].height
            }))
            .position({
              x: params.clipRects[index].x,
              y: params.clipRects[index].y
            })
          }
        },
        (window: MotionGlyphWindow, index: number) => `${window.sourceKind}-${index}`
      )
    }
    .width('100%')
    .height('100%')
    .alignContent(Alignment.TopStart)
  }
}
