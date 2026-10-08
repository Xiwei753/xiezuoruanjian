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
// Issue #879 本轮复核评论修复：
// - 问题1：添加 computeStaticMaskPath/computeStaticMaskRects 方法，
//   从 EditorVisualFrame.staticVisibleRanges 和行布局生成静态可见裁切区域。
//   主 Text 仍完整排版，但其绘制输出按 staticVisibleRanges 用 clipShape 裁切——
//   只显示未被运动接管的范围。不再用背景色 Rect 遮罩。
// - 问题2：MotionTextOverlayParams 改为接收 oldTextController/newTextController
//   + oldStyledString/newStyledString（StyledString 类型）+ 布局宽度 + 排版身份，
//   不再接收 oldTextContent/newTextContent 纯字符串。
//   Builder 内部用 Text(undefined, {controller}) 显示真实原始排版。
//   旧、新两份 Text 的字体、行高、wordBreak(BREAK_ALL)、首行缩进、宽度、坐标原点
//   必须分别与对应 revision 的主布局完全一致。
//   clipShape 作用在绘制输出上，不能因 clip 容器缩窄而改变 Text 的排版宽度
//   （Text 的 width 设为完整排版宽度，Stack 外层做 clip）。
//   不能在每一帧重新 setStyledString——只在运动开始时设置一次（由 SujianEditor 负责）。
//
// ArkTS 限制：@Component struct 不能在非页面 .ets 文件中定义，
// 所以 MotionTextOverlay 以 Builder 函数形式提供，由 SujianEditor 在 build() 中调用。

import { EditorVisualFrame, MotionKind, MotionGlyphWindow, StaticVisibleRange } from './EditorVisualFrame'
import { CaretRect, LineLayout } from '../render/editor_render_geometry'
import { RectShape, PathShape } from '@ohos.arkui.shape'

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
 * Issue #879 本轮复核评论修复（问题1）：
 * 添加 computeStaticMaskPath/computeStaticMaskRects 方法，
 * 从 staticVisibleRanges 和行布局生成静态可见裁切区域（用 PathShape 组合多矩形）。
 */
export class EditorMotionLayer {
  /**
   * 构建裁切矩形。
   *
   * 从 MotionGlyphWindow 的 clipLeft/clipRight/clipTop/clipBottom
   * 构建一个 ClipRect，用于 clipShape/maskShape。
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
   * 计算静态可见区 mask 矩形列表。
   *
   * Issue #879 本轮复核评论修复（问题1）：
   * 从 EditorVisualFrame.staticVisibleRanges 和行布局生成静态可见裁切矩形列表。
   * 主 Text 仍完整排版，但其绘制输出按这些矩形用 clipShape 裁切——
   * 只显示未被运动接管的范围。
   *
   * 每个 StaticVisibleRange 可能跨多行，按行拆成多个矩形。
   * 非动画时返回单个全尺寸矩形（不裁切，全显示）。
   *
   * @param frame 当前视觉帧
   * @param lineLayouts 行布局（用于把 UTF-16 offset 范围转成几何矩形）
   * @param fullWidth 全宽（vp，非动画时用）
   * @param fullHeight 全高（vp，非动画时用）
   * @returns 裁切矩形列表（供 buildStaticMaskPath 构造 PathShape）
   */
  static computeStaticMaskRects(
    frame: EditorVisualFrame,
    lineLayouts: LineLayout[],
    fullWidth: number,
    fullHeight: number
  ): ClipRect[] {
    // 非动画状态：全显示（单个全尺寸矩形）
    if (!frame.isAnimating || frame.motionKind === MotionKind.None) {
      return [{ x: 0, y: 0, width: fullWidth, height: fullHeight }]
    }

    // 动画状态：把每个 staticVisibleRange 按行拆成矩形
    const rects: ClipRect[] = []
    for (const range of frame.staticVisibleRanges) {
      for (const line of lineLayouts) {
        // 行与区间无交集
        if (line.endUtf16 <= range.utf16Start || line.startUtf16 >= range.utf16End) {
          continue
        }
        const segStart = Math.max(range.utf16Start, line.startUtf16)
        const segEnd = Math.min(range.utf16End, line.endUtf16)
        if (segStart >= segEnd) {
          continue
        }
        // 用 caretStops 找起始 x 和结束 x
        let x = line.left
        for (const stop of line.caretStops) {
          if (stop.utf16Offset <= segStart) {
            x = stop.x
          }
        }
        let endX = x
        for (const stop of line.caretStops) {
          if (stop.utf16Offset <= segEnd) {
            endX = stop.x
          }
        }
        rects.push({
          x,
          y: line.y,
          width: Math.max(0, endX - x),
          height: line.height
        })
      }
    }
    return rects
  }

