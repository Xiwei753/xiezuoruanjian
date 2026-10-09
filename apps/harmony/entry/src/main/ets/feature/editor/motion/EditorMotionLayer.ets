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
// - 问题1：添加 computeStaticMaskPath/computeStaticMaskRects 方法，
//   从 EditorVisualFrame.staticVisibleRanges 和行布局生成静态可见裁切区域。
//   主 Text 仍完整排版，但其绘制输出按 staticVisibleRanges 用 clipShape 裁切——
//   只显示未被运动接管的范围。不再用背景色 Rect 遮罩。
// - 问题2：MotionTextOverlayParams 改为接收 motionSources（MotionSourceEntry[]），
//   不再接收 TextController 数组 / StyledString / runIdPrefix。
//   ForEach key 基于 window.runId（稳定身份），而非 `${sourceKind}-${runIdPrefix}-${index}`（自增事务编号）。
//   同一 runId 的 Text 不会因新事务而重建——只更新 clip 和位置。
//   TextController 和 StyledString 从 MotionSourceEntry 获取（通过 window.runId 匹配 source）。
// - 问题4：needsStaticMask 只有确切有 motion owner（motionGlyphWindows 中至少有一个 window
//   的 runId 非空）时才返回 true。非动画时不裁切。
//
// Issue #879 最新复核评论修复（问题3）：
// - 每个新 Text 的 onAppear 完成本节点自己的绑定（setStyledString），
//   不再检查 !item.mounted 跳过——避免同版本新窗口从源池读到 mounted=true 而漏字。
// - 用真实 ComponentObserver('layout') 替代 setTimeout 定时器充当绘制完成证据。
//   @Builder 函数中不能直接创建 ComponentObserver——通过 onWindowLayoutReady 回调
//   让父组件（SujianEditor）创建 observer，layout 回调中调用 markWindowLayoutReady。
// - onDisAppear 中调用 onWindowDisappear 回调清理本窗口状态。
// - 每个 Text 添加 .id() 基于 renderNodeKey（窗口身份），供父组件创建 ComponentObserver。
//
// ArkTS 限制：@Component struct 不能在非页面 .ets 文件中定义，
// 所以 MotionTextOverlay 以 Builder 函数形式提供，由 SujianEditor 在 build() 中调用。

import { EditorVisualFrame, MotionKind, MotionGlyphWindow, StaticVisibleRange } from './EditorVisualFrame'
import { CaretRect, LineLayout } from '../render/editor_render_geometry'
import { RectShape, PathShape } from '@ohos.arkui.shape'
import { EditorMotionCoordinator } from './EditorMotionCoordinator'
import { MotionSourceEntry } from './EditorMotionSourcePool'

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

    return EditorMotionCoordinator.subtractRanges(
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
   * Issue #879 复核评论修复（问题1/4）：
   * 动画中且 needsMotionLayer 时，主 Text 需要按 staticVisibleRanges 裁切，
   * 排除被运动接管的字形。非动画时不裁切（全显示）。
   *
   * Issue #879 复核评论修复（问题4）：
   * 只有确切有 motion owner（motionGlyphWindows 中至少有一个 window 的 runId 非空，
   * 即有对应的稳定源）时才返回 true。非动画时不裁切——staticClipCommands 默认应为
   * 全尺寸矩形（不裁切），而非空 path（完全隐藏）。
   */
  static needsStaticMask(frame: EditorVisualFrame): boolean {
    if (!EditorMotionLayer.needsMotionLayer(frame)) {
      return false
    }
    // 确保至少有一个 window 有有效的 runId（确切的 motion owner）
    for (const window of frame.motionGlyphWindows) {
      if (window.runId.length > 0) {
        return true
      }
    }
    return false
  }

  // ── 私有方法 ──
}

// ── ArkUI Builder 函数 ──

/**
 * MotionTextOverlayBuilder 的参数类型。
 * ArkTS 要求 @Builder 函数的参数类型必须是显式声明的 interface 或 class，
 * 不能用对象字面量类型。
 *
 * Issue #879 最新复核评论修复（问题3）：
 * - mounted/layoutReady 从窗口级状态读取（MotionWindowState），而非从源池 source 读取。
 *   避免某一窗口先 mounted 后，后续同版本新窗口从 source 读到 mounted=true，
 *   导致新 Text 的 onAppear 跳过 setStyledString 而空白。
 * - onWindowLayoutReady 回调替代旧的 onLayoutReady + setTimeout：
 *   @Builder 中不能直接创建 ComponentObserver，通过回调让父组件创建 observer，
 *   layout 回调中调用 markWindowLayoutReady(renderNodeKey)。
 * - onWindowDisappear 回调：Text.onDisAppear 中通知父组件清理本窗口状态。
 */
