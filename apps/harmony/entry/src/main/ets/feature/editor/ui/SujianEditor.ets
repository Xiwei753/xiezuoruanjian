// SujianEditor — 写作区组件宿主（属性字符串 RichEditor 模式）。
// Issue #1034 评论 #5：改用 ArkUI 原生 RichEditor 替换自研编辑器。
// Issue #1034 评论 #6/#7/#8：正确使用 RichEditor API、onReady 加载正文、
//   fontSize/lineHeight 通过文本样式应用、纠错下划线、标注背景/链接下划线、超链接点击。
// Issue #1053：原生段落断行策略统一入口；装饰只改绘制样式、不删改正文；
//   超链接命中改用原生 LayoutManager 字形偏移；恢复原生写作区运行时诊断快照。
// Issue #1054：
//   - RichEditorParagraphStyle 没有首行缩进 textIndent，改用
//     RichEditorStyledStringController + MutableStyledString；段落缩进由
//     ParagraphStyle.textIndent 表达（见 StyledParagraphIndent）。
//   - 属性字符串模式不支持组件级 onWillChange/onDidChange/aboutToIMEInput/onDidIMEInput，
//     用户编辑由 controller.onContentChanged 提供（见 NativeRichEditorBridge）。
//   - 样式重建只在初始打开、切章、段落结构变化、设置变化和装饰变化时发布；
//     不随每次按键/选区调用 setStyledString，避免全量重渲染和滚动跳变。
// 系统组件自行处理纵向滚动、长按选区、拖拽选择、手柄、粘贴、系统 IME 与无障碍。

import { NativeRichEditorBridge, NativeEditorStateEvent } from '../input/NativeRichEditorBridge'
import { EditorSessionCoordinator } from '../session/EditorSessionCoordinator'
import { MappedCorrectionSuggestion } from '../correction/CorrectionDtos'
import { AnnotationRangeDto } from '../../../corebridge/dto/AnnotationDtos'
import { TextOffsetMapper } from '../input/TextOffsetMapper'
import {
  StyledParagraphIndentOptions,
  buildParagraphIndentStyles,
  listParagraphRanges,
  resolveTextIndentVp
} from '../input/StyledParagraphIndent'
import { PlatformApiResolver } from '../../../platform/version/PlatformApiResolver'
import { LengthMetrics } from '@kit.ArkUI'
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
// 快照是内存态更新，150ms 合并一次即可；落盘仍由 Checkpoint/Worker 合并。
const EDITOR_SNAPSHOT_DEBOUNCE_MS = 150
// 中文标点排版使用系统正常断行 + 高质量换行策略，不能再用 BREAK_ALL 覆盖系统断行。
const EDITOR_EXPECTED_WORD_BREAK_LABEL = 'BREAK_WORD'
const EDITOR_EXPECTED_TEXT_ALIGN_LABEL = 'Start'
// compressLeadingPunctuation 自 API 23 起可用，punctuationOverflow 自 API 26 起可用。
const EDITOR_COMPRESS_LEADING_PUNCTUATION_MIN_API = 23
const EDITOR_PUNCTUATION_OVERFLOW_MIN_API = 26
// StyledString 的 BackgroundColorStyle 自 API 14 起可用；低版本只保留链接/纠错下划线。
const EDITOR_TEXT_BACKGROUND_MIN_API = 14
const EDITOR_TAG_BACKGROUND_COLOR = '#3498DB33'
const EDITOR_LINK_UNDERLINE_COLOR = '#3498DB'
const EDITOR_CORRECTION_UNDERLINE_COLOR = '#E74C3C'

interface DecorationRange {
  start: number
  end: number
}

// 原生选区（UTF-16；不可读时为 -1）。
interface NativeSelectionOffsets {
  start: number
  end: number
}

interface EditorSelectionSnapshot {
  start: number
  end: number
}

