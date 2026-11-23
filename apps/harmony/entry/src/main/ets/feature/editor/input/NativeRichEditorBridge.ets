// NativeRichEditorBridge — RichEditor（属性字符串模式）与 EditorSessionCoordinator 之间的输入桥接。
//
// Issue #1034 评论 #5/#6/#7/#8/#10/#11/#12/#13：原生 RichEditor 替换自研编辑器，
// 用户输入按原生变更范围重建差量、串行提交 Core。
// Issue #1053：程序性正文替换与用户编辑严格区分，保存/退出屏障做原生全文与 Core 的三方校验。
// Issue #1054：
//   - 改用 RichEditorStyledStringController + MutableStyledString：段落首行缩进需要
//     ParagraphStyle.textIndent，旧 RichEditorParagraphStyle 只有 leadingMargin。
//   - 属性字符串模式的组件不支持 onWillChange/onDidChange/aboutToIMEInput/onDidIMEInput，
//     用户编辑统一由 controller.onContentChanged 提供替换范围与替换属性字符串；
//     程序性 setStyledString 不会触发该监听，因此格式化重建不会再被误判成用户输入。
//   - 程序性正文重建统一走 applyProgrammaticText()，绝不进入用户编辑队列。
//   - onContentChanged.onWillChange 记录 changeSequence、范围、来源、镜像前后 hash；
//     只有确实发生的用户删除事务才提交 Core。
//   - 编辑事务把非空正文删成空时，记录精确 sessionId/revision 的清空意图，供保存路径
//     决定是否允许空覆盖；程序性初始空视图、卸载、切章不会产生该意图。
//
// 职责：
// - controller.onContentChanged.onWillChange → 记录同一笔原生变更事实（范围 + 替换文本）。
// - onContentChanged.onDidChange → 从前后文本差量重建编辑事实并串行提交 Core。
// - IME 预编辑（previewText）期间只更新本地镜像，结算后按基线差量提交一次。
// - whenIdle 为保存屏障：等已提交队列稳定，并校验原生全文、镜像与 Core 正文一致。
// - finishEditingAndWhenIdle 为退出/同步屏障：先 stopEditing() 结束预编辑，再等队列稳定。
// - syncFromCore/loadInitialText 通过统一程序性入口 setStyledString，保留正文样式由组件构建。
// - 切章时以 sessionId 隔离旧回调，旧队列不可触碰新章节 UI。

import { EditorSessionCoordinator } from '../session/EditorSessionCoordinator'
import { computeMinimalTextDiff } from '../session/editor_patch_logic'
import { TextOffsetMapper } from './TextOffsetMapper'
import { EditorTransactionCause, EditorEditResult, EditorSessionSnapshot, EditorEditOutcome } from '../../../corebridge/dto/EditorDtos'
import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'
import {
  NativeTextOperationSnapshot,
  NativeCoreAlignmentSnapshot,
  countNewlines,
  hashTextUtf16,
  firstMismatchUtf16
} from '../diagnostics/EditorRuntimeSnapshotStore'
import hilog from '@ohos.hilog'

const BRIDGE_LOG_DOMAIN = 0x0000
const BRIDGE_LOG_TAG = 'NativeRichEditorBridge'

// Issue #1053：原生编辑器状态事件。paragraphsChanged 只在段落结构真正变化时置 true，
// 组件据此重新发布正文段落样式；纯选区/光标变化不会触发全文重新排版。
export interface NativeEditorStateEvent {
  kind: string
  paragraphsChanged: boolean
}

export type NativeEditorStateListener = (event: NativeEditorStateEvent) => void

// Issue #1054：正文样式构建回调。桥接只负责文本，段落/字号/行距/装饰样式由组件构建。
export type EditorStyledDocumentBuilder = (text: string) => MutableStyledString

// Issue #1053 评论 7098652618：保存/退出屏障的三方校验结果。
// 放行条件是「原生组件实际全文 === 本地镜像 === Core snapshot.text」，
// 任何一方不一致都不能 resolve 等待者。
export interface NativeCoreAlignmentResult {
  stage: string
  matched: boolean
  coreAvailable: boolean
  nativeAvailable: boolean
  mirrorMatchesEditor: boolean
  nativeText: string
  nativeUtf16Length: number
  coreUtf16Length: number
  nativeSpanCount: number
  nativeRevision: number
  firstMismatchUtf16: number
  mismatchToCoreUtf16: number
  appliedRevision: number
  coreRevision: number
  coreAdvanced: boolean
  detail: string
}

// Issue #1054：合法的“用户删除至空”意图。只记录身份，不保存正文。
export interface EmptyDocumentIntent {
  sessionId: number
  revision: number
}

interface TextDiff {
  start: number
  end: number
  replacement: string
}

interface EditRangeMap {
  startUtf16: number
  endUtf16: number
}

interface NativeSelectionState {
  start: number
  end: number
  caret: number
}

// 一次原生 Range 文本提取的事实（不含正文原文记录，仅用于本次差量重建）。
interface NativeRangeText {
  text: string
  spanCount: number
  nonTextSpanCount: number
  startUtf16: number
  endUtf16: number
}

// Issue #1054：同一笔原生变更的事实。onWillChange 记录，onDidChange 消费。
interface PendingNativeChange {
  rangeStart: number
  rangeEnd: number
  insertedText: string
  preview: boolean
  changeSequence: number
}

// 记录一次原生文本操作所需的字段（补 hash/会话事实后落到快照）。
interface NativeOperationFacts {
  kind: string
  source: string
  beforeText: string
  afterText: string
  rangeBeforeStartUtf16: number
  rangeBeforeEndUtf16: number
  rangeAfterStartUtf16: number
  rangeAfterEndUtf16: number
  startUtf16: number
  endUtf16: number
  replacedUtf16Length: number
  insertedUtf16Length: number
  spanCountInRange: number
  nonTextSpanCount: number
  imeComposition: boolean
  programmatic: boolean
  factError: boolean
}

interface PendingEdit {
  sessionId: number
  localSequence: number
  baseRevision: number
  beforeText: string
  startUtf16: number
  endUtf16: number
  insertedText: string
  oldText: string
  cause: EditorTransactionCause
}

interface IdleWaiter {
  sessionId: number
  resolve: () => void
  reject: (e: Error) => void
}

interface SelectionIntent {
  localSessionToken: number
  expectedCoreSessionId: number
  startUtf16: number
  endUtf16: number
  textForConversion: string
  retriedAtRevision: number | null
}

export class NativeRichEditorBridge {
  private coordinator: EditorSessionCoordinator
  private richEditorController: RichEditorStyledStringController | null = null
  // 组件提供的正文样式构建器；文本写入必须同时带样式，避免 setStyledString 丢样式。
  private documentBuilder: EditorStyledDocumentBuilder | null = null
  private currentText: string = ''
  private programmaticUpdate: boolean = false
  private lastAppliedRevision: number = -1
  private editQueue: Promise<void> = Promise.resolve()
  private pendingPaste: boolean = false
  private pendingEdits: PendingEdit[] = []
  private imeComposing: boolean = false
  private imeComposingStartText: string = ''
  private imeComposingStartNewlineCount: number = 0
  private drainRunning: boolean = false
  private sessionId: number = 0
  private localSequence: number = 0
  private confirmedText: string = ''
  private confirmedRevision: number = -1
  private idleError: string | null = null
  private idleWaiters: IdleWaiter[] = []
  // 选区 latest-wins 合并上报
  private pendingSelectionIntent: SelectionIntent | null = null
  private selectionFlushTimerId: number = -1
  private selectionDrainRunning: boolean = false

  // 原生编辑事实与运行时诊断。只记录范围/长度/hash，不保存正文原文。
  private lastNativeOperation: NativeTextOperationSnapshot | null = null
  private lastNativeCoreAlignment: NativeCoreAlignmentSnapshot | null = null
  private nativeFactError: string | null = null
  // 屏障发现「原生正文有效但与 Core 不一致」：阻止 syncFromCore 异步回写覆盖原生内容。
  private nativeCoreDiverged: boolean = false
  private nativeCoreDivergedSessionId: number | null = null
  private stateListener: NativeEditorStateListener | null = null
  private barrierStage: string = 'save-when-idle'
  private nativeCoreConvergenceRunning: boolean = false
  private static readonly NATIVE_CORE_CONVERGENCE_MAX_ATTEMPTS: number = 3
  // 原生编辑器是否已加载过 Core 正文。未加载时 currentText 只是空占位，
  // 不能把它当成「原生正文与 Core 不一致」去收敛，否则会把 Core 正文清空。
  private nativeTextReady: boolean = false
  private nativeCoreRefreshRunning: boolean = false
  // 换行数按编辑增量维护，快照不再每 150ms 遍历整章。
  private nativeNewlineCount: number = 0
  private coreNewlineCount: number = 0
  private lastReadNativeSpanCount: number = -1

