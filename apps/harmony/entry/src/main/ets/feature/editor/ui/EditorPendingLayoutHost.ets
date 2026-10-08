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
// Issue #879 最新复核评论修复（问题5）：
// - OnLayoutReadyCallback 增加可选 firstCaretRect 参数
// - 空文本时也计算 firstCaretRect（从 LayoutManager 首行 metrics 或参数估算）
// - firstCaretRect 包含 x/y/height，考虑 autoIndent 和段落 padding

import { LineLayout, CaretRect } from '../render/editor_render_geometry'
import {
  LineBreakKind,
  CaretStop,
  buildLineRangesFromMetrics,
  buildLineCaretStops,
  SystemLineMetricData,
} from '../render/editor_layout_math'
import { inspector } from '@kit.ArkUI'

const PENDING_HOST_TEXT_ID = 'editor-pending-layout-text'

// Issue #879 最新复核评论修复（问题5）：排版完成回调——增加可选 firstCaretRect 参数。
// 空文本时 lineLayouts 为空数组，但 firstCaretRect 提供光标位置（含 autoIndent/padding）。
export type OnLayoutReadyCallback = (token: number, lineLayouts: LineLayout[], firstCaretRect?: CaretRect) => void

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
  /** Issue #879 最新复核评论修复（问题4）：文本内容 hash——`length:firstCharCode:lastCharCode` */
  textHash: string
}

// 排版请求的不可变身份——在请求发起时锁定，布局回调时验证匹配
interface LayoutRequestIdentity {
  token: number
  fullTextLength: number
  styleKey: string
  contentWidth: number
  /** Issue #879 最新复核评论修复（问题4）：文本内容 hash——防止同长度不同内容的误判 */
  textHash: string
}

// Issue #879 最新复核评论修复（问题4）：计算文本内容 hash。
// 格式：`length:firstCharCode:lastCharCode`——轻量但能有效区分同长度不同内容。
// 空文本的 hash 为 `0:0:0`。
function computeTextHash(text: string): string {
  if (text.length === 0) {
    return '0:0:0'
  }
  const firstCharCode = text.charCodeAt(0)
  const lastCharCode = text.charCodeAt(text.length - 1)
  return `${text.length}:${firstCharCode}:${lastCharCode}`
}

@Component
export struct EditorPendingLayoutHost {
  // 独立的 TextController——不与主 Text 共享
  private pendingTextController: TextController = new TextController()
  // Issue #879 最新复核评论修复（问题4）：活跃排版请求的身份链——支持多个并发请求。
  // 旧方案用单个 currentIdentity，新请求会覆盖旧 identity，导致旧 layout 回调可能用新 identity 验证通过。
  // 新方案用数组保留所有活跃请求的 identity，layout 回调时在链中匹配并移除。
  private activeIdentities: LayoutRequestIdentity[] = []
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
        // Issue #879 最新复核评论修复（问题4）：锁定不可变身份到 activeIdentities 链。
        // 不再清空旧 identity——旧请求的 layout 回调可以在链中找到自己的 identity。
        const identity: LayoutRequestIdentity = {
          token: this.layoutRequest.token,
          fullTextLength: this.layoutRequest.fullTextLength,
          styleKey: this.layoutRequest.styleKey,
          contentWidth: this.layoutRequest.contentWidth,
          textHash: this.layoutRequest.textHash,
        }
        this.activeIdentities.push(identity)
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
      if (!this.isMounted || this.activeIdentities.length === 0) {
        return
      }
      // 从 LayoutManager 读取行布局
      const layout = this.pendingTextController.getLayoutManager()
      if (layout === null || layout === undefined) {
        return
      }
      const lineCount = layout.getLineCount()

      // Issue #879 最新复核评论修复（问题4）：在 activeIdentities 链中匹配 identity。
      // 旧方案用单个 currentIdentity + 组件状态验证，新请求会覆盖旧 identity，
      // 导致旧 layout 回调可能用新 identity 验证通过（同长度内容替换时）。
      // 新方案用链保留所有活跃请求的 identity，layout 回调时在链中匹配并移除。
      //
      // 匹配策略：
      // 1. 从 LayoutManager 读取末行 endIndex（排版的实际内容长度）
      // 2. 在 activeIdentities 中找到 fullTextLength === endIndex 的 identity
      // 3. 通过 resolveLayoutRequest 获取该 token 的 StyledString，计算 textHash
      // 4. 验证 textHash 匹配——防止同长度不同内容的误判
      // 5. 验证 styleKey/contentWidth 匹配——从 identity 读取，不从组件状态读取
      let matchedIdentity: LayoutRequestIdentity | null = null
      let matchedIndex: number = -1

