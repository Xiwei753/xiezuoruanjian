// NativeRichEditorBridge — RichEditor 与 EditorSessionCoordinator 之间的输入桥接。
// Issue #1034 评论 #5：用 ArkUI 原生 RichEditor 替换自研编辑器。
// Issue #1034 评论 #6 修复：正确使用 RichEditorController API、串行事务队列、UTF-8/UTF-16 坐标转换。
// Issue #1034 评论 #7 修复：getSpans offsetInSpan 截取、deleteSpans() 无参数清空、
//   computeDiff 按 code point、whenIdle 注入保存链路、事务失败恢复。
// Issue #1034 评论 #8 补全：IME 预编辑/提交区分、事务失败队列收敛恢复。
// Issue #1034 评论 #10 修复：IME 预编辑由真实预上屏状态决定、PendingEdit 不可变结构、
//   逐笔重放保持原 cause、whenIdle 报告失败、切章 session ID 隔离。
//
// 职责：
// - RichEditor onWillChange → 只做校验（返回 true 允许改变），不调用 coordinator。
// - RichEditor aboutToIMEInput → 读取 previewText 判断是否预编辑开始，首次冻结基线。
// - RichEditor onDidChange(rangeBefore, rangeAfter) → 从前后文本差量计算编辑事实，
//   IME 预编辑期间只更新本地镜像不提交 Core；非预编辑期间串行提交给 coordinator。
// - RichEditor onDidIMEInput → 通过 getPreviewText() 确认预上屏消失后才提交差量。
// - RichEditor onSelectionChange → 将稳定选区映射后传给 EditorSessionCoordinator.setSelection()。
// - 维护 programmaticUpdate 标记和有序 revision，区分用户编辑与程序回放。
// - 事务失败时从 Core 最新 snapshot 重新映射剩余 edits、保持原 cause 逐笔重放。
// - 真实内容冲突无法映射时保留 pending 内容和错误状态，whenIdle 报告失败。
// - 切章时以 session ID 隔离旧回调，旧队列不可触碰新章节 UI。
// - Core 外部加载/撤销/同步更新通过 revision 比较、最小文本差量更新 RichEditor。

import { EditorSessionCoordinator } from '../session/EditorSessionCoordinator'
import { TextOffsetMapper } from './TextOffsetMapper'
import { EditorTransactionCause, EditorEditResult, EditorSessionSnapshot, EditorEditOutcome } from '../../../corebridge/dto/EditorDtos'
import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'
import hilog from '@ohos.hilog'

const BRIDGE_LOG_DOMAIN = 0x0000
const BRIDGE_LOG_TAG = 'NativeRichEditorBridge'

interface TextDiff {
  start: number
  end: number
  replacement: string
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

export class NativeRichEditorBridge {
  private coordinator: EditorSessionCoordinator
  private richEditorController: RichEditorController | null = null
  private currentText: string = ''
  private programmaticUpdate: boolean = false
  private lastAppliedRevision: number = -1
  private editQueue: Promise<void> = Promise.resolve()
  private pendingPaste: boolean = false
  private pendingEdits: PendingEdit[] = []
  private imeComposing: boolean = false
  private imeComposingStartText: string = ''
  private recoveryScheduled: boolean = false
  private sessionId: number = 0
  private localSequence: number = 0
  private confirmedText: string = ''
  private confirmedRevision: number = -1
  private idleError: string | null = null

  constructor(coordinator: EditorSessionCoordinator) {
    this.coordinator = coordinator
  }