/** 预匹配的运动窗口+源+裁切矩形三元组——避免在 @Builder 中做匹配（ArkUI 限制） */
export interface MotionWindowSource {
  /** 运动字形窗口 */
  window: MotionGlyphWindow
  /** 匹配的稳定源（提供可共享的 StyledString） */
  source: MotionSourceEntry
  /** 裁切矩形 */
  clipRect: ClipRect
  /** 独立的 TextController——每个窗口有自己的 controller，不共享 */
  textController: TextController
  /** 是否已挂载（onAppear 已触发）——从窗口级状态读取，而非从源池 source 读取 */
  mounted: boolean
  /** 布局是否已完成——从窗口级状态读取，而非从源池 source 读取 */
  layoutReady: boolean
  /**
   * 窗口是否可见——Issue #879 复核评论修复（问题1）：
   * displayedVisualFrame 的窗口 visible=true（正常显示），
   * candidateVisualFrame 的窗口 visible=false（opacity 0，参与布局但不可见）。
   * 候选窗口 layoutReady 后原子提交时 visible 变为 true。
   */
  visible: boolean
  /**
   * 窗口布局就绪回调——Text.onAppear 中调用，通知父组件为本窗口创建 ComponentObserver。
   * 父组件创建 observer 后，在 layout 回调中调用 motionSourcePool.markWindowLayoutReady(renderNodeKey)。
   * 替代旧的 onLayoutReady + setTimeout——用真实 ComponentObserver('layout') 确认布局完成。
   */
  onWindowLayoutReady: ((renderNodeKey: string) => void) | null
  /**
   * 窗口消失回调——Text.onDisAppear 中调用，通知父组件清理本窗口状态。
   * 父组件调用 motionSourcePool.releaseWindowState(renderNodeKey) 并销毁对应的 ComponentObserver。
   */
  onWindowDisappear: ((renderNodeKey: string) => void) | null
}

export interface MotionTextOverlayParams {
  /** 预匹配的窗口-源-裁切矩形列表 */
  motionWindowSources: MotionWindowSource[]
  /** 正文排版宽度（vp）——Text 的 width，确保排版和主 Text 一致，不被 clipRect 约束 */
  textWidth: number
}

/**
 * 运动层文字遮罩 Builder。
 *
 * 在 ArkUI 组件树中渲染运动层——对真实 ArkUI Text 组件做 clipShape 裁切，
 * 实现吐字/吞字的像素级遮罩效果。
 *
 * Issue #879 最新复核评论修复（问题3）：
 * - 每个新 Text 的 onAppear 完成本节点自己的绑定（setStyledString），
 *   不再检查 !item.mounted 跳过——避免同版本新窗口从源池读到 mounted=true 而漏字。
 * - 用真实 ComponentObserver('layout') 替代 setTimeout 定时器充当绘制完成证据。
 *   @Builder 中不能直接创建 ComponentObserver——通过 onWindowLayoutReady 回调
 *   让父组件创建 observer，layout 回调中调用 markWindowLayoutReady。
 * - onDisAppear 中调用 onWindowDisappear 回调清理本窗口状态。
 * - 每个 Text 添加 .id() 基于 renderNodeKey（窗口身份），供父组件创建 ComponentObserver。
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
  if (params.motionWindowSources.length > 0) {
    Stack() {
      ForEach(
        params.motionWindowSources,
        (item: MotionWindowSource, index: number) => {
          Stack() {
            Text(undefined, { controller: item.textController })
              .width(params.textWidth)
              .textAlign(TextAlign.Start)
              .wordBreak(WordBreak.BREAK_ALL)
              // Issue #879 本轮复核评论修复（问题2）：
              // 每个 Text 添加 .id() 基于 renderNodeKey（窗口身份 windowId），
              // 供父组件创建 ComponentObserver。renderNodeKey 包含 sourceKind + sourceRevision，
              // 切源时创建新节点而非复用旧节点。
              .id(`motion-window-${item.window.renderNodeKey}`)
              .offset({
                x: -item.clipRect.x + item.window.offsetX,
                y: -item.clipRect.y + item.window.offsetY
              })
              .onAppear(() => {
                // Issue #879 最新复核评论修复（问题3）：
                // 每个新 Text 的 onAppear 完成本节点自己的绑定——
                // 不再检查 !item.mounted 跳过，避免同版本新窗口从源池读到 mounted=true 而漏字。
                // 每个 Text 都调用自己的 textController.setStyledString(source.styledString)。
                item.textController.setStyledString(item.source.styledString)
                // 通知父组件本窗口已挂载——父组件创建 ComponentObserver 监听 layout 事件。
                // ComponentObserver 'layout' 回调确保 ArkUI Text 的 measure/layout 真正完成后
                // 才调用 markWindowLayoutReady(renderNodeKey)，不再用 setTimeout 充当绘制完成证据。
                // Issue #879 本轮复核评论修复（问题2）：
                // 通知父组件本窗口已挂载——用 renderNodeKey 作为窗口身份。
                // renderNodeKey 包含 sourceKind + sourceRevision，切源时创建新窗口状态。
                if (item.onWindowLayoutReady !== null) {
                  item.onWindowLayoutReady(item.window.renderNodeKey)
                }
              })
              .onDisAppear(() => {
                // Issue #879 最新复核评论修复（问题3）：
                // Text 从组件树移除时清理本窗口状态——
                // 父组件调用 releaseWindowState(renderNodeKey) 并销毁对应的 ComponentObserver。
                // Issue #879 本轮复核评论修复（问题2）：
                // Text 从组件树移除时清理本窗口状态——用 renderNodeKey 作为窗口身份。
                if (item.onWindowDisappear !== null) {
                  item.onWindowDisappear(item.window.renderNodeKey)
                }
              })
          }
          .width(item.clipRect.width)
          .height(item.clipRect.height)
          .clipShape(new RectShape({
            width: item.clipRect.width,
            height: item.clipRect.height
          }))
          .position({
            x: item.clipRect.x,
            y: item.clipRect.y
          })
          // Issue #879 复核评论修复（问题1）：候选窗口 opacity 0——参与布局但不可见。
          // 候选窗口 layoutReady 后原子提交时 visible 变为 true，opacity 变为 1。
          .opacity(item.visible ? 1 : 0)
        },
        (item: MotionWindowSource, index: number) => `${item.window.renderNodeKey}`
      )
    }
    .width('100%')
    .height('100%')
    .alignContent(Alignment.TopStart)
  }
}
