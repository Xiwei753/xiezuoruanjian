// SujianEditor — 写作区组件宿主。
// Issue #1034 评论 #5：改用 ArkUI 原生 RichEditor 替换自研编辑器。
// Issue #1034 评论 #6/#7/#8 修复：正确使用 RichEditor API、onReady 加载正文、
//   fontSize/lineHeight 通过 span style 应用、移除无效 lineSpacingMultiplier 属性、
//   移除 NativeCaretOverlay 空状态、onPaste 区分粘贴事件。
// Issue #1034 评论 #8 补全：纠错下划线、标注背景/链接下划线、超链接 span 点击回调。
// Issue #1053：原生段落断行策略统一入口；标注 Span 只改绘制样式、不删改正文；
//   超链接命中改用原生 LayoutManager 字形偏移；恢复原生写作区运行时诊断快照。
// 系统组件自行处理纵向滚动、长按选区、拖拽选择、手柄、粘贴、系统 IME 与无障碍。

import { NativeRichEditorBridge, NativeEditorStateEvent } from '../input/NativeRichEditorBridge'
import { EditorSessionCoordinator } from '../session/EditorSessionCoordinator'
import { MappedCorrectionSuggestion } from '../correction/CorrectionDtos'
import { AnnotationRangeDto } from '../../../corebridge/dto/AnnotationDtos'
import { TextOffsetMapper } from '../input/TextOffsetMapper'
import { PlatformApiResolver } from '../../../platform/version/PlatformApiResolver'
import {
  EditorRuntimeSnapshotStore,
  RichEditorRuntimeSnapshot,
  RichEditorLineMetricsSnapshot
} from '../diagnostics/EditorRuntimeSnapshotStore'
import hilog from '@ohos.hilog'

const EDITOR_LOG_DOMAIN = 0x0000
const EDITOR_LOG_TAG = 'SujianEditor'
const EDITOR_HORIZONTAL_PADDING_VP = 20
const EDITOR_TOP_PADDING_VP = 20
const EDITOR_BOTTOM_PADDING_VP = 32
// Issue #1053：快照是内存态更新，150ms 合并一次即可；落盘仍由 Checkpoint/Worker 合并。
const EDITOR_SNAPSHOT_DEBOUNCE_MS = 150
// Issue #1053 评论 7100110974：中文标点排版使用系统正常断行 + 高质量换行策略，
// 不能再用 BREAK_ALL 覆盖系统断行；标点溢出/行首压缩按 API 版本启用。
const EDITOR_EXPECTED_WORD_BREAK_LABEL = 'BREAK_WORD'
const EDITOR_EXPECTED_TEXT_ALIGN_LABEL = 'Start'
const EDITOR_EXPECTED_LINE_BREAK_STRATEGY_LABEL = 'HIGH_QUALITY'
// compressLeadingPunctuation 自 API 23 起可用，punctuationOverflow 自 API 26 起可用。
const EDITOR_COMPRESS_LEADING_PUNCTUATION_MIN_API = 23
const EDITOR_PUNCTUATION_OVERFLOW_MIN_API = 26
const EDITOR_TAG_BACKGROUND_COLOR = '#3498DB33'
const EDITOR_LINK_UNDERLINE_COLOR = '#3498DB'
const EDITOR_CORRECTION_UNDERLINE_COLOR = '#E74C3C'
// setTypingParagraphStyle 自 API 20 起可用；低版本只应用 updateParagraphStyle。
const EDITOR_TYPING_PARAGRAPH_STYLE_MIN_API = 20

// 标注装饰区间（UTF-16，已按正文长度裁剪）。
interface DecorationRange {
  start: number
  end: number
}

// 原生选区（UTF-16；不可读时为 -1）。
interface NativeSelectionOffsets {
  start: number
  end: number
}

// 不重叠的装饰段：同一段可同时带标注背景与链接下划线。
interface DecorationSegment extends DecorationRange {
  hasTagBackground: boolean
  hasLinkUnderline: boolean
}

// Issue #1053 评论 7100110974：中文标点原生排版属性做 API 版本隔离。
// compressLeadingPunctuation（API 23+）与 punctuationOverflow（API 26+）都是
// RichEditorAttribute，用 AttributeModifier 按设备能力启用，不手写断行/空格操作。
class RichEditorPunctuationModifier implements AttributeModifier<RichEditorAttribute> {
  // Issue #1053 评论 7100350617：记录原生属性调用的真实结果（无 readback，不伪称已读回）。
  compressLeadingPunctuationCallSucceeded: boolean = false
  compressLeadingPunctuationError: string | null = null
  punctuationOverflowCallSucceeded: boolean = false
  punctuationOverflowError: string | null = null