  setController(controller: RichEditorController): void {
    this.richEditorController = controller
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

  // 返回当前 editQueue 尾部，供保存/同步/关闭链路在 coordinator.whenIdle 之前 await。
  // 如果 IME 预编辑还在，先结算差量。
  // 如果有未解决的 idleError，抛出错误。
  whenIdle(): Promise<void> {
    if (this.imeComposing) {
      this.settleImeComposition()
    }
    return this.editQueue.then(() => {
      if (this.idleError !== null) {
        throw new Error(this.idleError)
      }
    })
  }

  setPendingPaste(): void {
    this.pendingPaste = true
  }

  // 切章时调用：增加 sessionId 隔离旧回调，清空队列和错误状态。
  beginSession(): void {
    this.sessionId++
    this.pendingEdits = []
    this.idleError = null
    this.imeComposing = false
    this.imeComposingStartText = ''
    this.recoveryScheduled = false
    this.localSequence = 0
  }

  // onWillChange 只做校验：允许所有改变。
  onWillChange(_value: RichEditorChangeValue): boolean {
    return true
  }

  // onDidChange：内容已改变后，从 rangeBefore/rangeAfter 和本地镜像计算编辑事实，
  // 串行提交给 coordinator。
  // IME 预编辑期间只更新本地镜像，不提交 Core 事务。
  onDidChange(rangeBefore: TextRange, rangeAfter: TextRange): void {
    if (this.programmaticUpdate || this.richEditorController === null) {
      return
    }

    const insertedSpans = this.richEditorController.getSpans({
      start: rangeAfter.start,
      end: rangeAfter.end
    })
    const textSpans: RichEditorTextSpanResult[] = []
    for (const span of insertedSpans) {
      if ((span as RichEditorTextSpanResult).value !== undefined) {
        textSpans.push(span as RichEditorTextSpanResult)
      } else {
        hilog.warn(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, 'onDidChange: non-text span encountered, skipping')
      }
    }
    const sortedSpans = textSpans.sort((a: RichEditorTextSpanResult, b: RichEditorTextSpanResult): number => {
      return a.spanPosition.spanRange[0] - b.spanPosition.spanRange[0]
    })
    let insertedText = ''
    for (const span of sortedSpans) {
      const offsetStart = span.offsetInSpan[0]
      const offsetEnd = span.offsetInSpan[1]
      insertedText += span.value.substring(offsetStart, offsetEnd)
    }

    const oldText = this.currentText.substring(rangeBefore.start, rangeBefore.end)

    const newText = this.currentText.substring(0, rangeBefore.start)
      + insertedText
      + this.currentText.substring(rangeBefore.end)

    const previousText = this.currentText
    this.currentText = newText

    if (this.imeComposing) {
      return
    }

    const cause: EditorTransactionCause = this.pendingPaste ? EditorTransactionCause.Paste : EditorTransactionCause.Typing
    this.pendingPaste = false

    const pendingEdit: PendingEdit = {
      sessionId: this.sessionId,
      localSequence: this.localSequence++,
      baseRevision: this.lastAppliedRevision,
      beforeText: previousText,
      startUtf16: rangeBefore.start,
      endUtf16: rangeBefore.end,
      insertedText: insertedText,
      oldText: oldText,
      cause: cause
    }
    this.pendingEdits.push(pendingEdit)

    this.editQueue = this.editQueue.then(async () => {
      if (this.idleError !== null) {
        return
      }
      if (pendingEdit.sessionId !== this.sessionId) {
        return
      }
      const editIndex = this.pendingEdits.indexOf(pendingEdit)
      if (editIndex < 0) {
        return
      }

      const snap = this.coordinator.getSnapshot()
      if (snap === null) {
        hilog.warn(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, 'onDidChange: no active session')
        return
      }

      const byteStart = TextOffsetMapper.utf16ToUtf8(pendingEdit.beforeText, pendingEdit.startUtf16)
      const byteEnd = TextOffsetMapper.utf16ToUtf8(pendingEdit.beforeText, pendingEdit.endUtf16)

      let result: ResultEnvelope<EditorEditResult> | null = null
      let transactionFailed = false
      if (pendingEdit.oldText.length > 0 && pendingEdit.insertedText.length > 0) {
        result = await this.coordinator.replace(byteStart, byteEnd, pendingEdit.insertedText, pendingEdit.oldText, pendingEdit.cause)
      } else if (pendingEdit.oldText.length > 0) {
        result = await this.coordinator.delete(byteStart, byteEnd, EditorTransactionCause.Delete)
      } else if (pendingEdit.insertedText.length > 0) {
        result = await this.coordinator.insert(byteStart, pendingEdit.insertedText, pendingEdit.cause)
      }

      if (result !== null) {
        if (!result.success || result.data === undefined ||
            (result.data.outcome !== EditorEditOutcome.Applied &&
             result.data.outcome !== EditorEditOutcome.AppliedWithAdjustedSelection &&
             result.data.outcome !== EditorEditOutcome.NoChange)) {
          transactionFailed = true
        }
      }

      if (transactionFailed) {
        hilog.warn(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, 'onDidChange: transaction failed, scheduling recovery')
        this.scheduleRecovery()
        return
      }

      this.pendingEdits.splice(editIndex, 1)

      const newSnap = this.coordinator.getSnapshot()
      if (newSnap !== null) {
        this.confirmedText = newSnap.text
        this.confirmedRevision = newSnap.revision
        this.lastAppliedRevision = newSnap.revision
        if (result !== null && result.success && result.data !== undefined &&
            result.data.outcome === EditorEditOutcome.AppliedWithAdjustedSelection &&
            this.richEditorController !== null) {
          const utf16Cursor = TextOffsetMapper.utf8ToUtf16(newSnap.text, newSnap.cursor)
          const utf16Anchor = TextOffsetMapper.utf8ToUtf16(newSnap.text, newSnap.selectionAnchor)
          this.richEditorController.setSelection(utf16Anchor, utf16Cursor)
        }
      }
    }).catch(e => {
      hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, `onDidChange error: ${JSON.stringify(e)}`)
      this.scheduleRecovery()
    })
  }

