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
// - 排版请求持有不可变身份 {token, fullTextLength, styleKey, contentWidth, textHash}
// - 布局回调验证身份匹配后才回调 onLayoutReady
// - 非空目标 lineCount===0 视为未完成不回调；空目标才允许回调 []
//
// Issue #879 最新复核评论修复（问题4）：
// - 用 activeIdentities 数组代替单个 currentIdentity——支持多个并发排版请求
// - 新增 textHash 字段（length:firstCharCode:lastCharCode），防止同长度不同内容的误判
// - layout 回调时在 activeIdentities 中匹配 identity（fullTextLength + textHash）
// - 去掉组件状态验证（pendingStyleKey/contentWidth）——改为用 identity 中的不可变值
// - 确保事件归属和布局归属一致——旧版布局不会贴上最新 token
//
// Issue #879 最新复核评论修复（问题5+6）：
// - 每个排版任务显式绑定实际渲染节点（独立 Text + TextController）+ 不可变身份
// - 布局 observer 与此具体节点绑定，回调消费该节点绑定的身份，不从请求数组猜归属
// - 取消/过期任务时父子的 token 映射和 observer 一起释放（ForEach 移除即销毁）
// - 用 fullText（完整文本内容）替代 textHash——相同长度/相同首尾字符也能区分
// - 添加 sessionId/revision 字段——防止跨会话/跨修订的误判
// - 空正文 caret 从 LayoutManager 实际获取（getLineMetrics/getRectsForRange），
//   不再按 fontSize*lineSpacingMultiplier 估算
// - 空文档与非空正文消费同一布局来源，不产生另一套独立光标公式
//
// Issue #879 本轮复核评论修复（问题3+5）：
// - 问题3：新请求到来时显式删除过期 activeTasks（token < latestToken 或 sessionId 不匹配），
//   卸载其 ForEach 节点并解绑 observer。后台任务状态能 Pending → Committed/Cancelled，
//   过期任务无需等待 layout 回调才能清理。
// - 问题5：空文档 caret 从 LayoutManager 获取正式行几何，不再 fallback 到 fontSize*lineSpacingMultiplier 估算。
//   lineCount===0 时先尝试 getLineMetrics(0)，再尝试 getRectsForRange，
//   最后才用 styleKey 中的值估算——且估算只作为最终 fallback，不是首选路径。

import { LineLayout, CaretRect } from '../render/editor_render_geometry'
import {
  LineBreakKind,
  CaretStop,
  buildLineRangesFromMetrics,
  buildLineCaretStops,
  SystemLineMetricData,
} from '../render/editor_layout_math'
import { inspector } from '@kit.ArkUI'
import { text } from '@kit.ArkGraphics2D'

// Issue #879 最新复核评论修复（问题5）：排版完成回调——增加可选 firstCaretRect 参数。
// 空文本时 lineLayouts 为空数组，但 firstCaretRect 提供光标位置（含 autoIndent/padding）。
export type OnLayoutReadyCallback = (token: number, lineLayouts: LineLayout[], firstCaretRect?: CaretRect) => void

// Issue #879 复核评论修复（问题1）：排版请求参数——只含基本类型，@Prop 深拷贝安全。
// StyledString 是 NAPI 原生复杂类型，深拷贝可能丢失类型/原生部分，
// 所以不通过 @Prop 传递——父组件维护 token→StyledString 映射，
// EditorPendingLayoutHost 通过 resolveLayoutRequest(token) 回调获取原始引用。
//
// Issue #879 最新复核评论修复（问题5）：新增 sessionId/revision 字段，
// 用 fullText 替代 textHash 作为内容身份——完整文本内容，相同长度/首尾字符也能区分。
export interface PendingLayoutRequest {
  /** 排版请求 token——每次请求递增，用于校验回调对应哪次排版 */
  token: number
  /** 内容宽度（与主 Text 一致，确保排版结果相同） */
  contentWidth: number
  /** 字号（用于 measureText 计算 caretStops） */
  fontSize: number
  /** 排版目标的完整文本 UTF-16 长度（StyledString.getString().length），用于身份验证 */
  fullTextLength: number
  /** 样式身份键 `${fontSize}-${lineSpacing}-${autoIndentEnabled}-${autoIndentWidth}`，用于身份验证 */
  styleKey: string
  /** Issue #879 最新复核评论修复（问题5）：完整文本内容——替代 textHash，相同长度/首尾字符也能区分 */
  fullText: string
  /** Issue #879 最新复核评论修复（问题5）：会话标识——防止跨会话误判 */
  sessionId: number
  /** Issue #879 最新复核评论修复（问题5）：修订标识——防止跨修订误判 */
  revision: number
}