// 中文标点原生排版属性做 API 版本隔离。compressLeadingPunctuation（API 23+）与
// punctuationOverflow（API 26+）都是 RichEditorAttribute，用 AttributeModifier 按设备能力启用。
class RichEditorPunctuationModifier implements AttributeModifier<RichEditorAttribute> {
  // 记录原生属性调用的真实结果（无 readback，不伪称已读回）。
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
  // Issue #1054：与 Linux Qt 共用同一首行缩进设置语义。
  @Prop @Watch('onIndentSettingsChanged') autoIndentEnabled: boolean = true
  @Prop @Watch('onIndentSettingsChanged') autoIndentWidth: number = 2
  caretColor: ResourceColor = $r('sys.color.ohos_id_color_text_primary')
  private richEditorController: RichEditorStyledStringController = new RichEditorStyledStringController()
  // 原生组件 onReady 后才允许改属性字符串，避免对未绑定的 controller 操作。
  private editorReady: boolean = false
  private editorFocused: boolean = false
  private nativeEditing: boolean = false
  private editorWidthVp: number = 0
  private editorHeightVp: number = 0
  private punctuationModifier: RichEditorPunctuationModifier = new RichEditorPunctuationModifier()
  // 非热路径读取到的实际段落样式（诊断报告真实值）。
  private appliedWordBreakLabel: string = 'unknown'
  private appliedTextAlignLabel: string = 'unknown'
  private appliedLineBreakStrategyLabel: string = 'default'
  private paragraphStyleChecked: boolean = false
  private checkedParagraphCount: number = 0
  private mismatchedParagraphCount: number = 0
  private firstMismatchParagraphRange: string = ''
  private paragraphStyleApplied: boolean = false
  private paragraphStyleError: string | null = null
  private snapshotTimerId: number = -1
  // 段落结构变化后的样式重发合并到下一 tick，避免在 onDidChange 回调内重入组件。
  private paragraphRestyleTimer: number = -1
  // 正文样式全量发布次数（诊断用）。
  private stylePublishCount: number = 0

  aboutToDisappear(): void {
    this.cancelSnapshotTimer()
    if (this.paragraphRestyleTimer !== -1) {
      clearTimeout(this.paragraphRestyleTimer)
      this.paragraphRestyleTimer = -1
    }
    if (this.richEditorBridge) {
      this.richEditorBridge.captureDiagnosticsFacts()
    }
    this.publishRuntimeSnapshot()
    EditorRuntimeSnapshotStore.getInstance().markEditorUnmounted()
    if (this.richEditorBridge) {
      this.richEditorBridge.setStateListener(null)
      this.richEditorBridge.setDocumentBuilder(null)
      this.richEditorBridge.beginSession()
    }
    this.editorReady = false
  }

  // ── 正文属性字符串构建 ─────────────────────────────────────────

  // 唯一正文样式构建入口：字体、行高、段落首行缩进、标注背景、链接下划线、纠错波浪线。
  // 只读取当前文本与设置，不产生 Core 编辑事务。
  private buildDocument(text: string): MutableStyledString {
    const styles: StyleOptions[] = []
    if (text.length > 0) {
      styles.push({
        start: 0,
        length: text.length,
        styledKey: StyledStringKey.FONT,
        styledValue: new TextStyle({
          fontSize: LengthMetrics.fp(this.fontSize)
        })
      })
      styles.push({
        start: 0,
        length: text.length,
        styledKey: StyledStringKey.LINE_HEIGHT,
        styledValue: new LineHeightStyle(LengthMetrics.vp(this.fontSize * this.lineSpacingMultiplier))
      })
      const indentOptions: StyledParagraphIndentOptions = {
        fontSize: this.fontSize,
        autoIndentEnabled: this.autoIndentEnabled,
        autoIndentWidth: this.autoIndentWidth
      }
      const paragraphStyles: StyleOptions[] = buildParagraphIndentStyles(text, indentOptions)
      for (let i = 0; i < paragraphStyles.length; i++) {
        styles.push(paragraphStyles[i])
      }
      this.appendAnnotationStyles(styles, text)
      this.appendCorrectionStyles(styles, text)
    }
    return new MutableStyledString(text, styles)
  }

  // 标注只改绘制样式：tagBackground → 文本背景色；linkUnderline → 实线下划线。
  private appendAnnotationStyles(styles: StyleOptions[], text: string): void {
    if (this.annotationRanges.length === 0 || text.length === 0) {
      return
    }
    const allowBackground: boolean =
      PlatformApiResolver.getInstance().isApiAtLeast(EDITOR_TEXT_BACKGROUND_MIN_API)
    for (let i = 0; i < this.annotationRanges.length; i++) {
      const range = this.annotationRanges[i]
      const startUtf16 = TextOffsetMapper.utf8ToUtf16(text, range.startByte)
      const endUtf16 = TextOffsetMapper.utf8ToUtf16(text, range.endByte)
      const clipped = this.clipDecorationRange(startUtf16, endUtf16, text.length)
      if (clipped === null) {
        continue
      }
      if (range.kind === 'tagBackground' && allowBackground) {
        styles.push({
          start: clipped.start,
          length: clipped.end - clipped.start,
          styledKey: StyledStringKey.BACKGROUND_COLOR,
          styledValue: new BackgroundColorStyle({ color: EDITOR_TAG_BACKGROUND_COLOR })
        })
      } else if (range.kind === 'linkUnderline') {
        styles.push({
          start: clipped.start,
          length: clipped.end - clipped.start,
          styledKey: StyledStringKey.DECORATION,
          styledValue: new DecorationStyle({
            type: TextDecorationType.Underline,
            color: EDITOR_LINK_UNDERLINE_COLOR,
            style: TextDecorationStyle.SOLID
          })
        })
      }
    }
  }