  // Issue #1054：同一笔原生变更事实与递增序号。
  private pendingNativeChange: PendingNativeChange | null = null
  private changeSequence: number = 0
  private lastWillRangeStart: number = -1
  private lastWillRangeEnd: number = -1
  // 最近一次成功保存的磁盘基线 hash（只用于诊断，不参与保存判定）。
  private saveBaselineHash: string = ''
  // 用户把非空 Core 正文删除到空时记录；保存路径据此决定是否允许空覆盖。
  private emptyDocumentIntent: EmptyDocumentIntent | null = null

  constructor(coordinator: EditorSessionCoordinator) {
    this.coordinator = coordinator
  }

  setController(controller: RichEditorStyledStringController): void {
    this.richEditorController = controller
  }

  // 组件设置正文样式构建器（字号/行距/段落缩进/装饰）。同步写入与初始加载都走它。
  setDocumentBuilder(builder: EditorStyledDocumentBuilder | null): void {
    this.documentBuilder = builder
  }

  setStateListener(listener: NativeEditorStateListener | null): void {
    this.stateListener = listener
  }

  isProgrammaticUpdate(): boolean {
    return this.programmaticUpdate
  }

  setProgrammaticUpdate(value: boolean): void {
    this.programmaticUpdate = value
  }

  getCurrentText(): string {
    return this.currentText
  }

  getLastAppliedRevision(): number {
    return this.lastAppliedRevision
  }

  isImeComposing(): boolean {
    return this.imeComposing
  }

  getLastNativeOperation(): NativeTextOperationSnapshot | null {
    return this.lastNativeOperation
  }

  getLastNativeCoreAlignment(): NativeCoreAlignmentSnapshot | null {
    return this.lastNativeCoreAlignment
  }

  getNativeFactError(): string | null {
    return this.nativeFactError
  }

  getNativeNewlineCount(): number {
    return this.nativeNewlineCount
  }

  getCoreNewlineCount(): number {
    return this.coreNewlineCount
  }

  getNativeSpanCount(): number {
    return this.lastReadNativeSpanCount
  }

  getEmptyDocumentIntent(): EmptyDocumentIntent | null {
    return this.emptyDocumentIntent
  }

  // 保存路径复核：目标为空且 session/revision 与用户清空事务完全一致才允许空覆盖。
  isLegitimateEmptyDocument(sessionId: number, revision: number): boolean {
    const intent = this.emptyDocumentIntent
    return intent !== null && intent.sessionId === sessionId && intent.revision === revision
  }

  // 保存成功后由页面回填磁盘基线 hash（仅诊断用）。
  setSaveBaselineHash(hash: string | null): void {
    this.saveBaselineHash = hash !== null ? hash : ''
  }

  // 非热路径：读取原生编辑器当前全文（只供样式发布前校验，不记录正文）。
  getNativeFullText(): string | null {
    return this.readNativeFullText()
  }

  // 非热路径：在失焦/退出/导出前刷新一次完整事实（全文 hash 与三方对齐）。
  captureDiagnosticsFacts(): void {
    this.validateNativeCoreAlignment('diagnostics')
  }

  isNativeCoreDiverged(): boolean {
    return this.nativeCoreDiverged
  }

  // 普通保存屏障：只等已提交队列事务，不调用 stopEditing()。
  whenIdle(): Promise<void> {
    this.barrierStage = 'save-when-idle'
    // 普通保存不强制结束预编辑：只有组件已无预览文本时结算，否则等待用户完成输入。
    this.settleImeComposition(false)
    this.requestSelectionFlush()
    return new Promise<void>((resolve, reject) => {
      this.idleWaiters.push({ sessionId: this.sessionId, resolve: resolve, reject: reject })
      this.checkIdle()
    })
  }

  // 退出章节/同步屏障：先 stopEditing() 结束预编辑，再等队列稳定。
  finishEditingAndWhenIdle(): Promise<void> {
    this.barrierStage = 'exit-finish-editing'
    if (this.richEditorController !== null) {
      try {
        this.richEditorController.stopEditing()
      } catch (e) {
        hilog.warn(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, `stopEditing failed: ${JSON.stringify(e)}`)
      }
    }
    // stopEditing() 之后仍可能残留预编辑镜像，强制按基线结算。
    this.settleImeComposition(true)
    this.requestSelectionFlush()
    return new Promise<void>((resolve, reject) => {
      this.idleWaiters.push({ sessionId: this.sessionId, resolve: resolve, reject: reject })
      this.checkIdle()
    })
  }

  // 事件驱动检查：先查 idleError，再等 editQueue tail，条件不满足时等待状态变化通知。
  private checkIdle(): void {
    if (this.idleWaiters.length === 0) {
      return
    }

    if (this.idleError !== null) {
      const error = new Error(this.idleError)
      const waiters = this.idleWaiters
      this.idleWaiters = []
      for (const w of waiters) {
        w.reject(error)
      }
      return
    }

    const currentTail = this.editQueue
    currentTail.then(() => {
      if (this.idleWaiters.length === 0) {
        return
      }

      if (this.editQueue !== currentTail) {
        this.checkIdle()
        return
      }

      if (this.idleError !== null) {
        const error = new Error(this.idleError)
        const waiters = this.idleWaiters
        this.idleWaiters = []
        for (const w of waiters) {
          w.reject(error)
        }
        return
      }

      if (this.pendingEdits.length > 0 || this.drainRunning) {
        return
      }

      if (this.imeComposing) {
        // 预编辑尚未结算：等 onDidChange/stopEditing 或下一次屏障再次唤醒。
        return
      }

      if (this.pendingSelectionIntent !== null) {
        if (this.pendingEdits.length > 0 || this.drainRunning) {
          return
        }
        if (this.selectionDrainRunning) {
          return
        }
        const snap = this.coordinator.getSnapshot()
        if (snap !== null && snap.text !== this.currentText) {
          this.pendingSelectionIntent = null
        } else {
          this.requestSelectionFlush()
          return
        }
      }

      const currentSession = this.sessionId
      const currentWaiters: IdleWaiter[] = []
      const remaining: IdleWaiter[] = []
      for (const w of this.idleWaiters) {
        if (w.sessionId === currentSession) {
          currentWaiters.push(w)
        } else {
          remaining.push(w)
        }
      }
      if (currentWaiters.length === 0) {
        this.idleWaiters = remaining
        return
      }

      // 只有原生全文、本地镜像、Core 正文三方一致才放行。
      const alignment = this.validateNativeCoreAlignment(this.barrierStage)
      const nativeEditorLoaded = this.richEditorController !== null && this.nativeTextReady
      if (alignment.matched || !alignment.coreAvailable || !nativeEditorLoaded) {
        this.idleWaiters = remaining
        for (const w of currentWaiters) {
          w.resolve()
        }
        return
      }

      if (alignment.coreAdvanced) {
        hilog.warn(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG,
          `core text advanced beyond native mirror: appliedRevision=${alignment.appliedRevision}`
            + ` coreRevision=${alignment.coreRevision}`)
        this.idleWaiters = remaining.concat(currentWaiters)
        if (this.nativeCoreRefreshRunning) {
          return
        }
        this.nativeCoreRefreshRunning = true
        void this.refreshNativeMirrorFromCore()
        return
      }

      // 未对齐：等待者保持挂起，先用原生全文收敛 Core；收敛失败才向等待者返回真实错误。
      this.idleWaiters = remaining.concat(currentWaiters)
      if (this.nativeCoreConvergenceRunning) {
        return
      }
      if (!alignment.nativeAvailable) {
        this.failNativeCoreAlignment(
          `native full text unavailable; mirrorLen=${alignment.nativeUtf16Length} coreLen=${alignment.coreUtf16Length}`)
        return
      }
      if (!alignment.mirrorMatchesEditor) {
        this.reportNativeFactError('barrier',
          `mirror diverged from native text firstMismatch=${alignment.firstMismatchUtf16}`)
        this.currentText = alignment.nativeText
        this.nativeNewlineCount = countNewlines(alignment.nativeText)
      }
      void this.convergeNativeTextToCore()
    })
  }