  // Issue #1034 评论 #10：aboutToIMEInput 读取 previewText 判断是否预编辑开始。
  // 只有当 insertValue.previewText 非空时才标记 composing，首次冻结基线。
  aboutToIMEInput(insertValue: RichEditorInsertValue): boolean {
    if (insertValue.previewText !== undefined && insertValue.previewText.length > 0) {
      if (!this.imeComposing) {
        this.imeComposing = true
        this.imeComposingStartText = this.currentText
      }
    }
    return true
  }

  // Issue #1034 评论 #10：onDidIMEInput 通过 getPreviewText() 确认预上屏消失后才提交差量。
  // 预上屏还在时不做提交；预上屏消失后计算基线和最终文本差量生成一笔 TypingCommit 事务。
  // 取消预上屏时差量为空，不提交事务（不当删除记录）。
  onDidIMEInput(_range: TextRange): void {
    if (this.richEditorController !== null) {
      const previewText = this.richEditorController.getPreviewText()
      if (previewText.offset >= 0 && previewText.value.length > 0) {
        return
      }
    }

    if (!this.imeComposing) {
      return
    }
    this.imeComposing = false

    const diff = this.computeDiff(this.imeComposingStartText, this.currentText)
    if (diff.start >= diff.end && diff.replacement.length === 0) {
      return
    }

    const baselineText = this.imeComposingStartText
    const diffStart = diff.start
    const diffEnd = diff.end
    const diffReplacement = diff.replacement
    const diffOldText = baselineText.substring(diffStart, diffEnd)

    this.editQueue = this.editQueue.then(async () => {
      if (this.idleError !== null) {
        return
      }

      const snap = this.coordinator.getSnapshot()
      if (snap === null) {
        hilog.warn(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, 'onDidIMEInput: no active session')
        return
      }

      if (snap.text !== baselineText) {
        hilog.warn(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, 'onDidIMEInput: Core text changed during IME composition, scheduling recovery')
        this.scheduleRecovery()
        return
      }

      const byteStart = TextOffsetMapper.utf16ToUtf8(baselineText, diffStart)
      const byteEnd = TextOffsetMapper.utf16ToUtf8(baselineText, diffEnd)

      let result: ResultEnvelope<EditorEditResult> | null = null
      if (diffOldText.length > 0 && diffReplacement.length > 0) {
        result = await this.coordinator.replace(byteStart, byteEnd, diffReplacement, diffOldText, EditorTransactionCause.TypingCommit)
      } else if (diffOldText.length > 0) {
        result = await this.coordinator.delete(byteStart, byteEnd, EditorTransactionCause.Delete)
      } else if (diffReplacement.length > 0) {
        result = await this.coordinator.insert(byteStart, diffReplacement, EditorTransactionCause.TypingCommit)
      }

      if (result !== null) {
        if (!result.success || result.data === undefined ||
            (result.data.outcome !== EditorEditOutcome.Applied &&
             result.data.outcome !== EditorEditOutcome.AppliedWithAdjustedSelection &&
             result.data.outcome !== EditorEditOutcome.NoChange)) {
          hilog.warn(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, 'onDidIMEInput: transaction failed, scheduling recovery')
          this.scheduleRecovery()
          return
        }
      }

      const newSnap = this.coordinator.getSnapshot()
      if (newSnap !== null) {
        this.confirmedText = newSnap.text
        this.confirmedRevision = newSnap.revision
        this.lastAppliedRevision = newSnap.revision
        if (result !== null && result.success && result.data !== undefined &&
            result.data.outcome === EditorEditOutcome.AppliedWithAdjustedSelection &&
            this.richEditorController !== null) {
          const utf16Cursor = TextOffsetMapper.utf8ToUtf16(newSnap.text, newSnap.cursor)
          const utf16Anchor = TextOffsetMapper.utf8ToUtf16(newSnap.text, newSnap.selectionAnchor)
          this.richEditorController.setSelection(utf16Anchor, utf16Cursor)
        }
      }
    }).catch(e => {
      hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, `onDidIMEInput error: ${JSON.stringify(e)}`)
      this.imeComposing = false
      this.scheduleRecovery()
    })
  }