  // 纠错波浪线最后追加：与链接下划线重叠时优先显示纠错样式。
  private appendCorrectionStyles(styles: StyleOptions[], text: string): void {
    for (let i = 0; i < this.correctionSuggestions.length; i++) {
      const suggestion = this.correctionSuggestions[i]
      const clipped = this.clipDecorationRange(suggestion.startUtf16, suggestion.endUtf16, text.length)
      if (clipped === null) {
        continue
      }
      styles.push({
        start: clipped.start,
        length: clipped.end - clipped.start,
        styledKey: StyledStringKey.DECORATION,
        styledValue: new DecorationStyle({
          type: TextDecorationType.Underline,
          color: EDITOR_CORRECTION_UNDERLINE_COLOR,
          style: TextDecorationStyle.WAVY
        })
      })
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

  // 纠错建议变化：只重发样式，不生成 Core 编辑事务。
  private onCorrectionSuggestionsChanged(): void {
    this.publishStyledDocument('correction')
  }

  // 标注范围变化：只重发样式，不删除/重建正文 Span。
  private onAnnotationRangesChanged(): void {
    this.publishStyledDocument('annotation')
  }

  // 字号/行距变化：重发字体与行高样式。
  private onFormatChanged(): void {
    this.publishStyledDocument('format')
  }

  // 首行缩进设置变化：重发段落样式。
  private onIndentSettingsChanged(): void {
    this.publishStyledDocument('indent')
  }

  // 全量发布正文属性字符串。只在非逐键事件（打开/切章/段落结构/设置/装饰）调用。
  // setStyledString 会全量重渲染并可能滚动到底部，因此必须恢复原有选区/光标。
  private publishStyledDocument(reason: string): void {
    if (!this.editorReady || this.richEditorBridge === null) {
      return
    }
    if (this.richEditorBridge.isImeComposing()) {
      // 预编辑期间不重发全文，等提交后由下一笔编辑/保存屏障收敛。
      hilog.info(EDITOR_LOG_DOMAIN, EDITOR_LOG_TAG, `style publish deferred during IME: ${reason}`)
      return
    }
    const text: string = this.richEditorBridge.getCurrentText()
    // 发布前核对原生实际全文：镜像落后时绝不用旧文本覆盖用户可见正文。
    const nativeText: string | null = this.richEditorBridge.getNativeFullText()
    if (nativeText === null || nativeText !== text) {
      hilog.warn(EDITOR_LOG_DOMAIN, EDITOR_LOG_TAG,
        `style publish skipped: mirror/native mismatch reason=${reason}`
          + ` mirrorLen=${text.length} nativeLen=${nativeText !== null ? nativeText.length : -1}`)
      return
    }
    const previousSelection = this.readSelectionSnapshot()
    this.richEditorBridge.setProgrammaticUpdate(true)
    try {
      this.richEditorController.setStyledString(this.buildDocument(text))
      this.restoreSelectionSnapshot(previousSelection, text.length)
      this.stylePublishCount++
      this.refreshPublishedStyleFacts(text)
    } catch (e) {
      this.paragraphStyleApplied = false
      this.paragraphStyleError = `setStyledString failed (${reason}): ${JSON.stringify(e)}`
      hilog.error(EDITOR_LOG_DOMAIN, EDITOR_LOG_TAG, this.paragraphStyleError)
    } finally {
      this.richEditorBridge.setProgrammaticUpdate(false)
    }
    this.publishRuntimeSnapshot()
  }

  private readSelectionSnapshot(): EditorSelectionSnapshot | null {
    try {
      const selection: RichEditorRange = this.richEditorController.getSelection()
      if (selection === undefined || selection === null) {
        return null
      }
      const start: number = selection.start ?? 0
      const end: number = selection.end ?? start
      return { start: start, end: end }
    } catch (e) {
      return null
    }
  }

  private restoreSelectionSnapshot(selection: EditorSelectionSnapshot | null, textLength: number): void {
    if (selection === null) {
      return
    }
    const start = Math.max(0, Math.min(selection.start, textLength))
    const end = Math.max(0, Math.min(selection.end, textLength))
    try {
      this.richEditorController.setSelection(start, end)
    } catch (e) {
      hilog.warn(EDITOR_LOG_DOMAIN, EDITOR_LOG_TAG, `restore selection failed: ${JSON.stringify(e)}`)
    }
  }

  // 段落样式读回：ParagraphStyle 可读 textIndent/textAlign/wordBreak。
  // 每个有内容的真实段落都必须拿到期望的 textIndent；空段落没有可缩进字符，不参与。
  private refreshPublishedStyleFacts(text: string): void {
    if (!this.editorReady) {
      return
    }
    try {
      const styled: MutableStyledString = this.richEditorController.getStyledString()
      if (styled === undefined || styled === null) {
        this.paragraphStyleChecked = false
        this.paragraphStyleApplied = false
        this.paragraphStyleError = 'styled string unavailable'
        return
      }
      const ranges = listParagraphRanges(text)
      const expectedIndent: number = resolveTextIndentVp({
        fontSize: this.fontSize,
        autoIndentEnabled: this.autoIndentEnabled,
        autoIndentWidth: this.autoIndentWidth
      })
      let checked = 0
      let mismatch = 0
      let firstRange = ''
      let firstSummary = ''
      for (let i = 0; i < ranges.length; i++) {
        const range = ranges[i]
        if (range.end <= range.start) {
          continue
        }
        checked++
        let matched = false
        let wordBreakLabel = 'unknown'
        let textAlignLabel = 'unknown'
        try {
          const spanStyles: SpanStyle[] = styled.getStyles(
            range.start, 1, StyledStringKey.PARAGRAPH_STYLE)
          for (let s = 0; s < spanStyles.length; s++) {
            const paragraphStyle = spanStyles[s].styledValue as ParagraphStyle
            if (paragraphStyle !== undefined && paragraphStyle !== null) {
              wordBreakLabel = this.wordBreakLabel(paragraphStyle.wordBreak)
              textAlignLabel = this.textAlignLabel(paragraphStyle.textAlign)
              const indent = paragraphStyle.textIndent
              if (indent !== undefined && Math.abs(indent - expectedIndent) < 0.01) {
                matched = true
              }
            }
          }
        } catch (e) {
          matched = false
        }
        if (i === 0) {
          this.appliedWordBreakLabel = wordBreakLabel
          this.appliedTextAlignLabel = textAlignLabel
          this.appliedLineBreakStrategyLabel = 'default'
        }
        if (!matched) {
          if (mismatch === 0) {
            firstRange = `[${range.start}, ${range.end})`
            firstSummary = `textIndent expected=${expectedIndent}`
          }
          mismatch++
        }
      }
      this.paragraphStyleChecked = true
      this.checkedParagraphCount = checked
      this.mismatchedParagraphCount = mismatch
      this.paragraphStyleApplied = checked > 0 && mismatch === 0
      this.firstMismatchParagraphRange = firstRange
      this.paragraphStyleError = mismatch === 0
        ? null
        : `${mismatch}/${checked} paragraph(s) missing textIndent firstRange=${firstRange} ${firstSummary}`
    } catch (e) {
      this.paragraphStyleChecked = false
      this.checkedParagraphCount = 0
      this.mismatchedParagraphCount = 0
      this.firstMismatchParagraphRange = ''
      this.paragraphStyleApplied = false
      this.paragraphStyleError = `getStyledString failed: ${JSON.stringify(e)}`
      hilog.warn(EDITOR_LOG_DOMAIN, EDITOR_LOG_TAG, this.paragraphStyleError)
    }
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

  // 原生编辑器状态变化：程序性同步与初始加载的正文样式已由桥接构建器写好；
  // 只有用户编辑新增/删除段落时才需要重新发布段落样式（仍不随普通输入重排全文）。
  private handleNativeEditorState(event: NativeEditorStateEvent): void {
    if (!this.editorReady) {
      return
    }
    if (event.paragraphsChanged &&
        (event.kind === 'native-change' || event.kind === 'ime-commit' || event.kind === 'edit-settled')) {
      this.scheduleParagraphRestyle()
    }
    this.scheduleRuntimeSnapshot()
  }

  // 段落结构变化后按下一 tick 重发样式：不在 onDidChange 回调内重入 setStyledString。
  private scheduleParagraphRestyle(): void {
    if (this.paragraphRestyleTimer !== -1) {
      return
    }
    this.paragraphRestyleTimer = setTimeout(() => {
      this.paragraphRestyleTimer = -1
      this.publishStyledDocument('paragraphs')
    }, 0)
  }

  // 超链接命中改用 RichEditor 自身点击事件 + 原生 LayoutManager 字形定位。
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

  private async initializeEditor(): Promise<void> {
    if (this.richEditorBridge) {
      await this.richEditorBridge.syncFromCore()
    }
    this.publishRuntimeSnapshot()
  }

  async syncFromCore(): Promise<void> {
    if (this.richEditorBridge) {
      await this.richEditorBridge.syncFromCore()
      this.refreshPublishedStyleFacts(this.richEditorBridge.getCurrentText())
      this.publishRuntimeSnapshot()
    }
  }

  async loadInitialText(text: string, cursorByteOffset: number): Promise<void> {
    if (this.richEditorBridge) {
      await this.richEditorBridge.loadInitialText(text, cursorByteOffset)
      this.refreshPublishedStyleFacts(text)
      this.publishRuntimeSnapshot()
    }
  }

  // ── 原生写作区运行时快照（不含正文原文） ──────────────────────

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
    const alignment = bridge !== null ? bridge.getLastNativeCoreAlignment() : null
    const nativeNewlineCount = bridge !== null ? bridge.getNativeNewlineCount() : 0
    const coreNewlineCount = bridge !== null ? bridge.getCoreNewlineCount() : 0
    const nativeSpanCount = bridge !== null ? bridge.getNativeSpanCount() : -1
    const caretUtf16 = this.readCaretUtf16()
    const selection = this.readNativeSelection()
    const editorWidthPx = this.vpToPx(this.editorWidthVp)
    const editorHeightPx = this.vpToPx(this.editorHeightVp)
    const lastOperation = bridge !== null ? bridge.getLastNativeOperation() : null

    const resolver = PlatformApiResolver.getInstance()
    const snapshot: RichEditorRuntimeSnapshot = {
      // Issue #1054：属性字符串模式，Span 级事实不再可读。
      schemaVersion: 5,
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
      typingParagraphStyleSupported: false,
      typingParagraphStyleApplied: false,
      typingParagraphStyleError: 'styled-string-mode',
      deviceApiLevel: resolver.getApiLevel(),
      fontSizeVp: this.fontSize,
      lineSpacingMultiplier: this.lineSpacingMultiplier,
      effectiveLineHeightVp: this.fontSize * this.lineSpacingMultiplier,
      editorFocused: this.editorFocused,
      editing: this.nativeEditing,
      imeComposing: bridge !== null ? bridge.isImeComposing() : false,
      previewTextUtf16Length: this.readPreviewTextLength(),
      lastNativeOperation: lastOperation,
      lastNativeCoreAlignment: alignment,
      nativeFactError: bridge !== null ? bridge.getNativeFactError() : null,
      saveBaselineHash: lastOperation !== null ? lastOperation.saveBaselineHash : '',
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
      const selection: RichEditorRange = this.richEditorController.getSelection()
      if (selection === undefined || selection === null) {
        return { start: -1, end: -1 }
      }
      return { start: selection.start ?? 0, end: selection.end ?? selection.start ?? 0 }
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
    Stack() {
      RichEditor({ controller: this.richEditorController })
        .width('100%')
        .height('100%')
        .padding(0)
        .attributeModifier(this.punctuationModifier)
        .barState(BarState.Auto)
        .caretColor(this.caretColor)
        .onReady(() => {
          hilog.info(EDITOR_LOG_DOMAIN, EDITOR_LOG_TAG, 'RichEditor onReady (styled string)')
          if (this.richEditorBridge) {
            this.richEditorBridge.setController(this.richEditorController)
            this.richEditorBridge.setDocumentBuilder(
              (text: string): MutableStyledString => this.buildDocument(text))
            this.richEditorBridge.setStateListener((event: NativeEditorStateEvent): void => {
              this.handleNativeEditorState(event)
            })
            // 属性字符串模式不支持组件级 onWillChange/onDidChange/aboutToIMEInput/onDidIMEInput；
            // 用户编辑统一由 controller.onContentChanged 提供。
            this.richEditorController.onContentChanged({
              onWillChange: (value: StyledStringChangeValue): boolean =>
                this.richEditorBridge !== null ? this.richEditorBridge.onContentWillChange(value) : true,
              onDidChange: (rangeBefore: TextRange, rangeAfter: TextRange): void => {
                if (this.richEditorBridge !== null) {
                  this.richEditorBridge.onContentDidChange(rangeBefore, rangeAfter)
                }
                this.scheduleRuntimeSnapshot()
              }
            })
            this.editorReady = true
            this.initializeEditor()
          }
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