  // Core 先行推进时刷新原生正文，刷新后由 checkIdle 重新做三方校验。
  private async refreshNativeMirrorFromCore(): Promise<void> {
    const sessionAtEntry = this.sessionId
    try {
      this.nativeCoreDiverged = false
      this.nativeCoreDivergedSessionId = null
      await this.syncFromCore()
      if (sessionAtEntry !== this.sessionId) {
        return
      }
      const snap = this.coordinator.getSnapshot()
      const editorText = this.readNativeFullText()
      if (snap === null || editorText === null) {
        this.failNativeCoreAlignment('cannot refresh native mirror from core')
        return
      }
      if (editorText !== snap.text) {
        this.failNativeCoreAlignment(
          `native editor did not accept core text after refresh nativeLen=${editorText.length} coreLen=${snap.text.length}`)
        return
      }
      this.currentText = snap.text
      this.nativeNewlineCount = countNewlines(snap.text)
      this.coreNewlineCount = this.nativeNewlineCount
      this.nativeTextReady = true
      this.lastAppliedRevision = snap.revision
      this.confirmedText = snap.text
      this.confirmedRevision = snap.revision
    } catch (e) {
      this.failNativeCoreAlignment(`refresh native mirror error: ${JSON.stringify(e)}`)
    } finally {
      this.nativeCoreRefreshRunning = false
      if (sessionAtEntry === this.sessionId) {
        this.checkIdle()
      }
    }
  }

  // 原生正文 → Core 已提交正文的收敛事务：按最小差量只提交实际改变区间。
  private async convergeNativeTextToCore(): Promise<void> {
    if (this.nativeCoreConvergenceRunning) {
      return
    }
    this.nativeCoreConvergenceRunning = true
    const sessionAtEntry = this.sessionId
    let attempts = 0
    try {
      while (attempts < NativeRichEditorBridge.NATIVE_CORE_CONVERGENCE_MAX_ATTEMPTS) {
        if (sessionAtEntry !== this.sessionId) {
          return
        }
        if (this.pendingEdits.length > 0 || this.drainRunning || this.imeComposing) {
          return
        }
        const coreSessionId = this.coordinator.getSessionId()
        const snap = this.coordinator.getSnapshot()
        if (coreSessionId === null || snap === null) {
          this.failNativeCoreAlignment('no active core session during convergence')
          return
        }
        const editorText = this.readNativeFullText()
        if (editorText === null) {
          this.failNativeCoreAlignment('native full text unavailable during convergence')
          return
        }
        if (editorText !== this.currentText) {
          this.reportNativeFactError('converge',
            `mirror behind native text firstMismatch=${firstMismatchUtf16(this.currentText, editorText)}`)
          this.currentText = editorText
          this.nativeNewlineCount = countNewlines(editorText)
        }
        if (this.lastAppliedRevision !== snap.revision) {
          return
        }
        if (snap.text === this.currentText) {
          this.lastAppliedRevision = snap.revision
          this.confirmedText = snap.text
          this.confirmedRevision = snap.revision
          this.coreNewlineCount = countNewlines(snap.text)
          this.nativeCoreDiverged = false
          this.nativeCoreDivergedSessionId = null
          this.refreshSelectionAfterConvergence()
          return
        }
        const diff = this.computeDiff(snap.text, this.currentText)
        if (diff.start === diff.end && diff.replacement.length === 0) {
          return
        }
        const byteStart = TextOffsetMapper.utf16ToUtf8Checked(snap.text, diff.start)
        const byteEnd = TextOffsetMapper.utf16ToUtf8Checked(snap.text, diff.end)
        if (byteStart === null || byteEnd === null) {
          this.failNativeCoreAlignment(
            `invalid convergence offsets start=${diff.start} end=${diff.end} coreLen=${snap.text.length}`)
          return
        }
        attempts++
        const result = await this.coordinator.replaceSliceIfUnchanged(
          coreSessionId, snap.revision, snap.text, this.currentText, EditorTransactionCause.Programmatic)
        if (sessionAtEntry !== this.sessionId) {
          return
        }
        if (result.success && result.data !== undefined) {
          const after = this.coordinator.getSnapshot()
          if (after !== null && after.text === this.currentText) {
            this.lastAppliedRevision = after.revision
            this.confirmedText = after.text
            this.confirmedRevision = after.revision
            this.coreNewlineCount = countNewlines(after.text)
            this.nativeCoreDiverged = false
            this.nativeCoreDivergedSessionId = null
            this.refreshSelectionAfterConvergence()
            return
          }
          continue
        }
        if (result.errorCode === 'SESSION_CHANGED') {
          return
        }
      }
      this.failNativeCoreAlignment(
        `native text did not converge with core after ${attempts} attempt(s)`)
    } catch (e) {
      this.failNativeCoreAlignment(`convergence error: ${JSON.stringify(e)}`)
    } finally {
      this.nativeCoreConvergenceRunning = false
      if (sessionAtEntry === this.sessionId) {
        this.checkIdle()
      }
    }
  }

