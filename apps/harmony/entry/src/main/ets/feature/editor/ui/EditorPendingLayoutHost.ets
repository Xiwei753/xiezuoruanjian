// EditorPendingLayoutHost — 独立隐藏排版源。
// 新版本 StyledString 只发给这个隐藏排版源，不影响可见主 Text。
// 布局完成后回调 token + LineLayout[]，供 SujianEditor 在同帧发布 RenderPlan。
//
// Issue #879：写作区文字运动层。
// 主 Text 在 layout 期间先完整显示新内容再被裁切会产生闪烁。
// 本组件在隐藏 Text 中排版新 StyledString，布局完成后回调行布局结果，
// 让 SujianEditor 在同一帧发布新静态可见区 + 运动源窗口 + 光标。
//
// Issue #879 复核评论修复（问题1+2）：
// - PendingLayoutRequest 只含基本类型，@Prop 深拷贝安全
// - StyledString 通过 resolveLayoutRequest(token) 回调获取，不走 @Prop
// - 排版请求持有不可变身份 {token, fullTextLength, styleKey, contentWidth}
// - 布局回调验证身份匹配后才回调 onLayoutReady
// - 非空目标 lineCount===0 视为未完成不回调；空目标才允许回调 []

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

// Issue #879 复核评论修复（问题1）：排版请求参数——只含基本类型，@Prop 深拷贝安全。
// StyledString 是 NAPI 原生复杂类型，深拷贝可能丢失类型/原生部分，
// 所以不通过 @Prop 传递——父组件维护 token→StyledString 映射，
// EditorPendingLayoutHost 通过 resolveLayoutRequest(token) 回调获取原始引用。
export interface PendingLayoutRequest {
  /** 排版请求 token——每次请求递增，用于校验回调对应哪次排版 */
  token: number
  /** 内容宽度（与主 Text 一致，确保排版结果相同） */
  contentWidth: number
  /** 字号（用于 measureText 计算 caretStops） */
  fontSize: number
  /** 排版目标的完整文本 UTF-16 长度（StyledString.getString().length），用于身份验证 */
  fullTextLength: number
  /** 样式身份键 `${fontSize}-${lineSpacing}`，用于身份验证 */
  styleKey: string
}

// 排版请求的不可变身份——在请求发起时锁定，布局回调时验证匹配
interface LayoutRequestIdentity {
  token: number
  fullTextLength: number
  styleKey: string
  contentWidth: number
}

@Component
export struct EditorPendingLayoutHost {
  // 独立的 TextController——不与主 Text 共享
  private pendingTextController: TextController = new TextController()
  // 当前排版的请求身份——布局回调时验证匹配
  private currentIdentity: LayoutRequestIdentity | null = null
  // 布局观察者
  private layoutObserver: inspector.ComponentObserver | null = null
  private layoutCallback: (() => void) | null = null
  // 是否已挂载
  private isMounted: boolean = false
  // 排版完成回调
  onLayoutReady: OnLayoutReadyCallback | null = null
  // Issue #879 复核评论修复（问题1）：通过回调获取原始 StyledString 引用——不走 @Prop 深拷贝
  resolveLayoutRequest: ((token: number) => StyledString | null) | null = null
  // 内容宽度（与主 Text 一致，确保排版结果相同）
  @State private contentWidth: number = 0
  // 字号（用于 measureText 计算 caretStops）
  private pendingFontSize: number = 16
  // 排版请求——父组件通过 @Prop 传递，@Watch 监听变化自动触发排版
  @Prop @Watch('onLayoutRequestChanged') layoutRequest: PendingLayoutRequest | null = null

  aboutToAppear(): void {
    this.attachLayoutObserver()
  }

  aboutToDisappear(): void {
    this.detachLayoutObserver()
  }