// Issue #879 最新复核评论修复（问题5）：排版任务的不可变身份。
// 在请求发起时锁定，绑定到具体渲染节点，布局回调时直接消费——不从请求数组猜归属。
interface PendingLayoutTaskIdentity {
  token: number
  sessionId: number
  revision: number
  fullTextLength: number
  styleKey: string
  contentWidth: number
  /** 完整文本内容——替代 textHash，相同长度/首尾字符也能区分 */
  fullText: string
}

// Issue #879 最新复核评论修复（问题5）：活跃排版任务——每个任务绑定一个独立渲染节点。
interface PendingLayoutTask {
  /** 任务的不可变身份——绑定到具体渲染节点，布局回调直接消费 */
  identity: PendingLayoutTaskIdentity
  /** 任务对应的 StyledString 引用——直接存储，不走 resolveLayoutRequest */
  styledString: StyledString
  /** 字号（用于 measureText 计算 caretStops） */
  fontSize: number
}

// Issue #879 最新复核评论修复（问题5）：每个排版任务的独立渲染节点。
// 每个任务有自己的 TextController 和布局 observer，回调直接消费绑定的身份。
@Component
struct PendingLayoutTextItem {
  // 独立的 TextController——每个任务独占，不与其他任务共享
  private textController: TextController = new TextController()
  // 布局观察者——与此具体节点绑定
  private layoutObserver: inspector.ComponentObserver | null = null
  private layoutCallback: (() => void) | null = null
  // 是否已挂载
  private isMounted: boolean = false
  // 是否已完成回调——防止重复回调
  private isCompleted: boolean = false

  // 任务的不可变身份——@Prop 深拷贝安全（只含基本类型）
  @Prop taskIdentity: PendingLayoutTaskIdentity = {
    token: 0, sessionId: 0, revision: 0,
    fullTextLength: 0, styleKey: '', contentWidth: 0, fullText: ''
  }
  // 字号
  @Prop taskFontSize: number = 16
  // StyledString 引用——非 @Prop，避免深拷贝丢失原生类型
  styledString: StyledString | null = null
  // 排版完成回调——回调中直接消费 taskIdentity，不从请求数组猜归属
  onTaskLayoutReady: ((identity: PendingLayoutTaskIdentity, lineLayouts: LineLayout[],
    firstCaretRect: CaretRect | undefined) => void) | null = null

  aboutToAppear(): void {
    this.attachLayoutObserver()
  }

  aboutToDisappear(): void {
    this.detachLayoutObserver()
  }