  applyNormalAttribute(instance: RichEditorAttribute): void {
    const resolver = PlatformApiResolver.getInstance()
    if (resolver.isApiAtLeast(EDITOR_COMPRESS_LEADING_PUNCTUATION_MIN_API)) {
      try {
        instance.compressLeadingPunctuation(true)
        this.compressLeadingPunctuationCallSucceeded = true
        this.compressLeadingPunctuationError = null
      } catch (e) {
        this.compressLeadingPunctuationCallSucceeded = false
        this.compressLeadingPunctuationError = JSON.stringify(e)
        hilog.warn(EDITOR_LOG_DOMAIN, EDITOR_LOG_TAG,
          `compressLeadingPunctuation unavailable: ${this.compressLeadingPunctuationError}`)
      }
    }
    if (resolver.isApiAtLeast(EDITOR_PUNCTUATION_OVERFLOW_MIN_API)) {
      try {
        instance.punctuationOverflow(true)
        this.punctuationOverflowCallSucceeded = true
        this.punctuationOverflowError = null
      } catch (e) {
        this.punctuationOverflowCallSucceeded = false
        this.punctuationOverflowError = JSON.stringify(e)
        hilog.warn(EDITOR_LOG_DOMAIN, EDITOR_LOG_TAG,
          `punctuationOverflow unavailable: ${this.punctuationOverflowError}`)
      }
    }
  }
}

@Component
export struct SujianEditor {
  editorCoordinator: EditorSessionCoordinator | null = null
  richEditorBridge: NativeRichEditorBridge | null = null
  @Prop @Watch('onCorrectionSuggestionsChanged') correctionSuggestions: MappedCorrectionSuggestion[] = []
  @Prop @Watch('onAnnotationRangesChanged') annotationRanges: AnnotationRangeDto[] = []
  hasHyperlinkAt: (utf16Offset: number, revision: number, text: string) => boolean =
    (_utf16Offset: number, _revision: number, _text: string): boolean => false
  onHyperlinkTap: (utf16Offset: number, revision: number, text: string) => boolean =
    (_utf16Offset: number, _revision: number, _text: string): boolean => false
  @Prop @Watch('onFormatChanged') fontSize: number = 16
  @Prop @Watch('onFormatChanged') lineSpacingMultiplier: number = 1.8
  caretColor: ResourceColor = $r('sys.color.ohos_id_color_text_primary')
  private richEditorController: RichEditorController = new RichEditorController()
  // 原生组件 onReady 后才允许改 Span/段落样式，避免对未绑定的 controller 操作。
  private editorReady: boolean = false
  private previousCorrectionRanges: number[] = []
  // Issue #1053：记录上一轮已应用装饰段，只用于清除绘制样式，不再删除/重建正文 Span。
  private appliedDecorationSegments: DecorationSegment[] = []
  private editorFocused: boolean = false
  private nativeEditing: boolean = false
  private editorWidthVp: number = 0
  private editorHeightVp: number = 0
  // Issue #1053 评论 7100110974：原生标点排版按 API 版本启用（AttributeModifier）。
  private punctuationModifier: RichEditorPunctuationModifier = new RichEditorPunctuationModifier()
  // 非热路径读取到的原生实际段落样式（诊断报告真实值）。
  private appliedWordBreakLabel: string = 'unknown'
  private appliedTextAlignLabel: string = 'unknown'
  private appliedLineBreakStrategyLabel: string = 'unknown'
  private paragraphStyleChecked: boolean = false
  private checkedParagraphCount: number = 0
  private mismatchedParagraphCount: number = 0
  private firstMismatchParagraphRange: string = ''
  private paragraphStyleApplied: boolean = false
  private paragraphStyleError: string | null = null
  // 新输入/新段落是否继承原生段落样式（setTypingParagraphStyle，API 20+）。
  private typingParagraphStyleSupported: boolean = false
  private typingParagraphStyleApplied: boolean = false
  private typingParagraphStyleError: string | null = null
  private snapshotTimerId: number = -1

  aboutToDisappear(): void {
    this.cancelSnapshotTimer()
    // 离页是非热路径：先刷新完整事实，再落一份挂载中的最后快照，最后标记已卸载。
    if (this.richEditorBridge) {
      this.richEditorBridge.captureDiagnosticsFacts()
    }
    this.publishRuntimeSnapshot()
    EditorRuntimeSnapshotStore.getInstance().markEditorUnmounted()
    if (this.richEditorBridge) {
      this.richEditorBridge.setStateListener(null)
      this.richEditorBridge.beginSession()
    }
    this.editorReady = false
  }

  // 纠错建议变化时更新 RichEditor 中的样式
  // Issue #1034 评论 #8：通过 updateSpanStyle 实现纠错波浪下划线。
  // 先清除上一轮装饰范围，再应用新一轮。
  private onCorrectionSuggestionsChanged(): void {
    if (!this.editorReady) {
      return
    }

    const lineHeight: number = this.fontSize * this.lineSpacingMultiplier

    for (let i = 0; i < this.previousCorrectionRanges.length; i += 2) {
      const start = this.previousCorrectionRanges[i]
      const end = this.previousCorrectionRanges[i + 1]
      this.applySpanTextStyle(start, end, {
        fontSize: this.fontSize,
        lineHeight: lineHeight,
        decoration: { type: TextDecorationType.None }
      })
    }
    this.previousCorrectionRanges = []

    if (this.correctionSuggestions.length === 0) {
      return
    }

    for (const suggestion of this.correctionSuggestions) {
      this.applySpanTextStyle(suggestion.startUtf16, suggestion.endUtf16, {
        fontSize: this.fontSize,
        lineHeight: lineHeight,
        decoration: {
          type: TextDecorationType.Underline,
          color: EDITOR_CORRECTION_UNDERLINE_COLOR,
          style: TextDecorationStyle.WAVY
        }
      })
      this.previousCorrectionRanges.push(suggestion.startUtf16, suggestion.endUtf16)
    }
  }

