// EditorPendingLayoutHost — 独立隐藏排版源。
// 新版本 StyledString 只发给这个隐藏排版源，不影响可见主 Text。
// 布局完成后回调 token + LineLayout[]，供 SujianEditor 在同帧发布 RenderPlan。
//
// Issue #879：写作区文字运动层。
// 主 Text 在 layout 期间先完整显示新内容再被裁切会产生闪烁。
// 本组件在隐藏 Text 中排版新 StyledString，布局完成后回调行布局结果，
// 让 SujianEditor 在同一帧发布新静态可见区 + 运动源窗口 + 光标。

import { LineLayout } from '../render/editor_render_geometry'
import {
  LineBreakKind,
  CaretStop,
  buildLineRangesFromMetrics,
  buildLineCaretStops,
  SystemLineMetricData,
} from '../render/editor_layout_math'
import { inspector } from '@kit.ArkUI'

const PENDING_HOST_TEXT_ID = 'editor-pending-layout-text'

// 排版完成回调——token 是排版版本号，lineLayouts 是系统布局结果
export type OnLayoutReadyCallback = (token: number, lineLayouts: LineLayout[]) => void

// Issue #879 复核评论修复（问题1）：排版请求参数——父组件通过 @Prop 传递，
// EditorPendingLayoutHost 用 @Watch 监听变化自动触发排版。
// ArkUI 中 @Component struct 的方法不能从父组件直接调用，
// 所以用 @Prop + @Watch 模式实现父子通信。
export interface PendingLayoutRequest {
  /** 要排版的 StyledString */
  styledString: StyledString
  /** 内容宽度（与主 Text 一致，确保排版结果相同） */
  contentWidth: number
  /** 字号（用于 measureText 计算 caretStops） */
  fontSize: number
  /** 排版请求 token——每次请求递增，用于校验回调对应哪次排版 */
  token: number
}

@Component
export struct EditorPendingLayoutHost {
  // 独立的 TextController——不与主 Text 共享
  private pendingTextController: TextController = new TextController()
  // 排版版本号——每次 requestLayout 递增，用于校验回调对应哪次排版请求
  private layoutToken: number = 0
  // 当前排版的 StyledString
  @State private pendingStyledString: StyledString | null = null
  // 布局观察者
  private layoutObserver: inspector.ComponentObserver | null = null
  private layoutCallback: (() => void) | null = null
  // 是否已挂载
  private isMounted: boolean = false
  // 排版完成回调
  onLayoutReady: OnLayoutReadyCallback | null = null
  // 内容宽度（与主 Text 一致，确保排版结果相同）
  @State private contentWidth: number = 0
  // 字号（用于 measureText 计算 caretStops）
  private pendingFontSize: number = 16
  // Issue #879 复核评论修复（问题1）：排版请求——父组件通过 @Prop 传递，
  // @Watch 监听变化自动触发 requestLayout。
  @Prop @Watch('onLayoutRequestChanged') layoutRequest: PendingLayoutRequest | null = null

  aboutToAppear(): void {
    this.attachLayoutObserver()
  }

  aboutToDisappear(): void {
    this.detachLayoutObserver()
  }

  // Issue #879 复核评论修复（问题1）：@Watch 回调——父组件设置 layoutRequest 时自动触发排版。
  // ArkUI 中 @Component struct 的方法不能从父组件直接调用，
  // 所以用 @Prop + @Watch 模式实现父子通信。
  private onLayoutRequestChanged(): void {
    if (this.layoutRequest !== null) {
      this.requestLayout(
        this.layoutRequest.styledString,
        this.layoutRequest.contentWidth,
        this.layoutRequest.fontSize
      )
    }
  }

  // 请求排版新版本——递增 token，设置 StyledString
  // fontSize 用于 measureText 计算 caretStops 的行内 x 坐标
  requestLayout(styledString: StyledString, contentWidth: number, fontSize: number): number {
    this.layoutToken++
    this.contentWidth = contentWidth
    this.pendingFontSize = fontSize
    this.pendingStyledString = styledString
    // 在 Text 挂载后通过 controller 设置 StyledString
    if (this.isMounted) {
      this.pendingTextController.setStyledString(styledString)
    }
    return this.layoutToken
  }

  private attachLayoutObserver(): void {
    if (this.layoutObserver !== null) {
      return
    }
    const observer = this.getUIContext()
      .getUIInspector()
      .createComponentObserver(PENDING_HOST_TEXT_ID)
    const callback = (): void => {
      if (!this.isMounted || this.pendingStyledString === null) {
        return
      }
      // 从 LayoutManager 读取行布局
      const layout = this.pendingTextController.getLayoutManager()
      if (layout === null || layout === undefined) {
        return
      }
      const lineCount = layout.getLineCount()
      if (lineCount === 0) {
        return
      }
      // 构建 LineLayout[] — 与 EditorRenderBackend.renderLayoutFromSystem 类似，
      // 但只提取行布局信息（startUtf16, endUtf16, y, height, left, breakKind, caretStops），
      // 不做选区/光标/composition 计算。
      const lineLayouts = this.buildLineLayouts(layout, lineCount)
      if (lineLayouts.length === 0) {
        return
      }
      // 回调通知排版完成
      if (this.onLayoutReady !== null) {
        this.onLayoutReady(this.layoutToken, lineLayouts)
      }
    }
    observer.on('layout', callback)
    this.layoutObserver = observer
    this.layoutCallback = callback
  }