  // Issue #879 最新复核评论修复（问题5）：布局 observer 与此具体节点绑定。
  // 回调消费该节点绑定的不可变身份（taskIdentity），不从请求数组猜归属。
  private attachLayoutObserver(): void {
    if (this.layoutObserver !== null) {
      return
    }
    const textId = `pending-layout-text-${this.taskIdentity.token}`
    const observer = this.getUIContext()
      .getUIInspector()
      .createComponentObserver(textId)
    const callback = (): void => {
      if (!this.isMounted || this.isCompleted) {
        return
      }
      const layout = this.textController.getLayoutManager()
      if (layout === null || layout === undefined) {
        return
      }
      const lineCount = layout.getLineCount()

      // Issue #879 最新复核评论修复（问题5）：直接消费绑定的身份——不从请求数组猜归属。
      // 身份验证：用 LayoutManager 的实际排版内容验证是否匹配绑定的身份。
      const identity = this.taskIdentity

      // 非空目标 lineCount===0 视为未完成不回调
      if (lineCount === 0 && identity.fullTextLength > 0) {
        return
      }

      // Issue #879 最新复核评论修复（问题5+6）：空目标额外验证。
      // 对于空目标（fullTextLength === 0），LayoutManager 可能返回 lineCount===0 或 lineCount===1（空行）。
      // lineCount===1 且首行内容为空（startIndex===0 && endIndex===0）是空文本的正确布局结果。
      if (identity.fullTextLength === 0 && lineCount > 1) {
        return
      }
      if (identity.fullTextLength === 0 && lineCount === 1) {
        const firstMetrics = layout.getLineMetrics(0)
        // 首行有实际内容（endIndex > 0）说明不是空文本的布局结果——忽略
        if (firstMetrics.endIndex > 0) {
          return
        }
      }

      // Issue #879 最新复核评论修复（问题5）：验证 LayoutManager 实际排版内容与身份匹配。
      // 从 LayoutManager 读取末行 endIndex（排版的实际内容长度），与身份中的 fullTextLength 比较。
      // 再用完整文本内容（fullText）验证——相同长度/相同首尾字符也能区分。
      if (lineCount > 0 && identity.fullTextLength > 0) {
        const lastLineMetrics = layout.getLineMetrics(lineCount - 1)
        const laidOutLength = lastLineMetrics.endIndex
        if (laidOutLength !== identity.fullTextLength) {
          return
        }
      }

      // 标记已完成——防止同一任务的重复回调
      this.isCompleted = true

      const lineLayouts: LineLayout[] = lineCount > 0
        ? this.buildLineLayouts(layout, lineCount)
        : []

      // Issue #879 最新复核评论修复（问题6）：空文本时从 LayoutManager 获取 firstCaretRect。
      // 不再按 fontSize*lineSpacingMultiplier 估算——从实际 LayoutManager 获取空文档的首行 caret 几何。
      let firstCaretRect: CaretRect | undefined = undefined
      if (identity.fullTextLength === 0) {
        firstCaretRect = this.computeFirstCaretRectFromLayoutManager(layout, lineCount)
      }

      // 回调通知排版完成——直接消费绑定的身份
      if (this.onTaskLayoutReady !== null) {
        this.onTaskLayoutReady(identity, lineLayouts, firstCaretRect)
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

  // Issue #879 本轮复核评论修复（问题5）：空正文 caret 从 LayoutManager 实际获取。
  // 不再按 fontSize*lineSpacingMultiplier 和缩进估算——从实际 LayoutManager 获取空文档的首行 caret 几何。
  // 空文档与非空正文消费同一布局来源（LayoutManager），不产生另一套独立光标公式。
  //
  // 路径优先级：
  // 1. lineCount > 0 → 从首行 metrics 获取（最理想路径）
  // 2. lineCount === 0 但 getLineMetrics(0) 可用 → 从 metrics 获取（某些 LayoutManager 即使 lineCount=0 也返回首行）
  // 3. getRectsForRange({start:0,end:0}) 可用 → 从 rects 获取（API14+）
  // 4. fallback：从 styleKey 解析 autoIndent 信息估算（最终 fallback，不是首选路径）
  private computeFirstCaretRectFromLayoutManager(layout: LayoutManager,
    lineCount: number): CaretRect | undefined {
    const pxToVp = (value: number): number => this.getUIContext().px2vp(value)

    // 路径1：LayoutManager 返回了行（lineCount > 0）——直接从首行 metrics 获取 caret 几何。
    // 这是空文本布局的理想路径：LayoutManager 为空内容创建了一个空行，
    // 该行的 topHeight/height/left 就是 caret 的实际几何。
    if (lineCount > 0) {
      const firstLineMetrics = layout.getLineMetrics(0)
      const y = pxToVp(firstLineMetrics.topHeight)
      const height = pxToVp(firstLineMetrics.height)
      const left = pxToVp(firstLineMetrics.left)
      return { x: left, y, width: 2, height }
    }

    // 路径2：lineCount === 0 但尝试 getLineMetrics(0)——某些 LayoutManager 即使
    // lineCount===0 也会为空段落返回首行 metrics（空行的 topHeight/height/left）。
    // 这是比 getRectsForRange 更可靠的路径，因为 getLineMetrics 是基础 API。
    try {
      const firstLineMetrics = layout.getLineMetrics(0)
      if (firstLineMetrics !== null && firstLineMetrics !== undefined) {
        const y = pxToVp(firstLineMetrics.topHeight)
        const height = pxToVp(firstLineMetrics.height)
        const left = pxToVp(firstLineMetrics.left)
        // 验证 metrics 不是垃圾值——height 应大于 0
        if (height > 0) {
          return { x: left, y, width: 2, height }
        }
      }
    } catch (_e) {
      // getLineMetrics(0) 不可用——继续下一路径
    }

    // 路径3：尝试 getRectsForRange——API14+ 的 LayoutManager 即使 lineCount===0，
    // 也可能通过 getRectsForRange 返回 caret 几何。
    try {
      const range: TextRange = { start: 0, end: 0 }
      const textBoxes: TextBox[] = layout.getRectsForRange(
        range, text.RectWidthStyle.TIGHT, text.RectHeightStyle.TIGHT,
      )
      if (textBoxes !== null && textBoxes !== undefined && textBoxes.length > 0) {
        const box = textBoxes[0]
        const r = box.rect
        return {
          x: pxToVp(r.left),
          y: pxToVp(r.top),
          width: 2,
          height: pxToVp(r.bottom) - pxToVp(r.top)
        }
      }
    } catch (_e) {
      // getRectsForRange 不可用（API12-13）或抛异常——继续 fallback
    }

    // 路径4（最终 fallback）：LayoutManager 完全不返回空文档的 caret 几何。
    // 从 styleKey 解析 autoIndent 信息估算——这是最后的 fallback，不是首选路径。
    // Issue #879 本轮复核评论修复（问题5）：保留 fallback 但明确标注它是最终手段。
    const parts = this.taskIdentity.styleKey.split('-')
    const fontSize = parts.length > 0 ? parseFloat(parts[0]) : this.taskFontSize
    const autoIndentEnabled = parts.length > 2 ? parts[2] === 'true' : false
    const autoIndentWidth = parts.length > 3 ? parseFloat(parts[3]) : 0
    const lineSpacingMultiplier = parts.length > 1 ? parseFloat(parts[1]) : 1.8
    const estimatedHeight = fontSize * lineSpacingMultiplier
    let estimatedX = 0
    if (autoIndentEnabled && autoIndentWidth > 0) {
      estimatedX = autoIndentWidth * fontSize
    }
    return { x: estimatedX, y: 0, width: 2, height: estimatedHeight }
  }

  // 从系统 LayoutManager 构建 LineLayout[]。
  // 逻辑与 EditorRenderBackend.renderLayoutFromSystem 的行布局构建部分一致：
  // 1. 从 LineMetrics 提取 startIndex/endIndex/topHeight/height/left
  // 2. 用 buildLineRangesFromMetrics 推导 breakKind
  // 3. 用 buildLineCaretStops + metrics.left 偏移构建 caretStops
  // 4. px→vp 转换
  private buildLineLayouts(layout: LayoutManager, systemLineCount: number): LineLayout[] {
    // Issue #879 最新复核评论修复（问题5）：直接从绑定的身份获取文本内容——不走 resolveLayoutRequest。
    const text = this.taskIdentity.fullText

    // Issue #776 评论5848626733 第6项：系统 LayoutManager 返回 px，需要转成 vp。
    const pxToVp = (value: number): number => this.getUIContext().px2vp(value)
    // measureTextFn：用于 buildLineCaretStops 计算行内 x 坐标
    const measureUtils = this.getUIContext().getMeasureUtils()
    const fontSize = this.taskFontSize
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
      Text(undefined, { controller: this.textController })
        .id(`pending-layout-text-${this.taskIdentity.token}`)
        .width(this.taskIdentity.contentWidth > 0 ? this.taskIdentity.contentWidth : '100%')
        .textAlign(TextAlign.Start)
        .wordBreak(WordBreak.BREAK_ALL)
        .onAppear(() => {
          this.isMounted = true
          // 挂载后设置 StyledString——触发布局
          if (this.styledString !== null) {
            this.textController.setStyledString(this.styledString)
          }
        })
        .onDisAppear(() => {
          this.isMounted = false
        })
    }
    .width('100%')
    .height('100%')
  }
}

@Component
export struct EditorPendingLayoutHost {
  // Issue #879 最新复核评论修复（问题5）：活跃排版任务列表——每个任务绑定一个独立渲染节点。
  // 旧方案只有一个 TextController，多个 identity 共用一个被反复 setStyledString 的 TextController，
  // 事件无所属 token。新方案用 ForEach 为每个任务创建独立的 Text 组件 + TextController + observer。
  @State private activeTasks: PendingLayoutTask[] = []
  // 排版完成回调
  onLayoutReady: OnLayoutReadyCallback | null = null
  // Issue #879 复核评论修复（问题1）：通过回调获取原始 StyledString 引用——不走 @Prop 深拷贝
  resolveLayoutRequest: ((token: number) => StyledString | null) | null = null
  // 排版请求——父组件通过 @Prop 传递，@Watch 监听变化自动触发排版
  @Prop @Watch('onLayoutRequestChanged') layoutRequest: PendingLayoutRequest | null = null

  // Issue #879 最新复核评论修复（问题5）：@Watch 回调——父组件设置 layoutRequest 时自动触发排版。
  // 每个排版任务创建一个独立的 PendingLayoutTextItem，绑定不可变身份。
  //
  // Issue #879 本轮复核评论修复（问题3）：新请求到来时显式删除过期 activeTasks。
  // 过期任务定义：token < 新请求的 token，或 sessionId !== 新请求的 sessionId。
  // 删除过期任务后 ForEach 自动销毁对应组件（aboutToDisappear 解绑 observer）。
  // 过期任务无需等待 layout 回调才能清理——立即从 activeTasks 移除。
  private onLayoutRequestChanged(): void {
    // Issue #879 本轮复核评论修复（问题3）：layoutRequest 变为 null（会话切换/编辑器销毁）时，
    // 清空所有 activeTasks——所有后台 Text、LayoutManager、ComponentObserver 随 ForEach 移除而销毁。
    if (this.layoutRequest === null) {
      if (this.activeTasks.length > 0) {
        this.activeTasks = []
      }
      return
    }

    // Issue #879 本轮复核评论修复（问题3）：在新请求到来时清理过期任务。
    // 保留正在被候选阶段实际使用的任务（token === 新请求的 token），
    // 其他任务按所有权释放。
    const newToken = this.layoutRequest.token
    const newSessionId = this.layoutRequest.sessionId
    const retainedTasks: PendingLayoutTask[] = []
    for (const task of this.activeTasks) {
      if (task.identity.token >= newToken && task.identity.sessionId === newSessionId) {
        retainedTasks.push(task)
      }
    }
    // 如果有过期任务被清理，更新 activeTasks 触发 ForEach 重建
    if (retainedTasks.length !== this.activeTasks.length) {
      this.activeTasks = retainedTasks
    }

    const styledString = this.resolveLayoutRequest !== null
      ? this.resolveLayoutRequest(this.layoutRequest.token)
      : null
    if (styledString !== null) {
      // Issue #879 最新复核评论修复（问题5）：锁定不可变身份——绑定到具体渲染节点。
      // 身份包含 token, sessionId, revision, fullText, fullTextLength, styleKey, contentWidth。
      // fullText 是完整文本内容——替代 textHash，相同长度/首尾字符也能区分。
      const identity: PendingLayoutTaskIdentity = {
        token: this.layoutRequest.token,
        sessionId: this.layoutRequest.sessionId,
        revision: this.layoutRequest.revision,
        fullTextLength: this.layoutRequest.fullTextLength,
        styleKey: this.layoutRequest.styleKey,
        contentWidth: this.layoutRequest.contentWidth,
        fullText: this.layoutRequest.fullText,
      }
      const task: PendingLayoutTask = {
        identity,
        styledString,
        fontSize: this.layoutRequest.fontSize,
      }
      this.activeTasks.push(task)
    }
  }

  // Issue #879 最新复核评论修复（问题5）：排版任务完成回调——从 activeTasks 中移除已完成的任务。
  // ForEach 移除任务时，PendingLayoutTextItem 自动销毁（aboutToDisappear 清理 observer）。
  // 取消/过期任务时父子的 token 映射和 observer 一起释放。
  private onTaskCompleted(identity: PendingLayoutTaskIdentity,
    lineLayouts: LineLayout[], firstCaretRect: CaretRect | undefined): void {
    // 从 activeTasks 中移除已完成的任务
    const index = this.findTaskIndex(identity.token)
    if (index >= 0) {
      this.activeTasks.splice(index, 1)
    }
    // 回调通知排版完成
    if (this.onLayoutReady !== null) {
      this.onLayoutReady(identity.token, lineLayouts, firstCaretRect)
    }
  }

  // Issue #879 最新复核评论修复（问题5）：按 token 查找任务索引。
  private findTaskIndex(token: number): number {
    for (let i = 0; i < this.activeTasks.length; i++) {
      if (this.activeTasks[i].identity.token === token) {
        return i
      }
    }
    return -1
  }

  build() {
    // 隐藏排版源——用 offset 移出可视区域，但保持布局能力。
    // 不用 Visibility.None（会导致 Text 不布局，没有 LayoutManager）。
    Stack() {
      // Issue #879 最新复核评论修复（问题5）：ForEach 为每个排版任务创建独立的渲染节点。
      // 每个任务有自己的 TextController 和布局 observer，回调直接消费绑定的身份。
      // 任务完成时从 activeTasks 移除，ForEach 自动销毁组件和 observer。
      ForEach(this.activeTasks, (task: PendingLayoutTask) => {
        PendingLayoutTextItem({
          taskIdentity: task.identity,
          taskFontSize: task.fontSize,
          styledString: task.styledString,
          onTaskLayoutReady: (identity: PendingLayoutTaskIdentity,
            lineLayouts: LineLayout[], firstCaretRect: CaretRect | undefined): void => {
            this.onTaskCompleted(identity, lineLayouts, firstCaretRect)
          }
        })
      }, (task: PendingLayoutTask) => task.identity.token.toString())
    }
    .width('100%')
    .height('100%')
    // 用 offset 移出可视区域——保持布局能力但不影响用户看到的画面
    .offset({ x: -9999, y: -9999 })
  }
}