  // Issue #879 复核评论修复（问题1）：@Watch 回调——父组件设置 layoutRequest 时自动触发排版。
  // 通过 resolveLayoutRequest(token) 回调获取原始 StyledString 引用，不走 @Prop 深拷贝。
  private onLayoutRequestChanged(): void {
    if (this.layoutRequest !== null) {
      const styledString = this.resolveLayoutRequest !== null
        ? this.resolveLayoutRequest(this.layoutRequest.token)
        : null
      if (styledString !== null) {
        // 锁定不可变身份——布局回调时验证匹配
        this.currentIdentity = {
          token: this.layoutRequest.token,
          fullTextLength: this.layoutRequest.fullTextLength,
          styleKey: this.layoutRequest.styleKey,
          contentWidth: this.layoutRequest.contentWidth,
        }
        this.requestLayout(
          styledString,
          this.layoutRequest.contentWidth,
          this.layoutRequest.fontSize,
          this.layoutRequest.token
        )
      }
    }
  }

  // 请求排版新版本——使用父组件传入的 token，不自增
  requestLayout(styledString: StyledString, contentWidth: number, fontSize: number, token: number): number {
    this.contentWidth = contentWidth
    this.pendingFontSize = fontSize
    // 在 Text 挂载后通过 controller 设置 StyledString
    if (this.isMounted) {
      this.pendingTextController.setStyledString(styledString)
    }
    return token
  }

  private attachLayoutObserver(): void {
    if (this.layoutObserver !== null) {
      return
    }
    const observer = this.getUIContext()
      .getUIInspector()
      .createComponentObserver(PENDING_HOST_TEXT_ID)
    const callback = (): void => {
      if (!this.isMounted || this.currentIdentity === null) {
        return
      }
      // 从 LayoutManager 读取行布局
      const layout = this.pendingTextController.getLayoutManager()
      if (layout === null || layout === undefined) {
        return
      }
      const lineCount = layout.getLineCount()

      // Issue #879 复核评论修复（问题2）：布局回调身份验证。
      // 从 LayoutManager 读取最后一行的 endIndex，推算排版内容的总 UTF-16 长度，
      // 与请求的 fullTextLength 比较——如果不匹配，说明排版的还是旧内容，不回调。
      if (lineCount > 0) {
        const lastLineMetrics = layout.getLineMetrics(lineCount - 1)
        const laidOutLength = lastLineMetrics.endIndex
        if (laidOutLength !== this.currentIdentity.fullTextLength) {
          // 排版内容长度与请求不匹配——排版还是旧内容，忽略此次回调
          return
        }
      }

      // Issue #879 复核评论修复（问题2）：非空目标 lineCount===0 视为未完成不回调。
      // 对于非空目标（fullTextLength > 0），lineCount===0 只能是还没完成排版，
      // 不能回调 []——否则会把上一笔排版当成当前笔。
      // 对于明确空目标（fullTextLength === 0），才允许完成空结果回调 []。
      if (lineCount === 0 && this.currentIdentity.fullTextLength > 0) {
        // 非空目标但 lineCount===0——排版尚未完成，不回调
        return
      }

      const lineLayouts: LineLayout[] = lineCount > 0
        ? this.buildLineLayouts(layout, lineCount)
        : []
      // 回调通知排版完成——身份验证通过后才回调
      if (this.onLayoutReady !== null) {
        this.onLayoutReady(this.currentIdentity.token, lineLayouts)
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
    // Issue #879 复核评论修复（问题1）：通过 resolveLayoutRequest 获取 StyledString 的文本内容。
    // 不再依赖 @State pendingStyledString——改为从 currentIdentity.token 回查。
    const styledString = this.currentIdentity !== null && this.resolveLayoutRequest !== null
      ? this.resolveLayoutRequest(this.currentIdentity.token)
      : null
    const text = styledString?.getString() ?? ''
    // 空正文不在这里返回 []——空正文是合法排版结果，由调用方处理。
    // 这里只在 systemLineCount > 0 时构建行布局。

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
          // Issue #879 复核评论修复（问题1）：挂载后如果有待排版请求，通过回调获取 StyledString。
          if (this.currentIdentity !== null && this.resolveLayoutRequest !== null) {
            const styledString = this.resolveLayoutRequest(this.currentIdentity.token)
            if (styledString !== null) {
              this.pendingTextController.setStyledString(styledString)
            }
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