  // 标注范围变化时只更新绘制样式。
  // Issue #1053：旧实现用 deleteSpans + addTextSpan 重建链接 Span，会扰动正文 Span 边界、
  // 选区与 IME 生存期，并让原生 AceRichText spanIndex error 难以定位。
  // 现在 tagBackground / linkUnderline 全部走 updateSpanStyle；重叠标注按段合成样式，
  // 装饰样式变化不再触碰正文 Span 的文本内容。
  // AnnotationRangeDto 的 startByte/endByte 是 UTF-8，需转换为 UTF-16。
  private onAnnotationRangesChanged(): void {
    if (!this.editorReady) {
      return
    }

    const lineHeight: number = this.fontSize * this.lineSpacingMultiplier

    // 标注装饰只改绘制样式，不再隔离为程序性正文更新（正文 Span 内容保持不变）。
    this.resetAnnotationDecorationStyles(lineHeight)

    const text: string = this.richEditorBridge !== null ? this.richEditorBridge.getCurrentText() : ''
    if (this.annotationRanges.length > 0 && text.length > 0) {
      const tagRanges: DecorationRange[] = []
      const linkRanges: DecorationRange[] = []
      for (const range of this.annotationRanges) {
        const startUtf16 = TextOffsetMapper.utf8ToUtf16(text, range.startByte)
        const endUtf16 = TextOffsetMapper.utf8ToUtf16(text, range.endByte)
        const clipped = this.clipDecorationRange(startUtf16, endUtf16, text.length)
        if (clipped === null) {
          continue
        }
        if (range.kind === 'tagBackground') {
          tagRanges.push(clipped)
        } else if (range.kind === 'linkUnderline') {
          linkRanges.push(clipped)
        }
      }

      const segments = this.composeDecorationSegments(tagRanges, linkRanges)
      for (const segment of segments) {
        this.applySpanTextStyle(segment.start, segment.end, {
          fontSize: this.fontSize,
          lineHeight: lineHeight,
          textBackgroundStyle: {
            color: segment.hasTagBackground ? EDITOR_TAG_BACKGROUND_COLOR : Color.Transparent
          },
          decoration: segment.hasLinkUnderline
            ? {
                type: TextDecorationType.Underline,
                color: EDITOR_LINK_UNDERLINE_COLOR,
                style: TextDecorationStyle.SOLID
              }
            : { type: TextDecorationType.None }
        })
        this.appliedDecorationSegments.push({
          start: segment.start,
          end: segment.end,
          hasTagBackground: segment.hasTagBackground,
          hasLinkUnderline: segment.hasLinkUnderline
        })
      }
    }

    // 纠错波浪线与链接下划线可能重叠：纠错建议最后应用，保证重叠区优先显示纠错样式。
    this.onCorrectionSuggestionsChanged()
  }

  // 清除上一轮标注装饰：只恢复绘制属性，不删除/重建正文 Span。
  private resetAnnotationDecorationStyles(lineHeight: number): void {
    for (const segment of this.appliedDecorationSegments) {
      this.applySpanTextStyle(segment.start, segment.end, {
        fontSize: this.fontSize,
        lineHeight: lineHeight,
        textBackgroundStyle: { color: Color.Transparent },
        decoration: { type: TextDecorationType.None }
      })
    }
    this.appliedDecorationSegments = []
  }

  // 单次样式更新失败（旧范围已被正文变化截断）不阻断其余范围。
  private applySpanTextStyle(start: number, end: number, textStyle: RichEditorTextStyle): void {
    if (end <= start) {
      return
    }
    try {
      this.richEditorController.updateSpanStyle({
        start: start,
        end: end,
        textStyle: textStyle
      })
    } catch (e) {
      hilog.warn(EDITOR_LOG_DOMAIN, EDITOR_LOG_TAG,
        `updateSpanStyle failed start=${start} end=${end}: ${JSON.stringify(e)}`)
    }
  }

  private clipDecorationRange(start: number, end: number, textLength: number): DecorationRange | null {
    const clippedStart = Math.max(0, Math.min(start, textLength))
    const clippedEnd = Math.max(0, Math.min(end, textLength))
    if (clippedEnd <= clippedStart) {
      return null
    }
    return { start: clippedStart, end: clippedEnd }
  }