  // RichEditor onSelectionChange 回调：将稳定选区映射后传给 coordinator。
  onSelectionChange(start: number, end: number): void {
    if (this.programmaticUpdate) {
      return
    }

    const textForConversion = this.currentText
    this.editQueue = this.editQueue.then(async () => {
      const snap = this.coordinator.getSnapshot()
      if (snap === null) {
        return
      }

      const byteAnchor = TextOffsetMapper.utf16ToUtf8(textForConversion, start)
      const byteHead = TextOffsetMapper.utf16ToUtf8(textForConversion, end)
      await this.coordinator.setSelection(byteAnchor, byteHead)
    }).catch(e => {
      hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, `onSelectionChange error: ${JSON.stringify(e)}`)
    })
  }

  // 计算最小文本差量（按完整 Unicode code point 求前后缀，避免 surrogate pair 内部断裂）
  private computeDiff(oldText: string, newText: string): TextDiff {
    let prefixLen = 0
    while (prefixLen < oldText.length && prefixLen < newText.length) {
      const oldCp = oldText.codePointAt(prefixLen)
      const newCp = newText.codePointAt(prefixLen)
      if (oldCp !== newCp) {
        break
      }
      prefixLen += (oldCp > 0xFFFF) ? 2 : 1
    }

    let oldSuffixStart = oldText.length
    let newSuffixStart = newText.length
    while (oldSuffixStart > prefixLen && newSuffixStart > prefixLen) {
      let oldCpLen = 1
      if (oldSuffixStart >= 2) {
        const lastUnit = oldText.charCodeAt(oldSuffixStart - 1)
        const prevUnit = oldText.charCodeAt(oldSuffixStart - 2)
        if (lastUnit >= 0xDC00 && lastUnit <= 0xDFFF && prevUnit >= 0xD800 && prevUnit <= 0xDBFF) {
          oldCpLen = 2
        }
      }
      let newCpLen = 1
      if (newSuffixStart >= 2) {
        const lastUnit = newText.charCodeAt(newSuffixStart - 1)
        const prevUnit = newText.charCodeAt(newSuffixStart - 2)
        if (lastUnit >= 0xDC00 && lastUnit <= 0xDFFF && prevUnit >= 0xD800 && prevUnit <= 0xDBFF) {
          newCpLen = 2
        }
      }

      const oldCp = oldText.codePointAt(oldSuffixStart - oldCpLen)
      const newCp = newText.codePointAt(newSuffixStart - newCpLen)
      if (oldCp !== newCp) {
        break
      }
      oldSuffixStart -= oldCpLen
      newSuffixStart -= newCpLen
    }

    return {
      start: prefixLen,
      end: oldSuffixStart,
      replacement: newText.substring(prefixLen, newSuffixStart)
    }
  }

  // Issue #1034 评论 #10：将恢复任务排到队列末尾，避免覆盖后续输入。
  // recoveryScheduled 确保同一时间只排一个恢复任务。
  private scheduleRecovery(): void {
    if (this.recoveryScheduled) {
      return
    }
    this.recoveryScheduled = true
    this.editQueue = this.editQueue.then(async () => {
      this.recoveryScheduled = false
      await this.recoverWithPendingEdits()
    }).catch(e => {
      this.recoveryScheduled = false
      hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, `scheduleRecovery error: ${JSON.stringify(e)}`)
    })
  }

  // Issue #1034 评论 #10：逐笔映射重放——从 Core 最新 snapshot 开始，
  // 逐笔检查 pendingEdit 在 Core 文本中的对应位置是否匹配。
  // 匹配则保持原 cause 提交；不匹配则设置 idleError，停止重放。
  private async recoverWithPendingEdits(): Promise<void> {
    if (this.pendingEdits.length === 0) {
      return
    }

    const snap = this.coordinator.getSnapshot()
    if (snap === null || this.richEditorController === null) {
      return
    }

    let baselineText = snap.text

    while (this.pendingEdits.length > 0) {
      const edit = this.pendingEdits[0]
      if (edit.sessionId !== this.sessionId) {
        this.pendingEdits.shift()
        continue
      }

      const currentSlice = baselineText.substring(edit.startUtf16, edit.endUtf16)
      if (currentSlice !== edit.oldText) {
        this.idleError = `Cannot map pending edit at offset ${edit.startUtf16}`
        hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, this.idleError)
        return
      }

      const byteStart = TextOffsetMapper.utf16ToUtf8(baselineText, edit.startUtf16)
      const byteEnd = TextOffsetMapper.utf16ToUtf8(baselineText, edit.endUtf16)

      let result: ResultEnvelope<EditorEditResult> | null = null
      if (edit.oldText.length > 0 && edit.insertedText.length > 0) {
        result = await this.coordinator.replace(byteStart, byteEnd, edit.insertedText, edit.oldText, edit.cause)
      } else if (edit.oldText.length > 0) {
        result = await this.coordinator.delete(byteStart, byteEnd, EditorTransactionCause.Delete)
      } else if (edit.insertedText.length > 0) {
        result = await this.coordinator.insert(byteStart, edit.insertedText, edit.cause)
      }

      if (result === null || !result.success || result.data === undefined ||
          (result.data.outcome !== EditorEditOutcome.Applied &&
           result.data.outcome !== EditorEditOutcome.AppliedWithAdjustedSelection &&
           result.data.outcome !== EditorEditOutcome.NoChange)) {
        this.idleError = `Recovery failed for edit at offset ${edit.startUtf16}`
        hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, this.idleError)
        return
      }

      baselineText = baselineText.substring(0, edit.startUtf16)
        + edit.insertedText
        + baselineText.substring(edit.endUtf16)

      this.pendingEdits.shift()

      const newSnap = this.coordinator.getSnapshot()
      if (newSnap !== null) {
        this.confirmedText = newSnap.text
        this.confirmedRevision = newSnap.revision
        this.lastAppliedRevision = newSnap.revision
        if (result.data.outcome === EditorEditOutcome.AppliedWithAdjustedSelection &&
            this.richEditorController !== null) {
          const utf16Cursor = TextOffsetMapper.utf8ToUtf16(newSnap.text, newSnap.cursor)
          const utf16Anchor = TextOffsetMapper.utf8ToUtf16(newSnap.text, newSnap.selectionAnchor)
          this.richEditorController.setSelection(utf16Anchor, utf16Cursor)
        }
      }
    }
  }

  // 从 Core snapshot 恢复：事务失败或需要强制对齐时调用。
  // 把 RichEditor 和本地镜像回归到 Core 同一版本，选区也同步恢复。
  private async recoverFromCore(): Promise<void> {
    const snap = this.coordinator.getSnapshot()
    if (snap === null || this.richEditorController === null) {
      return
    }

    this.programmaticUpdate = true
    try {
      if (this.currentText !== snap.text) {
        const diff = this.computeDiff(this.currentText, snap.text)
        if (diff.start < diff.end || diff.replacement.length > 0) {
          this.richEditorController.deleteSpans({ start: diff.start, end: diff.end })
          if (diff.replacement.length > 0) {
            this.richEditorController.addTextSpan(diff.replacement, { offset: diff.start })
          }
        }
        this.currentText = snap.text
      }

      const utf16Cursor = TextOffsetMapper.utf8ToUtf16(snap.text, snap.cursor)
      const utf16Anchor = TextOffsetMapper.utf8ToUtf16(snap.text, snap.selectionAnchor)
      this.richEditorController.setSelection(utf16Anchor, utf16Cursor)

      this.lastAppliedRevision = snap.revision
      this.confirmedText = snap.text
      this.confirmedRevision = snap.revision
    } catch (e) {
      hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, `recoverFromCore error: ${JSON.stringify(e)}`)
    } finally {
      this.programmaticUpdate = false
    }
  }

  // 结算 IME 预编辑态：在保存/同步/关闭之前调用。
  // 如果预上屏还在，提交当前差量作为 TypingCommit 事务。
  private settleImeComposition(): void {
    if (!this.imeComposing) {
      return
    }
    this.imeComposing = false

    const diff = this.computeDiff(this.imeComposingStartText, this.currentText)
    if (diff.start >= diff.end && diff.replacement.length === 0) {
      return
    }

    const baselineText = this.imeComposingStartText
    const diffStart = diff.start
    const diffEnd = diff.end
    const diffReplacement = diff.replacement
    const diffOldText = baselineText.substring(diffStart, diffEnd)

    this.editQueue = this.editQueue.then(async () => {
      if (this.idleError !== null) {
        return
      }

      const snap = this.coordinator.getSnapshot()
      if (snap === null) {
        return
      }

      if (snap.text !== baselineText) {
        hilog.warn(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, 'settleImeComposition: Core text changed, scheduling recovery')
        this.scheduleRecovery()
        return
      }

      const byteStart = TextOffsetMapper.utf16ToUtf8(baselineText, diffStart)
      const byteEnd = TextOffsetMapper.utf16ToUtf8(baselineText, diffEnd)

      let result: ResultEnvelope<EditorEditResult> | null = null
      if (diffOldText.length > 0 && diffReplacement.length > 0) {
        result = await this.coordinator.replace(byteStart, byteEnd, diffReplacement, diffOldText, EditorTransactionCause.TypingCommit)
      } else if (diffOldText.length > 0) {
        result = await this.coordinator.delete(byteStart, byteEnd, EditorTransactionCause.Delete)
      } else if (diffReplacement.length > 0) {
        result = await this.coordinator.insert(byteStart, diffReplacement, EditorTransactionCause.TypingCommit)
      }

      if (result !== null) {
        if (!result.success || result.data === undefined ||
            (result.data.outcome !== EditorEditOutcome.Applied &&
             result.data.outcome !== EditorEditOutcome.AppliedWithAdjustedSelection &&
             result.data.outcome !== EditorEditOutcome.NoChange)) {
          hilog.warn(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, 'settleImeComposition: transaction failed, scheduling recovery')
          this.scheduleRecovery()
          return
        }
      }

      const newSnap = this.coordinator.getSnapshot()
      if (newSnap !== null) {
        this.confirmedText = newSnap.text
        this.confirmedRevision = newSnap.revision
        this.lastAppliedRevision = newSnap.revision
      }
    }).catch(e => {
      hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, `settleImeComposition error: ${JSON.stringify(e)}`)
      this.scheduleRecovery()
    })
  }

  // 程序回放：从 Core snapshot 同步正文到 RichEditor。
  async syncFromCore(): Promise<void> {
    await this.editQueue

    const snap = this.coordinator.getSnapshot()
    if (snap === null || this.richEditorController === null) {
      return
    }

    this.programmaticUpdate = true
    try {
      if (this.currentText !== snap.text) {
        const diff = this.computeDiff(this.currentText, snap.text)
        if (diff.start < diff.end || diff.replacement.length > 0) {
          this.richEditorController.deleteSpans({ start: diff.start, end: diff.end })
          if (diff.replacement.length > 0) {
            this.richEditorController.addTextSpan(diff.replacement, { offset: diff.start })
          }
        }
        this.currentText = snap.text
      }

      const utf16Cursor = TextOffsetMapper.utf8ToUtf16(snap.text, snap.cursor)
      const utf16Anchor = TextOffsetMapper.utf8ToUtf16(snap.text, snap.selectionAnchor)
      this.richEditorController.setSelection(utf16Anchor, utf16Cursor)

      this.lastAppliedRevision = snap.revision
      this.confirmedText = snap.text
      this.confirmedRevision = snap.revision
    } catch (e) {
      hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, `syncFromCore error: ${JSON.stringify(e)}`)
    } finally {
      this.programmaticUpdate = false
    }
  }

  // 初始加载：把 Core 返回的初始正文写入 RichEditor。
  async loadInitialText(text: string, cursorByteOffset: number): Promise<void> {
    if (this.richEditorController === null) {
      return
    }

    this.programmaticUpdate = true
    try {
      this.richEditorController.deleteSpans()
      this.richEditorController.addTextSpan(text, { offset: 0 })
      this.currentText = text

      const utf16Cursor = TextOffsetMapper.utf16ToUtf16(text, cursorByteOffset)
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
  }
}