  private failNativeCoreAlignment(detail: string): void {
    const message = `Native/core text alignment failed stage=${this.barrierStage}: ${detail}`
    hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, message)
    const rejected: IdleWaiter[] = []
    const remaining: IdleWaiter[] = []
    for (const w of this.idleWaiters) {
      if (w.sessionId === this.sessionId) {
        rejected.push(w)
      } else {
        remaining.push(w)
      }
    }
    this.idleWaiters = remaining
    const error = new Error(message)
    for (const w of rejected) {
      w.reject(error)
    }
  }

  setPendingPaste(): void {
    this.pendingPaste = true
  }

  // 切章时调用：增加 sessionId 隔离旧回调，reject 旧 session 等待者，清空队列和错误状态。
  beginSession(): void {
    const oldSession = this.sessionId
    this.sessionId++

    const error = new Error(`Session ${oldSession} cancelled by beginSession`)
    const remaining: IdleWaiter[] = []
    for (const w of this.idleWaiters) {
      if (w.sessionId === oldSession) {
        w.reject(error)
      } else {
        remaining.push(w)
      }
    }
    this.idleWaiters = remaining

    this.pendingEdits = []
    this.idleError = null
    this.imeComposing = false
    this.imeComposingStartText = ''
    this.imeComposingStartNewlineCount = 0
    this.drainRunning = false
    this.localSequence = 0
    if (this.selectionFlushTimerId !== -1) {
      clearTimeout(this.selectionFlushTimerId)
      this.selectionFlushTimerId = -1
    }
    this.pendingSelectionIntent = null
    this.selectionDrainRunning = false
    this.lastNativeOperation = null
    this.lastNativeCoreAlignment = null
    this.nativeFactError = null
    this.nativeCoreDiverged = false
    this.nativeCoreDivergedSessionId = null
    this.nativeCoreConvergenceRunning = false
    this.nativeCoreRefreshRunning = false
    this.nativeTextReady = false
    this.confirmedText = ''
    this.confirmedRevision = -1
    this.lastAppliedRevision = -1
    this.nativeNewlineCount = 0
    this.coreNewlineCount = 0
    this.lastReadNativeSpanCount = -1
    this.pendingNativeChange = null
    this.changeSequence = 0
    this.lastWillRangeStart = -1
    this.lastWillRangeEnd = -1
    this.emptyDocumentIntent = null
  }

  // ── 属性字符串内容变化（用户编辑唯一入口） ──────────────────────

  // onContentChanged.onWillChange：记录同一笔原生变更的范围和替换内容。
  // 不在这里读取 getStyledString()：该接口会深拷贝全文，放进逐键回调会堆积临时对象。
  onContentWillChange(value: StyledStringChangeValue): boolean {
    if (this.richEditorController === null || this.programmaticUpdate) {
      this.pendingNativeChange = null
      return true
    }
    const rangeStart: number = value.range.start ?? 0
    const rangeEnd: number = value.range.end ?? rangeStart
    const insertedText: string = value.replacementString !== undefined
      ? value.replacementString.getString() : ''
    // previewText 只有携带非空预览内容时才代表未提交的预编辑。
    let preview: boolean = false
    if (value.previewText !== undefined && value.previewText !== null) {
      try {
        preview = value.previewText.getString().length > 0
      } catch (e) {
        preview = true
      }
    }
    this.changeSequence++
    this.lastWillRangeStart = rangeStart
    this.lastWillRangeEnd = rangeEnd
    this.pendingNativeChange = {
      rangeStart: rangeStart,
      rangeEnd: rangeEnd,
      insertedText: insertedText,
      preview: preview,
      changeSequence: this.changeSequence
    }
    return true
  }

  // onContentChanged.onDidChange：内容已改变后，按 onWillChange 记录的替换事实重建差量。
  // 属性字符串模式下没有 Span 级回调，替换文本来自 replacementString；
  // 范围事实不成立时才回退到原生全文差量（非热路径）。
  onContentDidChange(rangeBefore: TextRange, rangeAfter: TextRange): void {
    if (this.programmaticUpdate || this.richEditorController === null) {
      return
    }
    // 会话加载完成之前不产生编辑差量。
    if (!this.nativeTextReady) {
      return
    }

    const pending = this.pendingNativeChange
    this.pendingNativeChange = null
    const previousText = this.currentText
    let startUtf16: number = 0
    let endUtf16: number = 0
    let insertedText: string = ''
    let factError: boolean = false

    if (pending !== null && pending.rangeStart >= 0 && pending.rangeEnd >= pending.rangeStart &&
        pending.rangeEnd <= previousText.length) {
      startUtf16 = pending.rangeStart
      endUtf16 = pending.rangeEnd
      insertedText = pending.insertedText
      const declaredAfterLength: number = (rangeAfter.end ?? 0) - (rangeAfter.start ?? 0)
      if (insertedText.length > 0 && declaredAfterLength !== insertedText.length) {
        factError = true
        this.reportNativeFactError('onContentDidChange',
          `rangeAfter declares ${declaredAfterLength} but replacement has ${insertedText.length}`)
      }
    } else {
      // 缺少同一笔 onWillChange 事实：以原生全文为真相恢复（非热路径）。
      const nativeFullText = this.readNativeFullText()
      if (nativeFullText === null) {
        this.reportNativeFactError('onContentDidChange', 'native full text unavailable')
        this.idleError = 'Native text fact mismatch and native full text unavailable'
        this.rejectAllWaiters()
        return
      }
      const recovered = this.computeDiff(previousText, nativeFullText)
      startUtf16 = recovered.start
      endUtf16 = recovered.end
      insertedText = recovered.replacement
      factError = true
      this.reportNativeFactError('onContentDidChange',
        `missing will-change facts; recovered start=${startUtf16} end=${endUtf16} insertedLen=${insertedText.length}`)
    }

    const oldText: string = previousText.substring(startUtf16, endUtf16)
    this.currentText = previousText.substring(0, startUtf16)
      + insertedText
      + previousText.substring(endUtf16)
    this.nativeNewlineCount = Math.max(0,
      this.nativeNewlineCount + countNewlines(insertedText) - countNewlines(oldText))
    // 空文档第一次有字时需要重新发布段落/字体样式：空属性字符串没有可承载样式的位置。
    const becameNonEmpty: boolean = previousText.length === 0 && this.currentText.length > 0
    const paragraphsChanged: boolean = becameNonEmpty ||
      insertedText.indexOf('\n') >= 0 || oldText.indexOf('\n') >= 0

    if (pending !== null && pending.preview) {
      if (!this.imeComposing) {
        this.imeComposing = true
        this.imeComposingStartText = previousText
        this.imeComposingStartNewlineCount = this.nativeNewlineCount
          - countNewlines(insertedText) + countNewlines(oldText)
      }
      this.recordNativeOperation({
        kind: 'typing',
        source: 'ime',
        beforeText: previousText,
        afterText: this.currentText,
        rangeBeforeStartUtf16: rangeBefore.start ?? -1,
        rangeBeforeEndUtf16: rangeBefore.end ?? -1,
        rangeAfterStartUtf16: rangeAfter.start ?? -1,
        rangeAfterEndUtf16: rangeAfter.end ?? -1,
        startUtf16: startUtf16,
        endUtf16: endUtf16,
        replacedUtf16Length: endUtf16 - startUtf16,
        insertedUtf16Length: insertedText.length,
        spanCountInRange: -1,
        nonTextSpanCount: 0,
        imeComposition: true,
        programmatic: false,
        factError: factError
      })
      this.notifyStateListener('native-change', paragraphsChanged)
      return
    }

    if (this.imeComposing) {
      // 预编辑结束：从预编辑基线到当前镜像提交一笔 TypingCommit，不提交中间态。
      this.commitImeCompositionFromBaseline('ime-commit')
      this.notifyStateListener('ime-commit', paragraphsChanged)
      return
    }

    if (startUtf16 >= endUtf16 && insertedText.length === 0) {
      this.pendingPaste = false
      this.recordNativeOperation({
        kind: 'typing',
        source: 'user',
        beforeText: previousText,
        afterText: this.currentText,
        rangeBeforeStartUtf16: rangeBefore.start ?? -1,
        rangeBeforeEndUtf16: rangeBefore.end ?? -1,
        rangeAfterStartUtf16: rangeAfter.start ?? -1,
        rangeAfterEndUtf16: rangeAfter.end ?? -1,
        startUtf16: startUtf16,
        endUtf16: endUtf16,
        replacedUtf16Length: 0,
        insertedUtf16Length: 0,
        spanCountInRange: -1,
        nonTextSpanCount: 0,
        imeComposition: false,
        programmatic: false,
        factError: factError
      })
      this.notifyStateListener('native-change', paragraphsChanged)
      return
    }

    const cause: EditorTransactionCause = this.pendingPaste
      ? EditorTransactionCause.Paste : EditorTransactionCause.Typing
    this.pendingPaste = false

    this.recordNativeOperation({
      kind: cause === EditorTransactionCause.Paste ? 'paste' : 'typing',
      source: 'user',
      beforeText: previousText,
      afterText: this.currentText,
      rangeBeforeStartUtf16: rangeBefore.start ?? -1,
      rangeBeforeEndUtf16: rangeBefore.end ?? -1,
      rangeAfterStartUtf16: rangeAfter.start ?? -1,
      rangeAfterEndUtf16: rangeAfter.end ?? -1,
      startUtf16: startUtf16,
      endUtf16: endUtf16,
      replacedUtf16Length: endUtf16 - startUtf16,
      insertedUtf16Length: insertedText.length,
      spanCountInRange: -1,
      nonTextSpanCount: 0,
      imeComposition: false,
      programmatic: false,
      factError: factError
    })

    const pendingEdit: PendingEdit = {
      sessionId: this.sessionId,
      localSequence: this.localSequence++,
      baseRevision: this.lastAppliedRevision,
      beforeText: previousText,
      startUtf16: startUtf16,
      endUtf16: endUtf16,
      insertedText: insertedText,
      oldText: oldText,
      cause: cause
    }
    this.enqueuePendingEdit(pendingEdit)
    this.notifyStateListener('native-change', paragraphsChanged)
  }

  // 预编辑结算：从冻结基线到当前镜像只提交一笔差量。
  // 组件不支持 onDidIMEInput，因此保存/退出屏障会主动结算仍在进行的预编辑。
  private settleImeComposition(force: boolean): void {
    if (!this.imeComposing) {
      return
    }
    if (!force && this.readPreviewTextLength() > 0) {
      // 预上屏文本仍在：不是已提交正文，保存屏障继续等待 onDidChange/stopEditing 结算。
      return
    }
    this.commitImeCompositionFromBaseline('ime-commit')
  }

  private readPreviewTextLength(): number {
    if (this.richEditorController === null) {
      return 0
    }
    try {
      const previewText = this.richEditorController.getPreviewText()
      return previewText !== undefined && previewText !== null ? previewText.value.length : 0
    } catch (e) {
      return 0
    }
  }

  private commitImeCompositionFromBaseline(kind: string): void {
    if (!this.imeComposing) {
      return
    }
    const baselineText: string = this.imeComposingStartText
    const baselineNewlines: number = this.imeComposingStartNewlineCount
    this.imeComposing = false
    // 预编辑期间镜像按中间态增量维护；以基线差量重算，避免中间态计数叠加。
    const diff = this.computeDiff(baselineText, this.currentText)
    this.nativeNewlineCount = Math.max(0,
      baselineNewlines + countNewlines(this.currentText) - countNewlines(baselineText))
    if (diff.start >= diff.end && diff.replacement.length === 0) {
      this.recordNativeOperation({
        kind: kind,
        source: 'ime',
        beforeText: baselineText,
        afterText: this.currentText,
        rangeBeforeStartUtf16: diff.start,
        rangeBeforeEndUtf16: diff.end,
        rangeAfterStartUtf16: diff.start,
        rangeAfterEndUtf16: diff.start,
        startUtf16: diff.start,
        endUtf16: diff.end,
        replacedUtf16Length: 0,
        insertedUtf16Length: 0,
        spanCountInRange: -1,
        nonTextSpanCount: 0,
        imeComposition: false,
        programmatic: false,
        factError: false
      })
      return
    }
    const oldText: string = baselineText.substring(diff.start, diff.end)
    this.recordNativeOperation({
      kind: kind,
      source: 'ime',
      beforeText: baselineText,
      afterText: this.currentText,
      rangeBeforeStartUtf16: diff.start,
      rangeBeforeEndUtf16: diff.end,
      rangeAfterStartUtf16: diff.start,
      rangeAfterEndUtf16: diff.start + diff.replacement.length,
      startUtf16: diff.start,
      endUtf16: diff.end,
      replacedUtf16Length: diff.end - diff.start,
      insertedUtf16Length: diff.replacement.length,
      spanCountInRange: -1,
      nonTextSpanCount: 0,
      imeComposition: false,
      programmatic: false,
      factError: false
    })
    const pendingEdit: PendingEdit = {
      sessionId: this.sessionId,
      localSequence: this.localSequence++,
      baseRevision: this.lastAppliedRevision,
      beforeText: baselineText,
      startUtf16: diff.start,
      endUtf16: diff.end,
      insertedText: diff.replacement,
      oldText: oldText,
      cause: EditorTransactionCause.TypingCommit
    }
    this.enqueuePendingEdit(pendingEdit)
  }

  // RichEditor onSelectionChange 回调：用 latest-wins 合并上报，防抖 30ms。
  onSelectionChange(start: number, end: number): void {
    if (this.programmaticUpdate) {
      return
    }

    const expectedCoreSessionId = this.coordinator.getSessionId()
    if (expectedCoreSessionId === null) {
      return
    }

    this.pendingSelectionIntent = {
      localSessionToken: this.sessionId,
      expectedCoreSessionId: expectedCoreSessionId,
      startUtf16: start,
      endUtf16: end,
      textForConversion: this.currentText,
      retriedAtRevision: null
    }

    if (this.selectionFlushTimerId !== -1) {
      clearTimeout(this.selectionFlushTimerId)
    }
    this.selectionFlushTimerId = setTimeout(() => {
      this.selectionFlushTimerId = -1
      this.requestSelectionFlush()
    }, 30)
  }

  private requestSelectionFlush(): void {
    if (this.selectionDrainRunning) {
      return
    }
    this.selectionDrainRunning = true
    const sessionToken = this.sessionId
    this.editQueue = this.editQueue.then(async () => {
      try {
        await this.drainSelectionIntent(sessionToken)
      } catch (e) {
        hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, `drainSelectionIntent error: ${JSON.stringify(e)}`)
        if (sessionToken === this.sessionId) {
          this.idleError = `Selection drain error: ${JSON.stringify(e)}`
        }
      }
      if (sessionToken === this.sessionId) {
        this.selectionDrainRunning = false
      }
      this.checkIdle()
    }).catch((e: Object) => {
      hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, `requestSelectionFlush error: ${JSON.stringify(e)}`)
      if (sessionToken === this.sessionId) {
        this.selectionDrainRunning = false
        this.idleError = `Selection flush error: ${JSON.stringify(e)}`
      }
      this.checkIdle()
    })
  }

  private async drainSelectionIntent(sessionToken: number): Promise<void> {
    while (true) {
      if (sessionToken !== this.sessionId) {
        return
      }

      const intent = this.pendingSelectionIntent
      if (intent === null) {
        return
      }

      if (intent.localSessionToken !== this.sessionId) {
        if (this.pendingSelectionIntent === intent) {
          this.pendingSelectionIntent = null
        }
        continue
      }
      if (this.coordinator.getSessionId() !== intent.expectedCoreSessionId) {
        if (this.pendingSelectionIntent === intent) {
          this.pendingSelectionIntent = null
        }
        continue
      }

      if (intent.textForConversion !== this.currentText) {
        if (this.pendingSelectionIntent === intent) {
          this.pendingSelectionIntent = null
          this.captureCurrentNativeSelection()
        }
        continue
      }

      const snap = this.coordinator.getSnapshot()
      if (snap === null) {
        return
      }
      if (snap.text !== intent.textForConversion) {
        return
      }

      if (this.imeComposing) {
        return
      }

      const byteAnchor = TextOffsetMapper.utf16ToUtf8Checked(intent.textForConversion, intent.startUtf16)
      const byteHead = TextOffsetMapper.utf16ToUtf8Checked(intent.textForConversion, intent.endUtf16)
      if (byteAnchor === null || byteHead === null) {
        this.idleError = `Invalid selection offset: anchor=${intent.startUtf16}, head=${intent.endUtf16}, textLen=${intent.textForConversion.length}`
        hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, this.idleError)
        this.rejectAllWaiters()
        return
      }

      const result = await this.coordinator.setSelectionWithExpectedSession(byteAnchor, byteHead, intent.expectedCoreSessionId)

      if (sessionToken !== this.sessionId) {
        return
      }

      if (!result.success || result.data === undefined) {
        if (result.errorCode === 'SESSION_CHANGED') {
          if (this.pendingSelectionIntent === intent) {
            this.pendingSelectionIntent = null
          }
          continue
        }
        if (this.pendingSelectionIntent !== intent) {
          continue
        }
        this.idleError = `Selection transaction failed: ${result.errorCode ?? 'UNKNOWN'}`
        hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, this.idleError)
        this.rejectAllWaiters()
        return
      }

      const outcome = result.data.outcome
      if (outcome === EditorEditOutcome.Applied ||
          outcome === EditorEditOutcome.AppliedWithAdjustedSelection ||
          outcome === EditorEditOutcome.NoChange) {
        if (this.pendingSelectionIntent === intent) {
          this.pendingSelectionIntent = null
        }
        continue
      }

      if (outcome === EditorEditOutcome.StaleRevision) {
        const newSnap = this.coordinator.getSnapshot()
        if (newSnap !== null && newSnap.text !== intent.textForConversion) {
          return
        }
        const currentRevision = newSnap !== null ? newSnap.revision : null
        if (this.pendingSelectionIntent === intent) {
          if (currentRevision !== null && intent.retriedAtRevision === currentRevision) {
            this.idleError = 'StaleRevision: same revision retry failed, Core snapshot recovery may have failed'
            hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, this.idleError)
            this.rejectAllWaiters()
            return
          }
          this.pendingSelectionIntent = null
          this.captureCurrentNativeSelection(currentRevision)
        }
        continue
      }

      if (outcome === EditorEditOutcome.InvalidOffset || outcome === EditorEditOutcome.InvalidRange) {
        if (this.pendingSelectionIntent !== intent) {
          continue
        }
        this.pendingSelectionIntent = null
        this.captureCurrentNativeSelection()
        if (this.isSameSelectionPosition(this.pendingSelectionIntent, intent)) {
          this.idleError = `${outcome === EditorEditOutcome.InvalidOffset ? 'InvalidOffset' : 'InvalidRange'}: selection cannot be corrected`
          hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, this.idleError)
          this.rejectAllWaiters()
          return
        }
        continue
      }

      if (this.pendingSelectionIntent !== intent) {
        continue
      }
      this.idleError = `Unknown selection outcome: ${outcome}`
      hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, this.idleError)
      this.rejectAllWaiters()
      return
    }
  }

  private isSameSelectionPosition(a: SelectionIntent | null, b: SelectionIntent): boolean {
    if (a === null) {
      return false
    }
    return a.startUtf16 === b.startUtf16 &&
           a.endUtf16 === b.endUtf16 &&
           a.textForConversion === b.textForConversion
  }

  private rejectAllWaiters(): void {
    const error = new Error(this.idleError ?? 'Unknown error')
    const waiters = this.idleWaiters
    this.idleWaiters = []
    for (const w of waiters) {
      w.reject(error)
    }
  }

  private captureCurrentNativeSelection(retriedAtRevision: number | null = null): void {
    if (this.richEditorController === null) {
      return
    }
    const expectedCoreSessionId = this.coordinator.getSessionId()
    if (expectedCoreSessionId === null) {
      return
    }

    const selection = this.readSelectionState()
    if (selection === null) {
      return
    }

    this.pendingSelectionIntent = {
      localSessionToken: this.sessionId,
      expectedCoreSessionId: expectedCoreSessionId,
      startUtf16: selection.start,
      endUtf16: selection.end,
      textForConversion: this.currentText,
      retriedAtRevision: retriedAtRevision
    }
  }

  private enqueuePendingEdit(pendingEdit: PendingEdit): void {
    this.pendingEdits.push(pendingEdit)
    this.wakeDrain()
  }

  private wakeDrain(): void {
    if (this.drainRunning) {
      return
    }
    this.drainRunning = true
    const sessionToken = this.sessionId
    this.editQueue = this.editQueue.then(async () => {
      try {
        await this.drainPendingEdits()
      } catch (e) {
        hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, `drainPendingEdits error: ${JSON.stringify(e)}`)
        if (sessionToken === this.sessionId) {
          this.idleError = `Unexpected error in drainPendingEdits: ${JSON.stringify(e)}`
        }
      }
      if (sessionToken === this.sessionId) {
        this.drainRunning = false
      }
      this.checkIdle()
    }).catch((e: Object) => {
      hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, `wakeDrain error: ${JSON.stringify(e)}`)
      if (sessionToken === this.sessionId) {
        this.drainRunning = false
        this.idleError = `Unexpected error in wakeDrain: ${JSON.stringify(e)}`
      }
      this.checkIdle()
    })
  }

  // 唯一消费者协程：从队首处理，单笔事务确定 Applied 后才取下一笔。
  private async drainPendingEdits(): Promise<void> {
    while (this.pendingEdits.length > 0) {
      if (this.idleError !== null) {
        return
      }

      const edit = this.pendingEdits[0]
      if (edit.sessionId !== this.sessionId) {
        this.pendingEdits.shift()
        continue
      }

      const snap = this.coordinator.getSnapshot()
      if (snap === null) {
        this.idleError = 'No active session'
        hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, this.idleError)
        return
      }

      const baselineText = snap.text
      let mappedStart: number
      let mappedEnd: number

      if (baselineText === edit.beforeText) {
        mappedStart = edit.startUtf16
        mappedEnd = edit.endUtf16
      } else {
        const mapped = this.mapEditToBaseline(edit, baselineText)
        if (mapped === null) {
          this.idleError = `Cannot map pending edit at offset ${edit.startUtf16}: overlapping diff`
          hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, this.idleError)
          return
        }
        const currentSlice = baselineText.substring(mapped.startUtf16, mapped.endUtf16)
        if (currentSlice !== edit.oldText) {
          this.idleError = `Cannot map pending edit at offset ${edit.startUtf16}: text mismatch`
          hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, this.idleError)
          return
        }
        mappedStart = mapped.startUtf16
        mappedEnd = mapped.endUtf16
      }

      const byteStart = TextOffsetMapper.utf16ToUtf8Checked(baselineText, mappedStart)
      const byteEnd = TextOffsetMapper.utf16ToUtf8Checked(baselineText, mappedEnd)
      if (byteStart === null || byteEnd === null) {
        this.idleError = `Invalid edit offset: start=${mappedStart}, end=${mappedEnd}, textLen=${baselineText.length}`
        hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, this.idleError)
        return
      }

      let result: ResultEnvelope<EditorEditResult> | null = null
      if (edit.oldText.length > 0 && edit.insertedText.length > 0) {
        result = await this.coordinator.replace(byteStart, byteEnd, edit.insertedText, edit.oldText, edit.cause)
      } else if (edit.oldText.length > 0) {
        result = await this.coordinator.delete(byteStart, byteEnd, EditorTransactionCause.Delete)
      } else if (edit.insertedText.length > 0) {
        result = await this.coordinator.insert(byteStart, edit.insertedText, edit.cause)
      }

      if (edit.sessionId !== this.sessionId) {
        return
      }

      if (result === null || !result.success || result.data === undefined ||
          (result.data.outcome !== EditorEditOutcome.Applied &&
           result.data.outcome !== EditorEditOutcome.AppliedWithAdjustedSelection &&
           result.data.outcome !== EditorEditOutcome.NoChange)) {
        this.idleError = `Transaction failed for edit at offset ${edit.startUtf16}`
        hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, this.idleError)
        return
      }

      this.pendingEdits.shift()

      const newSnap = this.coordinator.getSnapshot()
      if (newSnap !== null && edit.sessionId === this.sessionId) {
        const expectedLength = baselineText.length - (mappedEnd - mappedStart) + edit.insertedText.length
        if (result.data.outcome === EditorEditOutcome.NoChange) {
          // 正文未变，换行数保持不变。
        } else if (newSnap.text.length === expectedLength) {
          this.coreNewlineCount = Math.max(0,
            this.coreNewlineCount + countNewlines(edit.insertedText) - countNewlines(edit.oldText))
        } else {
          this.coreNewlineCount = countNewlines(newSnap.text)
        }
        this.confirmedText = newSnap.text
        this.confirmedRevision = newSnap.revision
        this.lastAppliedRevision = newSnap.revision
        if (newSnap.text === this.currentText) {
          this.nativeCoreDiverged = false
          this.nativeCoreDivergedSessionId = null
        }
        // Issue #1054：记录/清理“用户删除至空”的合法意图。
        this.updateEmptyDocumentIntent(edit, baselineText, newSnap.text)
        if (result.data.outcome === EditorEditOutcome.AppliedWithAdjustedSelection &&
            this.richEditorController !== null) {
          const utf16Cursor = TextOffsetMapper.utf8ToUtf16(newSnap.text, newSnap.cursor)
          const utf16Anchor = TextOffsetMapper.utf8ToUtf16(newSnap.text, newSnap.selectionAnchor)
          this.richEditorController.setSelection(utf16Anchor, utf16Cursor)
        }
      }

      if (edit.sessionId === this.sessionId) {
        const paragraphsChanged = edit.insertedText.indexOf('\n') >= 0 || edit.oldText.indexOf('\n') >= 0
        this.notifyStateListener('edit-settled', paragraphsChanged)
      }
    }
  }

  // 只在编辑事务把整章正文由非空删成空、且删除范围覆盖全部旧正文时记录意图。
  // 程序性初始空视图、卸载、切章不会进入这里；新的非空提交会清理旧意图。
  private updateEmptyDocumentIntent(edit: PendingEdit, baselineText: string, newText: string): void {
    if (newText.length > 0) {
      this.emptyDocumentIntent = null
      return
    }
    const coreSessionId = this.coordinator.getSessionId()
    if (coreSessionId === null || baselineText.length === 0) {
      return
    }
    const deletesWholeDocument: boolean = edit.insertedText.length === 0 &&
      edit.oldText.length === baselineText.length && edit.oldText === baselineText
    if (!deletesWholeDocument) {
      return
    }
    const snapshot = this.coordinator.getSnapshot()
    const revision = snapshot !== null ? snapshot.revision : this.lastAppliedRevision
    this.emptyDocumentIntent = { sessionId: coreSessionId, revision: revision }
  }

  private computeDiff(oldText: string, newText: string): TextDiff {
    return computeMinimalTextDiff(oldText, newText)
  }

  private mapEditToBaseline(edit: PendingEdit, baselineText: string): EditRangeMap | null {
    if (baselineText === edit.beforeText) {
      return { startUtf16: edit.startUtf16, endUtf16: edit.endUtf16 }
    }

    const diff = this.computeDiff(edit.beforeText, baselineText)
    const diffLenDelta = diff.replacement.length - (diff.end - diff.start)

    if (diff.end <= edit.startUtf16) {
      return { startUtf16: edit.startUtf16 + diffLenDelta, endUtf16: edit.endUtf16 + diffLenDelta }
    }

    if (diff.start >= edit.endUtf16) {
      return { startUtf16: edit.startUtf16, endUtf16: edit.endUtf16 }
    }

    return null
  }

  // ── 原生编辑事实与运行时诊断（不含正文原文） ─────────────────────

  // 从属性字符串读取全文；失败返回 null（controller 未绑定组件时为 undefined）。
  private readNativeFullText(): string | null {
    if (this.richEditorController === null) {
      return null
    }
    try {
      const styled: MutableStyledString = this.richEditorController.getStyledString()
      if (styled === undefined || styled === null) {
        return null
      }
      const text: string = styled.getString()
      // 属性字符串模式没有 Span 列表；Span 数不再作为可读事实。
      this.lastReadNativeSpanCount = -1
      return text
    } catch (e) {
      hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, `readNativeFullText error: ${JSON.stringify(e)}`)
      return null
    }
  }

  // 诊断用：从镜像裁剪指定范围（不再读取 Span）。
  private extractRangeText(range: TextRange): NativeRangeText | null {
    const start = range.start ?? 0
    const end = range.end ?? this.currentText.length
    if (start < 0 || end < start || end > this.currentText.length) {
      return null
    }
    return {
      text: this.currentText.substring(start, end),
      spanCount: -1,
      nonTextSpanCount: 0,
      startUtf16: start,
      endUtf16: end
    }
  }

  private readSelectionState(): NativeSelectionState | null {
    if (this.richEditorController === null) {
      return null
    }
    try {
      // RichEditorStyledStringController.getSelection() 返回 RichEditorRange（start/end），
      // 不是 RichEditorController 的 RichEditorSelection（selection: [start, end]）。
      const selection: RichEditorRange = this.richEditorController.getSelection()
      if (selection === undefined || selection === null) {
        return null
      }
      const start: number = selection.start ?? 0
      const end: number = selection.end ?? start
      let caret = this.richEditorController.getCaretOffset()
      if (caret < 0) {
        caret = end
      }
      return { start: start, end: end, caret: caret }
    } catch (e) {
      return null
    }
  }

  private restoreSelectionState(selection: NativeSelectionState | null, textLength: number): void {
    if (this.richEditorController === null || selection === null) {
      return
    }
    const start = Math.max(0, Math.min(selection.start, textLength))
    const end = Math.max(0, Math.min(selection.end, textLength))
    try {
      this.richEditorController.setSelection(start, end)
    } catch (e) {
      hilog.warn(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, `restore selection failed: ${JSON.stringify(e)}`)
    }
  }

  // 程序性正文重建的唯一入口：构建带样式的属性字符串并整体替换。
  // setStyledString 不会触发 onContentChanged，因此绝不会生成 Core 编辑事务。
  private applyProgrammaticText(text: string): boolean {
    if (this.richEditorController === null) {
      return false
    }
    const styled: MutableStyledString = this.documentBuilder !== null
      ? this.documentBuilder(text)
      : new MutableStyledString(text)
    this.richEditorController.setStyledString(styled)
    return true
  }

  private recordNativeOperation(facts: NativeOperationFacts): void {
    const programmatic: boolean = facts.programmatic
    this.lastNativeOperation = {
      kind: facts.kind,
      beforeUtf16Length: facts.beforeText.length,
      afterUtf16Length: facts.afterText.length,
      rangeBeforeStartUtf16: facts.rangeBeforeStartUtf16,
      rangeBeforeEndUtf16: facts.rangeBeforeEndUtf16,
      rangeAfterStartUtf16: facts.rangeAfterStartUtf16,
      rangeAfterEndUtf16: facts.rangeAfterEndUtf16,
      startUtf16: facts.startUtf16,
      endUtf16: facts.endUtf16,
      replacedUtf16Length: facts.replacedUtf16Length,
      insertedUtf16Length: facts.insertedUtf16Length,
      spanCountInRange: facts.spanCountInRange,
      nonTextSpanCount: facts.nonTextSpanCount,
      imeComposition: facts.imeComposition,
      programmatic: programmatic,
      coreRevision: this.lastAppliedRevision,
      changeSequence: this.changeSequence,
      source: facts.source,
      onWillChangeStartUtf16: programmatic ? -1 : this.lastWillRangeStart,
      onWillChangeEndUtf16: programmatic ? -1 : this.lastWillRangeEnd,
      beforeHash: hashTextUtf16(facts.beforeText),
      afterHash: hashTextUtf16(facts.afterText),
      saveBaselineHash: this.saveBaselineHash,
      factError: facts.factError,
      atMs: Date.now()
    }
    this.lastWillRangeStart = -1
    this.lastWillRangeEnd = -1
  }

  private reportNativeFactError(phase: string, detail: string): void {
    const message = `native text fact error [${phase}]: ${detail}`
    if (this.nativeFactError === null) {
      this.nativeFactError = message
    }
    hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, message)
  }

  private notifyStateListener(kind: string, paragraphsChanged: boolean): void {
    if (this.stateListener === null) {
      return
    }
    try {
      this.stateListener({ kind: kind, paragraphsChanged: paragraphsChanged })
    } catch (e) {
      hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, `state listener error: ${JSON.stringify(e)}`)
    }
  }

  private validateNativeCoreAlignment(stage: string): NativeCoreAlignmentResult {
    const snap = this.coordinator.getSnapshot()
    const coreAvailable = snap !== null
    const coreText = snap !== null ? snap.text : ''
    const nativeText = this.currentText
    const editorText = this.readNativeFullText()
    const nativeAvailable = editorText !== null
    const mirrorMatchesEditor = nativeAvailable && editorText === nativeText
    const firstMismatch = editorText !== null ? firstMismatchUtf16(nativeText, editorText) : -1
    const mismatchToCore = coreAvailable ? firstMismatchUtf16(nativeText, coreText) : -1
    const matched = coreAvailable && nativeAvailable && mirrorMatchesEditor && nativeText === coreText
    const appliedRevision = this.lastAppliedRevision
    const coreRevision = snap !== null ? snap.revision : -1
    const coreAdvanced = coreAvailable && appliedRevision !== coreRevision
    const nativeNewlines = countNewlines(nativeText)
    const coreNewlines = countNewlines(coreText)
    const nativeHash = hashTextUtf16(nativeText)
    const coreHash = hashTextUtf16(coreText)

    if (coreAvailable && mirrorMatchesEditor && !matched && !coreAdvanced) {
      this.nativeCoreDiverged = true
      this.nativeCoreDivergedSessionId = this.coordinator.getSessionId()
    }

    let detail = 'native full text, local mirror and core text are identical'
    if (!coreAvailable) {
      detail = 'no core snapshot at alignment barrier'
    } else if (!nativeAvailable) {
      detail = `native full text unavailable; mirrorLen=${nativeText.length} coreLen=${coreText.length}`
      hilog.warn(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, `native-core alignment unverified stage=${stage} ${detail}`)
    } else if (!matched) {
      detail = `mirrorLen=${nativeText.length} coreLen=${coreText.length}`
        + ` nativeNewlines=${nativeNewlines} coreNewlines=${coreNewlines}`
        + ` nativeHash=${nativeHash} coreHash=${coreHash}`
        + ` mirrorMatchesEditor=${mirrorMatchesEditor} firstMismatch=${firstMismatch}`
        + ` mismatchToCore=${mismatchToCore}`
      hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG,
        `native-core alignment mismatch stage=${stage} ${detail}`)
    }

    this.lastNativeCoreAlignment = {
      stage: stage,
      matched: matched,
      nativeUtf16Length: nativeText.length,
      coreUtf16Length: coreText.length,
      nativeNewlineCount: nativeNewlines,
      coreNewlineCount: coreNewlines,
      nativeHash: nativeHash,
      coreHash: coreHash,
      nativeSpanCount: this.lastReadNativeSpanCount,
      nativeRevision: appliedRevision,
      coreRevision: coreRevision,
      mirrorMatchesEditor: mirrorMatchesEditor,
      firstMismatchUtf16: firstMismatch,
      detail: detail,
      atMs: Date.now()
    }
    this.notifyStateListener('barrier-alignment', false)

    return {
      stage: stage,
      matched: matched,
      coreAvailable: coreAvailable,
      nativeAvailable: nativeAvailable,
      mirrorMatchesEditor: mirrorMatchesEditor,
      nativeText: editorText !== null ? editorText : nativeText,
      nativeUtf16Length: nativeText.length,
      coreUtf16Length: coreText.length,
      nativeSpanCount: this.lastReadNativeSpanCount,
      nativeRevision: appliedRevision,
      firstMismatchUtf16: firstMismatch,
      mismatchToCoreUtf16: mismatchToCore,
      appliedRevision: appliedRevision,
      coreRevision: coreRevision,
      coreAdvanced: coreAdvanced,
      detail: detail
    }
  }

  private refreshSelectionAfterConvergence(): void {
    if (this.richEditorController === null || this.coordinator.getSessionId() === null) {
      return
    }
    this.pendingSelectionIntent = null
    this.captureCurrentNativeSelection()
    if (this.pendingSelectionIntent !== null) {
      this.requestSelectionFlush()
    }
  }

  private async recoverFromCore(): Promise<void> {
    const snap = this.coordinator.getSnapshot()
    if (snap === null || this.richEditorController === null) {
      return
    }

    this.programmaticUpdate = true
    try {
      if (this.currentText !== snap.text) {
        this.applyProgrammaticText(snap.text)
        this.currentText = snap.text
      }
      this.nativeNewlineCount = countNewlines(snap.text)
      this.coreNewlineCount = this.nativeNewlineCount

      const utf16Cursor = TextOffsetMapper.utf8ToUtf16(snap.text, snap.cursor)
      const utf16Anchor = TextOffsetMapper.utf8ToUtf16(snap.text, snap.selectionAnchor)
      this.richEditorController.setSelection(utf16Anchor, utf16Cursor)

      this.lastAppliedRevision = snap.revision
      this.confirmedText = snap.text
      this.confirmedRevision = snap.revision
      this.nativeTextReady = true
    } catch (e) {
      hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, `recoverFromCore error: ${JSON.stringify(e)}`)
    } finally {
      this.programmaticUpdate = false
    }
  }

  // 程序回放：从 Core snapshot 同步正文到 RichEditor。
  // 有 pendingEdits/imeComposing/idleError 时不覆盖 RichEditor，保留用户可见正文。
  async syncFromCore(): Promise<void> {
    const sessionAtEntry = this.sessionId
    await this.editQueue

    if (this.sessionId !== sessionAtEntry) {
      return
    }

    if (this.pendingEdits.length > 0 || this.imeComposing || this.idleError !== null) {
      hilog.warn(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, 'syncFromCore: deferred due to pending edits/IME/error')
      return
    }

    if (this.nativeCoreDiverged &&
        this.coordinator.getSessionId() !== this.nativeCoreDivergedSessionId) {
      this.nativeCoreDiverged = false
      this.nativeCoreDivergedSessionId = null
    }
    if (this.nativeCoreDiverged) {
      const divergedSnap = this.coordinator.getSnapshot()
      if (divergedSnap === null || divergedSnap.text !== this.currentText) {
        hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG,
          'syncFromCore: refused to overwrite native text while native and core are diverged')
        return
      }
    }

    const snap = this.coordinator.getSnapshot()
    if (snap === null || this.richEditorController === null) {
      return
    }

    if (this.nativeTextReady && this.currentText !== this.confirmedText) {
      hilog.warn(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG,
        `syncFromCore: deferred because confirmed core text lags native mirror`
          + ` nativeLen=${this.currentText.length} confirmedLen=${this.confirmedText.length}`)
      return
    }

    if (this.currentText === snap.text) {
      this.nativeNewlineCount = countNewlines(this.currentText)
      this.coreNewlineCount = this.nativeNewlineCount
      this.lastAppliedRevision = snap.revision
      this.confirmedText = snap.text
      this.confirmedRevision = snap.revision
      this.recordNativeOperation({
        kind: 'programmatic-sync',
        source: 'programmatic',
        beforeText: this.currentText,
        afterText: snap.text,
        rangeBeforeStartUtf16: 0,
        rangeBeforeEndUtf16: 0,
        rangeAfterStartUtf16: 0,
        rangeAfterEndUtf16: 0,
        startUtf16: 0,
        endUtf16: 0,
        replacedUtf16Length: 0,
        insertedUtf16Length: 0,
        spanCountInRange: 0,
        nonTextSpanCount: 0,
        imeComposition: false,
        programmatic: true,
        factError: false
      })
      this.nativeTextReady = true
      this.validateNativeCoreAlignment('sync-from-core')
      return
    }

    const previousText = this.currentText
    const paragraphsChanged = countNewlines(previousText) !== countNewlines(snap.text)
    const previousSelection = this.readSelectionState()
    let syncStart = 0
    let syncEnd = 0
    let syncInsertedLength = 0

    this.programmaticUpdate = true
    try {
      const diff = this.computeDiff(previousText, snap.text)
      syncStart = diff.start
      syncEnd = diff.end
      syncInsertedLength = diff.replacement.length
      if (diff.start < diff.end || diff.replacement.length > 0) {
        this.applyProgrammaticText(snap.text)
      }
      this.currentText = snap.text
      this.nativeNewlineCount = countNewlines(snap.text)
      this.coreNewlineCount = this.nativeNewlineCount
      this.restoreSelectionState(previousSelection, snap.text.length)

      this.lastAppliedRevision = snap.revision
      this.confirmedText = snap.text
      this.confirmedRevision = snap.revision
      this.nativeCoreDiverged = false
      this.nativeCoreDivergedSessionId = null
      if (snap.text.length > 0) {
        this.emptyDocumentIntent = null
      }
    } catch (e) {
      hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, `syncFromCore error: ${JSON.stringify(e)}`)
    } finally {
      this.programmaticUpdate = false
    }

    this.recordNativeOperation({
      kind: 'programmatic-sync',
      source: 'programmatic',
      beforeText: previousText,
      afterText: this.currentText,
      rangeBeforeStartUtf16: syncStart,
      rangeBeforeEndUtf16: syncEnd,
      rangeAfterStartUtf16: syncStart,
      rangeAfterEndUtf16: syncStart + syncInsertedLength,
      startUtf16: syncStart,
      endUtf16: syncEnd,
      replacedUtf16Length: syncEnd - syncStart,
      insertedUtf16Length: syncInsertedLength,
      spanCountInRange: -1,
      nonTextSpanCount: 0,
      imeComposition: false,
      programmatic: true,
      factError: false
    })
    this.nativeTextReady = true
    this.notifyStateListener('sync-from-core', paragraphsChanged)
    this.validateNativeCoreAlignment('sync-from-core')
  }

  // 外部明确要求定位选区的场景（跳转纠错/搜索结果定位）。
  async syncSelectionFromCore(): Promise<void> {
    const sessionAtEntry = this.sessionId
    await this.editQueue

    if (this.sessionId !== sessionAtEntry) {
      return
    }

    const snap = this.coordinator.getSnapshot()
    if (snap === null || this.richEditorController === null) {
      return
    }

    this.programmaticUpdate = true
    try {
      const utf16Cursor = TextOffsetMapper.utf8ToUtf16(snap.text, snap.cursor)
      const utf16Anchor = TextOffsetMapper.utf8ToUtf16(snap.text, snap.selectionAnchor)
      this.richEditorController.setSelection(utf16Anchor, utf16Cursor)
    } catch (e) {
      hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, `syncSelectionFromCore error: ${JSON.stringify(e)}`)
    } finally {
      this.programmaticUpdate = false
    }
  }

  // 初始加载：把 Core 返回的初始正文写入 RichEditor。
  async loadInitialText(text: string, cursorByteOffset: number): Promise<void> {
    if (this.richEditorController === null) {
      return
    }

    const previousText = this.currentText
    this.programmaticUpdate = true
    try {
      this.applyProgrammaticText(text)
      this.currentText = text
      this.nativeNewlineCount = countNewlines(text)
      this.coreNewlineCount = this.nativeNewlineCount
      this.emptyDocumentIntent = null

      const utf16Cursor = TextOffsetMapper.utf8ToUtf16(text, cursorByteOffset)
      this.richEditorController.setSelection(utf16Cursor, utf16Cursor)

      const snap = this.coordinator.getSnapshot()
      if (snap !== null) {
        this.lastAppliedRevision = snap.revision
        this.confirmedText = snap.text
        this.confirmedRevision = snap.revision
      }
    } catch (e) {
      hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, `loadInitialText error: ${JSON.stringify(e)}`)
    } finally {
      this.programmaticUpdate = false
    }

    this.recordNativeOperation({
      kind: 'initial-load',
      source: 'programmatic',
      beforeText: previousText,
      afterText: text,
      rangeBeforeStartUtf16: 0,
      rangeBeforeEndUtf16: previousText.length,
      rangeAfterStartUtf16: 0,
      rangeAfterEndUtf16: text.length,
      startUtf16: 0,
      endUtf16: previousText.length,
      replacedUtf16Length: previousText.length,
      insertedUtf16Length: text.length,
      spanCountInRange: -1,
      nonTextSpanCount: 0,
      imeComposition: false,
      programmatic: true,
      factError: false
    })
    this.nativeTextReady = true
    this.notifyStateListener('initial-load', true)
    this.validateNativeCoreAlignment('initial-load')
  }
}