  // 按标注边界切段，逐段合成覆盖的样式；相同样式的相邻段合并，减少 updateSpanStyle 次数。
  private composeDecorationSegments(tagRanges: DecorationRange[],
    linkRanges: DecorationRange[]): DecorationSegment[] {
    const boundaries: number[] = []
    for (const range of tagRanges) {
      boundaries.push(range.start, range.end)
    }
    for (const range of linkRanges) {
      boundaries.push(range.start, range.end)
    }
    if (boundaries.length === 0) {
      return []
    }
    boundaries.sort((a: number, b: number): number => a - b)

    const uniqueBoundaries: number[] = []
    for (const boundary of boundaries) {
      if (uniqueBoundaries.length === 0 || uniqueBoundaries[uniqueBoundaries.length - 1] !== boundary) {
        uniqueBoundaries.push(boundary)
      }
    }

    const segments: DecorationSegment[] = []
    for (let i = 0; i + 1 < uniqueBoundaries.length; i++) {
      const start = uniqueBoundaries[i]
      const end = uniqueBoundaries[i + 1]
      if (end <= start) {
        continue
      }
      const hasTagBackground = this.isCoveredByRanges(tagRanges, start, end)
      const hasLinkUnderline = this.isCoveredByRanges(linkRanges, start, end)
      if (!hasTagBackground && !hasLinkUnderline) {
        continue
      }
      const last = segments.length > 0 ? segments[segments.length - 1] : null
      if (last !== null && last.end === start &&
          last.hasTagBackground === hasTagBackground && last.hasLinkUnderline === hasLinkUnderline) {
        last.end = end
        continue
      }
      segments.push({
        start: start,
        end: end,
        hasTagBackground: hasTagBackground,
        hasLinkUnderline: hasLinkUnderline
      })
    }
    return segments
  }

  private isCoveredByRanges(ranges: DecorationRange[], start: number, end: number): boolean {
    for (const range of ranges) {
      if (range.start <= start && end <= range.end) {
        return true
      }
    }
    return false
  }

  // Issue #1053：原生段落样式单一入口。
  // wordBreak 是 RichEditorParagraphStyle 属性（不能放进 RichEditorTextStyle）；
  // setTypingParagraphStyle 让普通输入/新段落继承，updateParagraphStyle 修正已有段落。
  // 只由初始化、程序性正文替换和段落结构变化调用，不随选区变化重排整篇。
  private applyNativeParagraphStyle(): void {
    if (!this.editorReady) {
      return
    }
    // Issue #1053 评论 7100110974：使用系统正常断行 + 高质量换行策略。
    // BREAK_ALL 会绕过 lineBreakStrategy，也无法得到中文标点的系统行首/行尾优化。
    const style: RichEditorParagraphStyle = {
      textAlign: TextAlign.Start,
      wordBreak: WordBreak.BREAK_WORD,
      lineBreakStrategy: LineBreakStrategy.HIGH_QUALITY
    }
    // setTypingParagraphStyle 自 API 20 起可用；低版本或组件未绑定时捕获异常，
    // 已有段落仍由 updateParagraphStyle 修正。调用结果单独记录：
    // 旧段落正确不代表后续新输入会继承正确折行规则。
    this.typingParagraphStyleSupported =
      PlatformApiResolver.getInstance().isApiAtLeast(EDITOR_TYPING_PARAGRAPH_STYLE_MIN_API)
    this.typingParagraphStyleApplied = false
    this.typingParagraphStyleError = null
    if (this.typingParagraphStyleSupported) {
      try {
        this.richEditorController.setTypingParagraphStyle(style)
        this.typingParagraphStyleApplied = true
      } catch (e) {
        this.typingParagraphStyleError = `setTypingParagraphStyle failed: ${JSON.stringify(e)}`
        hilog.warn(EDITOR_LOG_DOMAIN, EDITOR_LOG_TAG, this.typingParagraphStyleError)
      }
    }
    try {
      this.richEditorController.updateParagraphStyle({
        start: -1,
        end: -1,
        style: style
      })
    } catch (e) {
      // 段落样式没有写入成功，读回结论一并失效，不能沿用上一轮的 checked 状态。
      this.paragraphStyleChecked = false
      this.checkedParagraphCount = 0
      this.mismatchedParagraphCount = 0
      this.firstMismatchParagraphRange = ''
      this.paragraphStyleApplied = false
      this.paragraphStyleError = `updateParagraphStyle failed: ${JSON.stringify(e)}`
      hilog.warn(EDITOR_LOG_DOMAIN, EDITOR_LOG_TAG, this.paragraphStyleError)
      this.publishRuntimeSnapshot()
      return
    }
    this.refreshAppliedParagraphStyleFacts()
  }