  /**
   * 从矩形列表构建 SVG path 命令字符串（供 PathShape commands 参数）。
   *
   * 每个矩形生成一段 M...L...L...L...Z 路径，多矩形用 fillRule=evenodd 组合。
   *
   * @param rects 矩形列表
   * @returns SVG path 命令字符串
   */
  static buildStaticMaskPath(rects: ClipRect[]): string {
    if (rects.length === 0) {
      // 空路径——不可见
      return 'M0 0 Z'
    }
    const parts: string[] = []
    for (const r of rects) {
      if (r.width <= 0 || r.height <= 0) {
        continue
      }
      const x = r.x
      const y = r.y
      const w = r.width
      const h = r.height
      // 矩形路径：M x y L x+w y L x+w y+h L x y+h Z
      parts.push(`M${x} ${y} L${x + w} ${y} L${x + w} ${y + h} L${x} ${y + h} Z`)
    }
    if (parts.length === 0) {
      return 'M0 0 Z'
    }
    return parts.join(' ')
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

  /**
   * 判断当前帧是否需要对主 Text 做静态可见区裁切。
   *
   * Issue #879 本轮复核评论修复（问题1）：
   * 动画中且 needsMotionLayer 时，主 Text 需要按 staticVisibleRanges 裁切，
   * 排除被运动接管的字形。非动画时不裁切（全显示）。
   */
  static needsStaticMask(frame: EditorVisualFrame): boolean {
    return EditorMotionLayer.needsMotionLayer(frame)
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
 *
 * Issue #879 本轮复核评论修复（问题2）：
 * - 不再接收 oldTextContent/newTextContent 纯字符串。
 * - 改为接收 oldTextController/newTextController（TextController）+
 *   oldStyledString/newStyledString（StyledString，用于判断是否需要渲染）+
 *   布局宽度 + 排版身份。
 * - Builder 内部用 Text(undefined, {controller}) 显示真实原始排版。
 * - 旧、新两份 Text 的字体、行高、wordBreak(BREAK_ALL)、首行缩进、宽度、坐标原点
 *   必须分别与对应 revision 的主布局完全一致（通过 TextController.setStyledString 设置）。
 * - 不能在每一帧重新 setStyledString——只在运动开始时设置一次（由 SujianEditor 负责）。
 */
export interface MotionTextOverlayParams {
  /** 旧排版源 TextController（吞字时显示旧字），null 表示不需要旧字源 */
  oldTextController: TextController | null
  /** 新排版源 TextController（吐字时显示新字），null 表示不需要新字源 */
  newTextController: TextController | null
  /** 旧排版源 StyledString 是否已设置（用于判断是否渲染旧字源） */
  hasOldSource: boolean
  /** 新排版源 StyledString 是否已设置（用于判断是否渲染新字源） */
  hasNewSource: boolean
  /** 当前视觉帧 */
  visualFrame: EditorVisualFrame
  /** 裁切矩形列表（与 motionGlyphWindows 一一对应） */
  clipRects: ClipRect[]
  /** 正文排版宽度（vp）——Text 的 width，确保排版和主 Text 一致，不被 clipRect 约束 */
  textWidth: number
}

/**
 * 运动层文字遮罩 Builder。
 *
 * 在 ArkUI 组件树中渲染运动层——对真实 ArkUI Text 组件做 clipShape 裁切，
 * 实现吐字/吞字的像素级遮罩效果。
 *
 * Issue #879 本轮复核评论修复（问题2）：
 * - 用 Text(undefined, {controller}) 显示真实原始排版（通过 TextController.setStyledString 设置内容）。
 * - 旧、新两份 Text 的字体、行高、wordBreak(BREAK_ALL)、首行缩进、宽度、坐标原点
 *   必须分别与对应 revision 的主布局完全一致。
 * - clipShape 作用在绘制输出上，不能因 clip 容器缩窄而改变 Text 的排版宽度
 *   （Text 的 width 设为完整排版宽度 textWidth，Stack 外层做 clip）。
 * - 不能在每一帧重新 setStyledString——只在运动开始时设置一次（由 SujianEditor 负责）。
 *
 * 渲染策略：
 * - 吐字（Reveal）：新字源完整排版，clipShape 限制在 [x0, front] 裁切窗口
 * - 吞字（Conceal）：旧字源完整排版，clipShape 限制在 [x0, front] 裁切窗口
 * - Forward Delete：旧字源完整排版，clipShape 限制在 [caret, front] 裁切窗口
 * - Reflow（RetainedMove）：旧字源完整排版 + offset 平移到当前插值位置
 * - 非动画：不渲染任何内容（静态层负责）
 *
 * ArkTS 限制：@Component struct 不能在非页面 .ets 文件中定义，
 * 所以以 Builder 函数形式提供，由 SujianEditor 在 build() 中调用。
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
          // 根据 sourceKind 选择对应的 TextController
          // 吐字（Reveal）：sourceKind='new'，用 newTextController
          // 吞字（Conceal/ConcealForward）：sourceKind='old'，用 oldTextController
          // Reflow（RetainedMove）：sourceKind='old'，用 oldTextController + offset 平移
          if (window.sourceKind === 'new' && params.newTextController !== null && params.hasNewSource) {
            // 新字源：完整排版，用 Stack + offset + clipShape 限制可见区域
            // Text 的 width 设为 textWidth（完整排版宽度），不是 clipRect.width（裁切窗口宽度），
            // 确保排版和主 Text 一致，不会因 Stack 宽度限制而重新换行。
            // wordBreak(BREAK_ALL) 与主 Text 一致，确保折行行为相同。
            // clipShape 只裁切显示区域，不影响 Text 的排版宽度。
            // 通过 TextController.setStyledString 设置的 StyledString 包含字号/行高/首行缩进样式，
            // 不需要在 Text 属性上重复设置 fontSize/lineHeight。
            Stack() {
              Text(undefined, { controller: params.newTextController })
                .width(params.textWidth)
                .textAlign(TextAlign.Start)
                .wordBreak(WordBreak.BREAK_ALL)
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
          } else if (window.sourceKind === 'old' && params.oldTextController !== null && params.hasOldSource) {
            // 旧字源：完整排版，用 Stack + offset + clipShape 限制可见区域
            // Text 的 width 设为 textWidth（完整排版宽度），不是 clipRect.width（裁切窗口宽度），
            // 确保排版和主 Text 一致，不会因 Stack 宽度限制而重新换行。
            // wordBreak(BREAK_ALL) 与主 Text 一致，确保折行行为相同。
            // clipShape 只裁切显示区域，不影响 Text 的排版宽度。
            // RetainedMove 的平移通过 window.offsetX/offsetY 实现（叠加在 offset 上）。
            Stack() {
              Text(undefined, { controller: params.oldTextController })
                .width(params.textWidth)
                .textAlign(TextAlign.Start)
                .wordBreak(WordBreak.BREAK_ALL)
                .offset({
                  x: -params.clipRects[index].x + window.offsetX,
                  y: -params.clipRects[index].y + window.offsetY
                })
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