  private detachLayoutObserver(): void {
    if (this.layoutObserver !== null && this.layoutCallback !== null) {
      this.layoutObserver.off('layout', this.layoutCallback)
    }
    this.layoutObserver = null
    this.layoutCallback = null
  }

  // 从系统 LayoutManager 构建 LineLayout[]。
  // 逻辑与 EditorRenderBackend.renderLayoutFromSystem 的行布局构建部分一致：
  // 1. 从 LineMetrics 提取 startIndex/endIndex/topHeight/height/left
  // 2. 用 buildLineRangesFromMetrics 推导 breakKind
  // 3. 用 buildLineCaretStops + metrics.left 偏移构建 caretStops
  // 4. px→vp 转换
  private buildLineLayouts(layout: LayoutManager, systemLineCount: number): LineLayout[] {
    const text = this.pendingStyledString?.getString() ?? ''
    if (text.length === 0) {
      return []
    }

    // Issue #776 评论5848626733 第6项：系统 LayoutManager 返回 px，需要转成 vp。
    const pxToVp = (value: number): number => this.getUIContext().px2vp(value)
    // measureTextFn：用于 buildLineCaretStops 计算行内 x 坐标
    // Issue #776 评论5848626733 第6项：measureUtils.measureText 返回 px，用 px2vp 转成 vp。
    const measureUtils = this.getUIContext().getMeasureUtils()
    const fontSize = this.pendingFontSize
    const measureTextFn = (s: string): number => {
      return pxToVp(measureUtils.measureText({ textContent: s, fontSize }))
    }

    // 1. 从 LineMetrics 构建 LineLayout[]（breakKind 占位）和 SystemLineMetricData[]
    const systemLines: LineLayout[] = []
    const lineMetricsData: SystemLineMetricData[] = []
    for (let i = 0; i < systemLineCount; i++) {
      const metrics = layout.getLineMetrics(i)
      const startUtf16 = metrics.startIndex
      const endUtf16 = metrics.endIndex
      lineMetricsData.push({
        startIndex: startUtf16,
        endIndex: endUtf16,
        isLastLine: i === systemLineCount - 1,
        text,
      })
      // y 坐标：topHeight 是从段落顶部到当前行的距离，用作行顶部 y
      const y = pxToVp(metrics.topHeight)
      const height = pxToVp(metrics.height)
      const left = pxToVp(metrics.left)

      // 用 buildLineCaretStops 生成行内 caret stops，再用 metrics.left 偏移每个 stop 的 x
      const rawStops = buildLineCaretStops(
        text,
        { start: startUtf16, end: endUtf16 },
        measureTextFn,
      )
      const caretStops: CaretStop[] = rawStops.map((stop: CaretStop): CaretStop => ({
        utf16Offset: stop.utf16Offset,
        x: left + stop.x,
      }))

      systemLines.push({
        startUtf16,
        endUtf16,
        left,
        y,
        height,
        breakKind: LineBreakKind.EndOfText, // 占位，下面用 buildLineRangesFromMetrics 覆盖
        caretStops,
      })
    }

    // 2. 用 buildLineRangesFromMetrics 推导 breakKind，更新 systemLines
    const systemLineRanges = buildLineRangesFromMetrics(lineMetricsData)
    for (let i = 0; i < systemLines.length; i++) {
      systemLines[i] = {
        startUtf16: systemLines[i].startUtf16,
        endUtf16: systemLines[i].endUtf16,
        left: systemLines[i].left,
        y: systemLines[i].y,
        height: systemLines[i].height,
        caretStops: systemLines[i].caretStops,
        breakKind: systemLineRanges[i].breakKind,
      }
    }

    return systemLines
  }

  build() {
    // 隐藏排版源——用 offset 移出可视区域，但保持布局能力。
    // 不用 Visibility.None（会导致 Text 不布局，没有 LayoutManager）。
    Stack() {
      Text(undefined, { controller: this.pendingTextController })
        .id(PENDING_HOST_TEXT_ID)
        .width(this.contentWidth > 0 ? this.contentWidth : '100%')
        .textAlign(TextAlign.Start)
        .wordBreak(WordBreak.BREAK_ALL)
        .onAppear(() => {
          this.isMounted = true
          if (this.pendingStyledString !== null) {
            this.pendingTextController.setStyledString(this.pendingStyledString)
          }
        })
        .onDisAppear(() => {
          this.isMounted = false
        })
    }
    .width('100%')
    .height('100%')
    // 用 offset 移出可视区域——保持布局能力但不影响用户看到的画面
    .offset({ x: -9999, y: -9999 })
  }
}