  // Issue #1053 评论 7100110974：非热路径读取原生实际段落样式（初始化/切章/样式变更后），
  // 诊断不能再固定上报常量。完整正文段落只在此时读取，不进入逐键热路径。
  private refreshAppliedParagraphStyleFacts(): void {
    if (!this.editorReady) {
      return
    }
    try {
      const paragraphs = this.richEditorController.getParagraphs({ start: -1, end: -1 })
      this.paragraphStyleChecked = true
      if (paragraphs === undefined || paragraphs === null || paragraphs.length === 0) {
        // 空文档没有段落可核实：不能当作已生效。
        this.checkedParagraphCount = 0
        this.mismatchedParagraphCount = 0
        this.firstMismatchParagraphRange = ''
        this.paragraphStyleApplied = false
        this.paragraphStyleError = 'no-paragraph'
      } else {
        let mismatchCount = 0
        let firstRange = ''
        let firstSummary = ''
        for (const paragraph of paragraphs) {
          const style = paragraph.style
          const wordBreakLabel = this.wordBreakLabel(style.wordBreak)
          const textAlignLabel = this.textAlignLabel(style.textAlign)
          const strategyLabel = this.lineBreakStrategyLabel(style.lineBreakStrategy)
          const matched = wordBreakLabel === EDITOR_EXPECTED_WORD_BREAK_LABEL &&
            textAlignLabel === EDITOR_EXPECTED_TEXT_ALIGN_LABEL &&
            strategyLabel === EDITOR_EXPECTED_LINE_BREAK_STRATEGY_LABEL
          if (!matched) {
            if (mismatchCount === 0) {
              firstRange = `[${paragraph.range[0]}, ${paragraph.range[1]})`
              firstSummary = `wordBreak=${wordBreakLabel}`
                + ` lineBreakStrategy=${strategyLabel}`
                + ` textAlign=${textAlignLabel}`
            }
            mismatchCount++
          }
          // 快照仍展示首段实际值，供人工核对；总体结论以全部段落为准。
          if (paragraphs[0] === paragraph) {
            this.appliedWordBreakLabel = wordBreakLabel
            this.appliedTextAlignLabel = textAlignLabel
            this.appliedLineBreakStrategyLabel = strategyLabel
          }
        }
        this.checkedParagraphCount = paragraphs.length
        this.mismatchedParagraphCount = mismatchCount
        this.paragraphStyleApplied = mismatchCount === 0
        this.firstMismatchParagraphRange = firstRange
        this.paragraphStyleError = mismatchCount === 0
          ? null
          : `${mismatchCount}/${paragraphs.length} paragraph(s) style mismatch`
            + ` firstRange=${firstRange} ${firstSummary}`
      }
    } catch (e) {
      this.paragraphStyleChecked = false
      this.checkedParagraphCount = 0
      this.mismatchedParagraphCount = 0
      this.firstMismatchParagraphRange = ''
      this.paragraphStyleApplied = false
      this.paragraphStyleError = `getParagraphs failed: ${JSON.stringify(e)}`
      hilog.warn(EDITOR_LOG_DOMAIN, EDITOR_LOG_TAG, this.paragraphStyleError)
    }
    this.publishRuntimeSnapshot()
  }

  private wordBreakLabel(value: WordBreak | undefined): string {
    if (value === undefined) {
      return 'default'
    }
    if (value === WordBreak.BREAK_WORD) {
      return 'BREAK_WORD'
    }
    if (value === WordBreak.BREAK_ALL) {
      return 'BREAK_ALL'
    }
    if (value === WordBreak.NORMAL) {
      return 'NORMAL'
    }
    return `unknown(${value})`
  }

  private textAlignLabel(value: TextAlign | undefined): string {
    if (value === undefined) {
      return 'default'
    }
    if (value === TextAlign.Start) {
      return 'Start'
    }
    if (value === TextAlign.Center) {
      return 'Center'
    }
    if (value === TextAlign.End) {
      return 'End'
    }
    if (value === TextAlign.JUSTIFY) {
      return 'JUSTIFY'
    }
    return `unknown(${value})`
  }

  private lineBreakStrategyLabel(value: LineBreakStrategy | undefined): string {
    if (value === undefined) {
      return 'default'
    }
    if (value === LineBreakStrategy.HIGH_QUALITY) {
      return 'HIGH_QUALITY'
    }
    if (value === LineBreakStrategy.GREEDY) {
      return 'GREEDY'
    }
    if (value === LineBreakStrategy.BALANCED) {
      return 'BALANCED'
    }
    return `unknown(${value})`
  }

  // 字号/行距变化时更新 RichEditor 的文本样式
  // Issue #1034 评论 #8：updateSpanStyle 官方签名是单参数：
  // updateSpanStyle({ start, end, textStyle: { fontSize, lineHeight } })
  private onFormatChanged(): void {
    if (!this.editorReady) {
      return
    }
    const lineHeight: number = this.fontSize * this.lineSpacingMultiplier
    this.richEditorController.setTypingStyle({
      fontSize: this.fontSize,
      lineHeight: lineHeight
    })
    this.richEditorController.updateSpanStyle({
      start: 0,
      end: -1,
      textStyle: {
        fontSize: this.fontSize,
        lineHeight: lineHeight
      }
    })
  }

  // Issue #1053：原生编辑器状态变化。只有程序性正文替换（新增/删除段落）才重新应用
  // 原生段落样式与字号/行距；普通输入/回车由 setTypingParagraphStyle、setTypingStyle 继承，
  // 不随每次输入或选区变化对全文重新排版。任何状态变化都合并更新内存快照。
  private handleNativeEditorState(event: NativeEditorStateEvent): void {
    if (!this.editorReady) {
      return
    }
    if (event.paragraphsChanged && (event.kind === 'sync-from-core' || event.kind === 'initial-load')) {
      // 程序性 deleteSpans + addTextSpan 会丢掉新增 Span 的字号/行距/段落样式。
      this.onFormatChanged()
      this.applyNativeParagraphStyle()
    }
    this.scheduleRuntimeSnapshot()
  }

