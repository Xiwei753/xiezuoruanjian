// NativeRichEditorBridge — RichEditor 与 EditorSessionCoordinator 之间的输入桥接。
// Issue #1034 评论 #5/#6/#7/#8/#10/#11/#12/#13 修复历史见 git log。
// Issue #1034 评论 #14 修复：
//   - enqueuePendingEdit 只追加 pendingEdits 并唤醒唯一消费者 drainPendingEdits
//   - drainPendingEdits 消费者协程：队首处理、每笔 rebase、失败不放行下一笔
//   - sessionId 检查在 await 前后都做，防止切章后旧任务修改新会话状态
//   - 移除 scheduleRecovery/recoverWithPendingEdits，逻辑合并到 drainPendingEdits
//
// 职责：
// - RichEditor onWillChange → 只做校验（返回 true 允许改变），不调用 coordinator。
// - RichEditor aboutToIMEInput → 读取 previewText 判断是否预编辑开始，首次冻结基线。
// - RichEditor onDidChange → 从前后文本差量计算编辑事实，
//   IME 预编辑期间只更新本地镜像不提交 Core；非预编辑期间串行提交给 coordinator。
// - RichEditor onDidIMEInput → 通过 getPreviewText() 确认预上屏消失后才提交差量，
//   封装为 PendingEdit(cause=TypingCommit) 统一排队逐笔确认及失败重放。
// - whenIdle 为事件驱动队列稳定屏障：只等已提交队列事务，不调用 stopEditing()。
// - finishEditingAndWhenIdle 为退出/同步屏障：先 stopEditing() 结束预编辑，再等队列稳定。
// - checkIdle 只观察明确完成的状态，不自行把 imeComposing 改成 false。
// - drainPendingEdits 为唯一消费者协程：从队首处理，每笔执行前检查 snap.text 与
//   edit.beforeText 的关联并 rebase，失败设置 idleError 不放行下一笔。
// - 真实内容冲突无法映射时保留 pending 内容和错误状态，whenIdle 报告失败。
// - syncFromCore 有 pendingEdits/imeComposing/idleError 时不覆盖 RichEditor。
// - 切章时以 session ID 隔离旧回调，旧队列不可触碰新章节 UI。

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
  private drainRunning: boolean = false
  private sessionId: number = 0
  private localSequence: number = 0
  private confirmedText: string = ''
  private confirmedRevision: number = -1
  private idleError: string | null = null
  private idleWaiters: IdleWaiter[] = []
  // Issue #1034 评论 #18：选区 latest-wins 合并上报
  private pendingSelectionIntent: SelectionIntent | null = null
  private selectionFlushTimerId: number = -1

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

  // 普通保存屏障：只等已提交队列事务，不调用 stopEditing()。
  // imeComposing 时不 resolve，等待 onDidIMEInput 将 imeComposing 设为 false 后再继续。
  // Issue #1034 评论 #18/#19：先 flush pending 选区意图，确保保存取得的 snapshot 与用户最后可见选区一致。
  whenIdle(): Promise<void> {
    this.requestSelectionFlush()
    return new Promise<void>((resolve, reject) => {
      this.idleWaiters.push({ sessionId: this.sessionId, resolve: resolve, reject: reject })
      this.checkIdle()
    })
  }

  // 退出章节/同步屏障：先 stopEditing() 结束预编辑，再等队列稳定。
  // Issue #1034 评论 #18/#19：先 flush pending 选区意图，确保退出/同步取得的 snapshot 与用户最后可见选区一致。
  finishEditingAndWhenIdle(): Promise<void> {
    if (this.imeComposing && this.richEditorController !== null) {
      this.richEditorController.stopEditing()
    }
    this.requestSelectionFlush()
    return new Promise<void>((resolve, reject) => {
      this.idleWaiters.push({ sessionId: this.sessionId, resolve: resolve, reject: reject })
      this.checkIdle()
    })
  }

  // 事件驱动检查：先查 idleError，再等 editQueue tail，条件不满足时等待状态变化通知。
  // 不自旋等待已 resolved 的 Promise；由各完成路径调用 checkIdle 触发重新检查。
  // 不自行修改 imeComposing；状态转换集中在 onDidIMEInput 统一处理。
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
        return
      }

      // Issue #1034 评论 #18/#19：有 pending 选区意图或选区消费者在运行时不算空闲
      if (this.pendingSelectionIntent !== null) {
        this.requestSelectionFlush()
        return
      }
      if (this.selectionDrainRunning) {
        return
      }

      const currentSession = this.sessionId
      const resolved: IdleWaiter[] = []
      const remaining: IdleWaiter[] = []
      for (const w of this.idleWaiters) {
        if (w.sessionId === currentSession) {
          resolved.push(w)
        } else {
          remaining.push(w)
        }
      }
      this.idleWaiters = remaining
      for (const w of resolved) {
        w.resolve()
      }
    })
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
    this.drainRunning = false
    this.localSequence = 0
    // Issue #1034 评论 #18/#19：取消旧 session 的选区防抖定时器和意图
    if (this.selectionFlushTimerId !== -1) {
      clearTimeout(this.selectionFlushTimerId)
      this.selectionFlushTimerId = -1
    }
    this.pendingSelectionIntent = null
    this.selectionDrainRunning = false
  }

  onWillChange(_value: RichEditorChangeValue): boolean {
    return true
  }

  // onDidChange：内容已改变后，从 rangeBefore/rangeAfter 和本地镜像计算编辑事实，
  // 串行提交给 coordinator。IME 预编辑期间只更新本地镜像，不提交 Core 事务。
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
    this.enqueuePendingEdit(pendingEdit)
  }

  // aboutToIMEInput 读取 previewText 判断是否预编辑开始。
  aboutToIMEInput(insertValue: RichEditorInsertValue): boolean {
    if (insertValue.previewText !== undefined && insertValue.previewText.length > 0) {
      if (!this.imeComposing) {
        this.imeComposing = true
        this.imeComposingStartText = this.currentText
      }
    }
    return true
  }

  // onDidIMEInput 通过 getPreviewText() 确认预上屏消失后才提交差量。
  // 封装为 PendingEdit(cause=TypingCommit) 统一排队逐笔确认及失败重放。
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
      this.checkIdle()
      return
    }

    const baselineText = this.imeComposingStartText
    const diffOldText = baselineText.substring(diff.start, diff.end)

    const pendingEdit: PendingEdit = {
      sessionId: this.sessionId,
      localSequence: this.localSequence++,
      baseRevision: this.lastAppliedRevision,
      beforeText: baselineText,
      startUtf16: diff.start,
      endUtf16: diff.end,
      insertedText: diff.replacement,
      oldText: diffOldText,
      cause: EditorTransactionCause.TypingCommit
    }
    this.enqueuePendingEdit(pendingEdit)
  }

  // RichEditor onSelectionChange 回调：用 latest-wins 合并上报，防抖 30ms。
  // Issue #1034 评论 #18：高频选区变化（拖动手柄）只记录最新意图，
  // 不为每次事件追加 editQueue.then 和 Core 命令。
  // 防抖到期或 whenIdle 主动调用时才提交最后一笔选区到 Core。
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
      textForConversion: this.currentText
    }

    if (this.selectionFlushTimerId !== -1) {
      clearTimeout(this.selectionFlushTimerId)
    }
    this.selectionFlushTimerId = setTimeout(() => {
      this.selectionFlushTimerId = -1
      this.requestSelectionFlush()
    }, 30)
  }

  // Issue #1034 评论 #19：唤醒选区单飞消费者。如果消费者已在运行则不重复追加 editQueue.then，
  // 后续事件只更新 pendingSelectionIntent，消费者会在当前循环中处理最新意图。
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
      }
      if (sessionToken === this.sessionId) {
        this.selectionDrainRunning = false
      }
      this.checkIdle()
    }).catch(e => {
      hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, `requestSelectionFlush error: ${JSON.stringify(e)}`)
      if (sessionToken === this.sessionId) {
        this.selectionDrainRunning = false
      }
      this.checkIdle()
    })
  }

  // Issue #1034 评论 #19：选区单飞消费者协程。
  // 从 pendingSelectionIntent 拿最新意图，验证正文基线后提交 Core；
  // 提交完后若槽位又变更，继续按最新值处理；旧意图在尚未送入 Core 前直接被覆盖。
  // 正文基线验证：intent.textForConversion 必须与当前 this.currentText 一致，
  // 否则选区坐标已过期（RichEditor 正文已变），跳过这笔选区。
  private async drainSelectionIntent(sessionToken: number): Promise<void> {
    while (true) {
      if (sessionToken !== this.sessionId) {
        return
      }

      const intent = this.pendingSelectionIntent
      if (intent === null) {
        return
      }
      this.pendingSelectionIntent = null

      if (intent.localSessionToken !== this.sessionId) {
        continue
      }
      if (this.coordinator.getSessionId() !== intent.expectedCoreSessionId) {
        continue
      }

      // Issue #1034 评论 #19：验证正文基线——选区 UTF-16 坐标基于 intent.textForConversion，
      // 若 RichEditor 正文已变（this.currentText !== intent.textForConversion），
      // 旧坐标可能指向错误位置，跳过这笔选区。
      if (intent.textForConversion !== this.currentText) {
        hilog.warn(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, 'drainSelectionIntent: text baseline changed, skipping stale selection')
        continue
      }

      const byteAnchor = TextOffsetMapper.utf16ToUtf8(intent.textForConversion, intent.startUtf16)
      const byteHead = TextOffsetMapper.utf16ToUtf8(intent.textForConversion, intent.endUtf16)

      await this.coordinator.setSelectionWithExpectedSession(byteAnchor, byteHead, intent.expectedCoreSessionId)

      if (sessionToken !== this.sessionId) {
        return
      }
    }
  }

  // 只将不可变编辑追加到 pendingEdits 并唤醒唯一消费者 drainPendingEdits。
  // 不为每个 edit 单独排一个能绕过失败的 editQueue.then。
  private enqueuePendingEdit(pendingEdit: PendingEdit): void {
    this.pendingEdits.push(pendingEdit)
    this.wakeDrain()
  }

  // 唤醒唯一消费者协程。如果消费者已在运行则不重复唤醒。
  // 消费者带本地不可变的 sessionToken，旧消费者完成后不改新 session 的 drainRunning。
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
    }).catch(e => {
      hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, `wakeDrain error: ${JSON.stringify(e)}`)
      if (sessionToken === this.sessionId) {
        this.drainRunning = false
        this.idleError = `Unexpected error in wakeDrain: ${JSON.stringify(e)}`
      }
      this.checkIdle()
    })
  }

  // 唯一消费者协程：从队首处理，单笔事务确定 Applied 后才取下一笔。
  // 每笔执行前检查 snap.text 与 edit.beforeText 的关联并 rebase。
  // 失败设置 idleError，不从队列移除，不放行下一笔。
  // sessionId 检查在 await 前后都做，防止切章后旧任务修改新会话状态。
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

      const byteStart = TextOffsetMapper.utf16ToUtf8(baselineText, mappedStart)
      const byteEnd = TextOffsetMapper.utf16ToUtf8(baselineText, mappedEnd)

      let result: ResultEnvelope<EditorEditResult> | null = null
      if (edit.oldText.length > 0 && edit.insertedText.length > 0) {
        result = await this.coordinator.replace(byteStart, byteEnd, edit.insertedText, edit.oldText, edit.cause)
      } else if (edit.oldText.length > 0) {
        result = await this.coordinator.delete(byteStart, byteEnd, EditorTransactionCause.Delete)
      } else if (edit.insertedText.length > 0) {
        result = await this.coordinator.insert(byteStart, edit.insertedText, edit.cause)
      }

      // await 后再次检查 sessionId，防止切章后旧任务修改新会话状态
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

  // 计算最小文本差量（按完整 Unicode code point 求前后缀）
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

  // 将 edit 的偏移从 edit.beforeText 映射到 baselineText。
  private mapEditToBaseline(edit: PendingEdit, baselineText: string): { startUtf16: number, endUtf16: number } | null {
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

  // 从 Core snapshot 恢复：事务失败或需要强制对齐时调用。
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

  // 程序回放：从 Core snapshot 同步正文到 RichEditor。
  // Issue #1034 评论 #16：仅当正文实际改变时更新文本，不主动 setSelection，
  // 避免系统选区经 Core 异步回写形成闭环，干扰原生手柄拖选。
  // 外部明确要求定位选区的场景（跳转纠错/搜索结果定位）走 syncSelectionFromCore()。
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

    const snap = this.coordinator.getSnapshot()
    if (snap === null || this.richEditorController === null) {
      return
    }

    if (this.currentText === snap.text) {
      this.lastAppliedRevision = snap.revision
      this.confirmedText = snap.text
      this.confirmedRevision = snap.revision
      return
    }

    this.programmaticUpdate = true
    try {
      const diff = this.computeDiff(this.currentText, snap.text)
      if (diff.start < diff.end || diff.replacement.length > 0) {
        this.richEditorController.deleteSpans({ start: diff.start, end: diff.end })
        if (diff.replacement.length > 0) {
          this.richEditorController.addTextSpan(diff.replacement, { offset: diff.start })
        }
      }
      this.currentText = snap.text

      this.lastAppliedRevision = snap.revision
      this.confirmedText = snap.text
      this.confirmedRevision = snap.revision
    } catch (e) {
      hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, `syncFromCore error: ${JSON.stringify(e)}`)
    } finally {
      this.programmaticUpdate = false
    }
  }

  // Issue #1034 评论 #16：外部明确要求定位选区的场景（跳转纠错/搜索结果定位）。
  // 从 Core snapshot 读取选区并设置到 RichEditor，不修改正文。
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

    this.programmaticUpdate = true
    try {
      this.richEditorController.deleteSpans()
      this.richEditorController.addTextSpan(text, { offset: 0 })
      this.currentText = text

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
  }
}