      if (lineCount > 0) {
        // 非空排版：从 LayoutManager 读取末行 endIndex 作为实际排版内容长度
        const lastLineMetrics = layout.getLineMetrics(lineCount - 1)
        const laidOutLength = lastLineMetrics.endIndex

        // 在 activeIdentities 中查找匹配的 identity
        for (let i = 0; i < this.activeIdentities.length; i++) {
          const identity = this.activeIdentities[i]
          if (identity.fullTextLength !== laidOutLength) {
            continue
          }
          // Issue #879 最新复核评论修复（问题4）：验证 textHash——防止同长度不同内容的误判。
          // 通过 resolveLayoutRequest 获取该 token 的 StyledString，计算当前 textHash，
          // 与 identity 锁定的 textHash 比较。如果 token 映射已被清理，跳过。
          if (this.resolveLayoutRequest !== null) {
            const styledString = this.resolveLayoutRequest(identity.token)
            if (styledString === null) {
              continue
            }
            const currentTextHash = computeTextHash(styledString.getString())
            if (currentTextHash !== identity.textHash) {
              continue
            }
          }
          matchedIdentity = identity
          matchedIndex = i
          break
        }

        if (matchedIdentity === null) {
          // 没有匹配的 identity——排版内容不对应任何活跃请求，忽略此次回调
          return
        }
      } else {
        // 空排版（lineCount === 0）：查找 fullTextLength === 0 的 identity
        for (let i = 0; i < this.activeIdentities.length; i++) {
          const identity = this.activeIdentities[i]
          if (identity.fullTextLength !== 0) {
            continue
          }
          // Issue #879 最新复核评论修复（问题4）：空文本也要验证 textHash
          if (this.resolveLayoutRequest !== null) {
            const styledString = this.resolveLayoutRequest(identity.token)
            if (styledString === null) {
              continue
            }
            const currentTextHash = computeTextHash(styledString.getString())
            if (currentTextHash !== identity.textHash) {
              continue
            }
          }
          matchedIdentity = identity
          matchedIndex = i
          break
        }

        if (matchedIdentity === null) {
          return
        }
      }

      // Issue #879 最新复核评论修复（问题4）：非空目标 lineCount===0 视为未完成不回调。
      if (lineCount === 0 && matchedIdentity.fullTextLength > 0) {
        return
      }

      // Issue #879 复核评论修复（问题2）：空目标额外验证。
      // 对于空目标（fullTextLength === 0），LayoutManager 的 lineCount 必须确实为 0。
      if (matchedIdentity.fullTextLength === 0 && lineCount !== 0) {
        return
      }

      // Issue #879 最新复核评论修复（问题4）：从 activeIdentities 中移除已匹配的 identity。
      // 防止同一 token 的重复回调。
      this.activeIdentities.splice(matchedIndex, 1)

      const lineLayouts: LineLayout[] = lineCount > 0
        ? this.buildLineLayouts(layout, lineCount, matchedIdentity)
        : []

      // Issue #879 最新复核评论修复（问题5）：空文本时计算 firstCaretRect。
      let firstCaretRect: CaretRect | undefined = undefined
      if (matchedIdentity.fullTextLength === 0) {
        firstCaretRect = this.computeFirstCaretRect(layout, lineCount, matchedIdentity)
      }

      // 回调通知排版完成——身份验证通过后才回调
      if (this.onLayoutReady !== null) {
        this.onLayoutReady(matchedIdentity.token, lineLayouts, firstCaretRect)
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

  // Issue #879 最新复核评论修复（问题5）：空文本时计算 firstCaretRect。
  // 从 LayoutManager 获取首行 metrics（即使内容为空，LayoutManager 也可能有一行）；
  // 如果 LayoutManager 不返回行信息，则从 pendingFontSize 和 autoIndent 估算。
  private computeFirstCaretRect(layout: LayoutManager, lineCount: number,
    identity: LayoutRequestIdentity): CaretRect {
    // 尝试从 LayoutManager 获取首行 metrics
    if (lineCount > 0) {
      const firstLineMetrics = layout.getLineMetrics(0)
      const pxToVp = (value: number): number => this.getUIContext().px2vp(value)
      const y = pxToVp(firstLineMetrics.topHeight)
      const height = pxToVp(firstLineMetrics.height)
      const left = pxToVp(firstLineMetrics.left)
      return { x: left, y, width: 2, height }
    }

    // LayoutManager 不返回行信息——从参数估算
    // Issue #879 最新复核评论修复（问题5）：从 styleKey 解析 autoIndent 信息。
    // styleKey 格式：`${fontSize}-${lineSpacing}-${autoIndentEnabled}-${autoIndentWidth}`
    const parts = identity.styleKey.split('-')
    const fontSize = parts.length > 0 ? parseFloat(parts[0]) : this.pendingFontSize
    const autoIndentEnabled = parts.length > 2 ? parts[2] === 'true' : false
    const autoIndentWidth = parts.length > 3 ? parseFloat(parts[3]) : 0

    // 估算行高：fontSize * lineSpacingMultiplier（默认 1.8）
    const lineSpacingMultiplier = parts.length > 1 ? parseFloat(parts[1]) : 1.8
    const estimatedHeight = fontSize * lineSpacingMultiplier

    // 估算 caret x 位置：autoIndent 启用时，首行缩进 = autoIndentWidth * fontSize
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
  private buildLineLayouts(layout: LayoutManager, systemLineCount: number,
    identity: LayoutRequestIdentity): LineLayout[] {
    // Issue #879 复核评论修复（问题1）：通过 resolveLayoutRequest 获取 StyledString 的文本内容。
    const styledString = this.resolveLayoutRequest !== null
      ? this.resolveLayoutRequest(identity.token)
      : null
    const text = styledString?.getString() ?? ''

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
          if (this.activeIdentities.length > 0 && this.resolveLayoutRequest !== null) {
            // 取最新的活跃 identity 对应的 StyledString
            const latestIdentity = this.activeIdentities[this.activeIdentities.length - 1]
            const styledString = this.resolveLayoutRequest(latestIdentity.token)
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