  // Issue #1053：超链接命中改用 RichEditor 自身点击事件 + 原生 LayoutManager 字形定位。
  // ClickEvent.x/y 是组件局部坐标（vp），LayoutManager 需要 px；左右内边距在宿主 Stack 上，
  // RichEditor 的局部原点就是正文内容区原点，不再重复扣减内边距。
  private handleContentClick(event: ClickEvent): void {
    if (this.richEditorBridge === null) {
      return
    }
    const fullText = this.richEditorBridge.getCurrentText()
    if (fullText.length === 0) {
      return
    }
    try {
      const layoutManager = this.richEditorController.getLayoutManager()
      if (layoutManager === undefined || layoutManager === null) {
        return
      }
      const glyphPosition = layoutManager.getGlyphPositionAtCoordinate(
        this.vpToPx(event.x), this.vpToPx(event.y))
      if (glyphPosition === undefined || glyphPosition === null) {
        return
      }
      const revision = this.richEditorBridge.getLastAppliedRevision()
      const offset = glyphPosition.position
      if (this.hasHyperlinkAt(offset, revision, fullText)) {
        this.onHyperlinkTap(offset, revision, fullText)
      }
    } catch (e) {
      hilog.warn(EDITOR_LOG_DOMAIN, EDITOR_LOG_TAG, `hyperlink hit test failed: ${JSON.stringify(e)}`)
    }
  }

  // Issue #1034 评论 #8：初始化顺序——先加载正文，再应用格式。
  // Issue #1053：syncFromCore/onFormatChanged 之后应用原生段落样式；loadInitialText 写完后同样应用。
  private async initializeEditor(): Promise<void> {
    if (this.richEditorBridge) {
      await this.richEditorBridge.syncFromCore()
    }
    this.onFormatChanged()
    this.applyNativeParagraphStyle()
    this.onCorrectionSuggestionsChanged()
    this.onAnnotationRangesChanged()
    this.publishRuntimeSnapshot()
  }

  async syncFromCore(): Promise<void> {
    if (this.richEditorBridge) {
      await this.richEditorBridge.syncFromCore()
      this.publishRuntimeSnapshot()
    }
  }

  async loadInitialText(text: string, cursorByteOffset: number): Promise<void> {
    if (this.richEditorBridge) {
      await this.richEditorBridge.loadInitialText(text, cursorByteOffset)
      this.onFormatChanged()
      this.applyNativeParagraphStyle()
      this.publishRuntimeSnapshot()
    }
  }

  // ── Issue #1053：原生写作区运行时快照（不含正文原文） ──────────────

  private scheduleRuntimeSnapshot(): void {
    if (this.snapshotTimerId !== -1) {
      return
    }
    this.snapshotTimerId = setTimeout(() => {
      this.snapshotTimerId = -1
      this.publishRuntimeSnapshot()
    }, EDITOR_SNAPSHOT_DEBOUNCE_MS)
  }

  private cancelSnapshotTimer(): void {
    if (this.snapshotTimerId !== -1) {
      clearTimeout(this.snapshotTimerId)
      this.snapshotTimerId = -1
    }
  }

  private publishRuntimeSnapshot(): void {
    this.cancelSnapshotTimer()

    const bridge = this.richEditorBridge
    const coordinator = this.editorCoordinator
    const coreSnapshot = coordinator !== null ? coordinator.getSnapshot() : null
    const coreText = coreSnapshot !== null ? coreSnapshot.text : ''
    const nativeText = bridge !== null ? bridge.getCurrentText() : ''
    const coreSessionId = coordinator !== null ? coordinator.getSessionId() : null
    // Issue #1053 评论 7099290808：热路径快照只用增量维护的轻量事实；
    // 全文 hash / 全量 Span 数来自上次保存屏障的稳定态校验，不再每 150ms 遍历整章。
    const alignment = bridge !== null ? bridge.getLastNativeCoreAlignment() : null
    const nativeNewlineCount = bridge !== null ? bridge.getNativeNewlineCount() : 0
    const coreNewlineCount = bridge !== null ? bridge.getCoreNewlineCount() : 0
    const nativeSpanCount = bridge !== null ? bridge.getNativeSpanCount() : -1
    const caretUtf16 = this.readCaretUtf16()
    const selection = this.readNativeSelection()
    const editorWidthPx = this.vpToPx(this.editorWidthVp)
    const editorHeightPx = this.vpToPx(this.editorHeightVp)

    const resolver = PlatformApiResolver.getInstance()
    const snapshot: RichEditorRuntimeSnapshot = {
      // Issue #1053 评论 7100350617：段落读回/属性调用/API 支持分开记录。
      schemaVersion: 4,
      hasActiveSession: coreSnapshot !== null,
      sessionId: coreSessionId !== null ? coreSessionId : -1,
      nativeRevision: bridge !== null ? bridge.getLastAppliedRevision() : -1,
      coreRevision: coreSnapshot !== null ? coreSnapshot.revision : -1,
      nativeTextUtf16Length: nativeText.length,
      nativeNewlineCount: nativeNewlineCount,
      nativeTextHash: alignment !== null ? alignment.nativeHash : '',
      coreTextUtf16Length: coreText.length,
      coreNewlineCount: coreNewlineCount,
      coreTextHash: alignment !== null ? alignment.coreHash : '',
      nativeCoreTextMatched: alignment !== null ? alignment.matched : false,
      nativeCoreDiverged: bridge !== null ? bridge.isNativeCoreDiverged() : false,
      nativeSpanCount: nativeSpanCount,
      paragraphCount: nativeNewlineCount + 1,
      layoutLineCount: this.readLayoutLineCount(),
      caretUtf16: caretUtf16,
      selectionStartUtf16: selection.start,
      selectionEndUtf16: selection.end,
      editorWidthPx: editorWidthPx,
      editorHeightPx: editorHeightPx,
      // 内边距在宿主 Stack 上，RichEditor 实测宽度/高度就是正文内容区尺寸。
      contentWidthPx: editorWidthPx,
      contentHeightPx: editorHeightPx,
      horizontalPaddingVp: EDITOR_HORIZONTAL_PADDING_VP,
      wordBreak: this.appliedWordBreakLabel,
      lineBreakStrategy: this.appliedLineBreakStrategyLabel,
      textAlign: this.appliedTextAlignLabel,
      punctuationOverflowSupported: resolver.isApiAtLeast(EDITOR_PUNCTUATION_OVERFLOW_MIN_API),
      punctuationOverflowCallSucceeded: this.punctuationModifier.punctuationOverflowCallSucceeded,
      punctuationOverflowError: this.punctuationModifier.punctuationOverflowError,
      compressLeadingPunctuationSupported: resolver.isApiAtLeast(EDITOR_COMPRESS_LEADING_PUNCTUATION_MIN_API),
      compressLeadingPunctuationCallSucceeded: this.punctuationModifier.compressLeadingPunctuationCallSucceeded,
      compressLeadingPunctuationError: this.punctuationModifier.compressLeadingPunctuationError,
      paragraphStyleChecked: this.paragraphStyleChecked,
      checkedParagraphCount: this.checkedParagraphCount,
      mismatchedParagraphCount: this.mismatchedParagraphCount,
      firstMismatchParagraphRange: this.firstMismatchParagraphRange,
      paragraphStyleApplied: this.paragraphStyleApplied,
      paragraphStyleError: this.paragraphStyleError,
      typingParagraphStyleSupported: this.typingParagraphStyleSupported,
      typingParagraphStyleApplied: this.typingParagraphStyleApplied,
      typingParagraphStyleError: this.typingParagraphStyleError,
      deviceApiLevel: resolver.getApiLevel(),
      fontSizeVp: this.fontSize,
      lineSpacingMultiplier: this.lineSpacingMultiplier,
      effectiveLineHeightVp: this.fontSize * this.lineSpacingMultiplier,
      editorFocused: this.editorFocused,
      editing: this.nativeEditing,
      imeComposing: bridge !== null ? bridge.isImeComposing() : false,
      previewTextUtf16Length: this.readPreviewTextLength(),
      lastNativeOperation: bridge !== null ? bridge.getLastNativeOperation() : null,
      lastNativeCoreAlignment: bridge !== null ? bridge.getLastNativeCoreAlignment() : null,
      nativeFactError: bridge !== null ? bridge.getNativeFactError() : null,
      currentLineMetrics: this.readCurrentLineMetrics(caretUtf16),
      isEditorMounted: true,
      capturedAtMs: Date.now()
    }
    EditorRuntimeSnapshotStore.getInstance().updateSnapshot(snapshot)
  }

  // vp → px 统一走 UIContext（全局 vp2px 已废弃）；读数失败用 -1 表示不可用。
  private vpToPx(valueVp: number): number {
    try {
      return Math.round(this.getUIContext().vp2px(valueVp))
    } catch (e) {
      return -1
    }
  }

  private readCaretUtf16(): number {
    try {
      return this.richEditorController.getCaretOffset()
    } catch (e) {
      return -1
    }
  }

  private readNativeSelection(): NativeSelectionOffsets {
    try {
      const selection = this.richEditorController.getSelection()
      if (selection === undefined || selection === null || selection.selection.length < 2) {
        return { start: -1, end: -1 }
      }
      return { start: selection.selection[0], end: selection.selection[1] }
    } catch (e) {
      return { start: -1, end: -1 }
    }
  }

  private readPreviewTextLength(): number {
    try {
      const previewText = this.richEditorController.getPreviewText()
      return previewText !== undefined && previewText !== null ? previewText.value.length : 0
    } catch (e) {
      return 0
    }
  }

  private readLayoutLineCount(): number {
    try {
      const layoutManager = this.richEditorController.getLayoutManager()
      if (layoutManager === undefined || layoutManager === null) {
        return -1
      }
      return layoutManager.getLineCount()
    } catch (e) {
      return -1
    }
  }

  // 只抓光标所在行的 LineMetrics；行索引由系统布局的 startIndex/endIndex 二分定位。
  private readCurrentLineMetrics(caretUtf16: number): RichEditorLineMetricsSnapshot | null {
    try {
      const layoutManager = this.richEditorController.getLayoutManager()
      if (layoutManager === undefined || layoutManager === null) {
        return null
      }
      const lineCount = layoutManager.getLineCount()
      if (lineCount <= 0) {
        return null
      }
      const caret = Math.max(0, caretUtf16)
      let low = 0
      let high = lineCount - 1
      let target = -1
      while (low <= high) {
        const mid = Math.floor((low + high) / 2)
        const metrics = layoutManager.getLineMetrics(mid)
        if (caret < metrics.startIndex) {
          high = mid - 1
        } else if (caret > metrics.endIndex) {
          low = mid + 1
        } else {
          target = mid
          break
        }
      }
      if (target < 0) {
        target = 0
      }
      const line = layoutManager.getLineMetrics(target)
      return {
        lineIndex: line.lineNumber,
        startIndex: line.startIndex,
        endIndex: line.endIndex,
        height: line.height,
        width: line.width,
        baseline: line.baseline,
        left: line.left,
        topHeight: line.topHeight
      }
    } catch (e) {
      return null
    }
  }

  build() {
    // Issue #1053：宽度由外层 Stack 承担，左右 20vp 内边距只在 Stack 上计算一次。
    // RichEditor 自身默认还有 16vp 水平 / 8vp 竖直 padding，必须显式清零，
    // 这样正文排版宽度 = RichEditor 实测内容区宽度，内边距不会被算两次。
    Stack() {
      RichEditor({ controller: this.richEditorController })
        .width('100%')
        .height('100%')
        .padding(0)
        // 中文标点原生排版：按 API 能力启用行首压缩与行尾溢出。
        .attributeModifier(this.punctuationModifier)
        .barState(BarState.Auto)
        .caretColor(this.caretColor)
        .onReady(() => {
          hilog.info(EDITOR_LOG_DOMAIN, EDITOR_LOG_TAG, 'RichEditor onReady')
          if (this.richEditorBridge) {
            this.richEditorBridge.setController(this.richEditorController)
            this.richEditorBridge.setStateListener((event: NativeEditorStateEvent): void => {
              this.handleNativeEditorState(event)
            })
            this.editorReady = true
            this.initializeEditor()
          }
        })
        .onWillChange((value: RichEditorChangeValue): boolean => {
          if (this.richEditorBridge) {
            return this.richEditorBridge.onWillChange(value)
          }
          return true
        })
        .onDidChange((rangeBefore: TextRange, rangeAfter: TextRange) => {
          if (this.richEditorBridge) {
            this.richEditorBridge.onDidChange(rangeBefore, rangeAfter)
          }
          this.scheduleRuntimeSnapshot()
        })
        .onSelectionChange((value: TextRange): void => {
          if (this.richEditorBridge) {
            const start: number = value.start ?? 0
            const end: number = value.end ?? start
            this.richEditorBridge.onSelectionChange(start, end)
          }
          this.scheduleRuntimeSnapshot()
        })
        .onPaste(() => {
          if (this.richEditorBridge) {
            this.richEditorBridge.setPendingPaste()
          }
        })
        .enablePreviewText(true)
        .aboutToIMEInput((insertValue: RichEditorInsertValue): boolean => {
          if (this.richEditorBridge) {
            return this.richEditorBridge.aboutToIMEInput(insertValue)
          }
          return true
        })
        .onDidIMEInput((range: TextRange): void => {
          if (this.richEditorBridge) {
            this.richEditorBridge.onDidIMEInput(range)
          }
          this.scheduleRuntimeSnapshot()
        })
        .onEditingChange((isEditing: boolean) => {
          this.nativeEditing = isEditing
          this.scheduleRuntimeSnapshot()
        })
        .onAreaChange((_oldArea: Area, newArea: Area): void => {
          this.editorWidthVp = Number(newArea.width)
          this.editorHeightVp = Number(newArea.height)
          this.scheduleRuntimeSnapshot()
        })
        .onClick((event: ClickEvent) => {
          this.handleContentClick(event)
        })
        .onFocus(() => {
          hilog.info(EDITOR_LOG_DOMAIN, EDITOR_LOG_TAG, 'RichEditor focus acquired')
          this.editorFocused = true
          this.scheduleRuntimeSnapshot()
        })
        .onBlur(() => {
          hilog.info(EDITOR_LOG_DOMAIN, EDITOR_LOG_TAG, 'RichEditor focus lost')
          this.editorFocused = false
          // 失焦是非热路径：刷新一次完整事实（全文 Span 数与 hash），再更新诊断快照。
          if (this.richEditorBridge) {
            this.richEditorBridge.captureDiagnosticsFacts()
          }
          this.publishRuntimeSnapshot()
        })
    }
    .width('100%')
    .height('100%')
    .padding({
      left: EDITOR_HORIZONTAL_PADDING_VP,
      right: EDITOR_HORIZONTAL_PADDING_VP,
      top: EDITOR_TOP_PADDING_VP,
      bottom: EDITOR_BOTTOM_PADDING_VP
    })
  }
}
