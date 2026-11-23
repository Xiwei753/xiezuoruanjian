// NativeRichEditorBridge — RichEditor 与 EditorSessionCoordinator 之间的输入桥接。
// Issue #1034 评论 #5/#6/#7/#8/#10/#11/#12/#13 修复历史见 git log。
// Issue #1053：明确「用户输入」与「原生格式化修改」的边界：
//   - onDidChange 按 rangeBefore/rangeAfter 的实际 offset 严格重建差量；原生事实不一致时
//     用原生全文 Span 重建镜像并记录事实错误，不静默漏掉换行/截断段落。
//   - onDidChange/onDidIMEInput/syncFromCore/loadInitialText 记录操作范围、长度、Span 数、
//     IME 与程序性状态（不含正文原文）。
//   - whenIdle/finishEditingAndWhenIdle 保存/退出屏障对齐原生全文镜像与 Core 正文的
//     长度、换行数与 hash，并记录差异 offset。
//   - syncFromCore/用户编辑改变段落结构时通知组件重新应用原生段落样式。
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
// 组件据此重新应用原生段落样式；纯选区/光标变化不会触发全文重新排版。
export interface NativeEditorStateEvent {
  kind: string
  paragraphsChanged: boolean
}
export type NativeEditorStateListener = (event: NativeEditorStateEvent) => void

// Issue #1053 评论 7098652618：保存/退出屏障的三方校验结果。
// 放行条件是「原生组件实际全文 === 本地镜像 === Core snapshot.text」，
// 任何一方不一致都不能 resolve 等待者。
export interface NativeCoreAlignmentResult {
  stage: string
  matched: boolean               // 三方文本完全一致
  coreAvailable: boolean         // 是否有 Core snapshot
  nativeAvailable: boolean       // readNativeFullText 是否成功
  mirrorMatchesEditor: boolean   // 本地镜像 === 原生全文
  nativeText: string             // 原生全文（仅用于本会话收敛，不落诊断）
  nativeUtf16Length: number
  coreUtf16Length: number
  nativeSpanCount: number        // 原生全文 Span 总数（完整读取时统计）
  nativeRevision: number
  firstMismatchUtf16: number     // 镜像与原生全文的首个差异；一致为 -1
  mismatchToCoreUtf16: number    // 镜像与 Core 正文的首个差异；一致为 -1
  appliedRevision: number        // 原生镜像最后一次成功应用的 Core revision
  coreRevision: number           // 当前 Core snapshot revision
  coreAdvanced: boolean          // Core 已由其它编辑路径推进（原生侧没有未结算编辑）
  detail: string
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

// 一次原生 Range 文本提取的事实（不含正文原文记录，仅用于本次差量重建）。
interface NativeRangeText {
  text: string
  spanCount: number
  nonTextSpanCount: number
  startUtf16: number
  endUtf16: number
}

// IME 提交后的范围签名：用于忽略同一次提交随后重复触发的 onDidChange。
interface ImeCommitSignature {
  start: number
  end: number
  insertedText: string
}

// 记录一次原生文本操作所需的字段（补 coreRevision/atMs 后落到快照）。
interface NativeOperationFacts {
  kind: string
  beforeUtf16Length: number
  afterUtf16Length: number
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
  // Issue #1034 评论 #31：本意图上次 StaleRevision 失败时的 Core revision，
  // null 表示全新意图无失败历史；同一意图同一 revision 重复失败才报告错误
  retriedAtRevision: number | null
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
  // Issue #1034 评论 #18/#19：选区 latest-wins 合并上报
  private pendingSelectionIntent: SelectionIntent | null = null
  private selectionFlushTimerId: number = -1
  // Issue #1034 评论 #19：选区%单飞消费者标记，防止重复追加 editQueue.then
  private selectionDrainRunning: boolean = false

  // Issue #1053：原生编辑事实与运行时诊断。只记录范围/长度，不保存正文原文。
  private lastNativeOperation: NativeTextOperationSnapshot | null = null
  private lastNativeCoreAlignment: NativeCoreAlignmentSnapshot | null = null
  private nativeFactError: string | null = null
  // 屏障发现「原生正文有效但与 Core 不一致」：阻止 syncFromCore 异步回写覆盖原生内容。
  private nativeCoreDiverged: boolean = false
  // 记录分歧发生时的 Core 会话；切到新会话后分歧不再适用，允许正常回填。
  private nativeCoreDivergedSessionId: number | null = null
  private stateListener: NativeEditorStateListener | null = null
  // 保存/退出屏障的阶段名，供 checkIdle 记录对齐事实。
  private barrierStage: string = 'save-when-idle'
  // Issue #1053 评论 7098652618：原生→Core 收敛事务单飞标记与尝试次数上限。
  private nativeCoreConvergenceRunning: boolean = false
  private static readonly NATIVE_CORE_CONVERGENCE_MAX_ATTEMPTS: number = 3
  // 原生编辑器是否已加载过 Core 正文。未加载时 currentText 只是空占位，
  // 不能把它当成「原生正文与 Core 不一致」去收敛，否则会把 Core 正文清空。
  private nativeTextReady: boolean = false
  // Core 先行推进时刷新原生镜像的单飞标记，避免 checkIdle 重入。
  private nativeCoreRefreshRunning: boolean = false
  // Issue #1053 评论 7099290808：换行数按编辑增量维护，快照不再每 150ms 遍历整章。
  private nativeNewlineCount: number = 0
  private coreNewlineCount: number = 0
  // 最近一次 readNativeFullText 统计到的 Span 总数（-1 表示未知）。
  private lastReadNativeSpanCount: number = -1
  // IME 预编辑开始时冻结的替换范围（UTF-16，基线坐标）与基线换行数。
  private imeReplaceStartUtf16: number = -1
  private imeReplaceEndUtf16: number = -1
  private imeComposingStartNewlineCount: number = 0
  // 预编辑期间第一次 onDidChange 的 rangeBefore（镜像坐标，即预编辑基线坐标），
  // 是组件真正替换的范围；优先于 aboutToIMEInput 时的选区快照。
  private imeRangeBeforeStartUtf16: number = -1
  private imeRangeBeforeEndUtf16: number = -1
  private imeRangeBeforeRecorded: boolean = false
  // 上一次 IME 提交的范围签名，供 onDidChange 去重。
  private lastImeCommit: ImeCommitSignature | null = null

  constructor(coordinator: EditorSessionCoordinator) {
    this.coordinator = coordinator
  }

  setController(controller: RichEditorController): void {
    this.richEditorController = controller
  }

  // Issue #1053：组件订阅原生编辑器状态变化（正文结算/段落结构变化）。
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

  // Issue #1053 评论 7099290808：增量维护的换行数，供诊断快照按事件读取（O(1)）。
  getNativeNewlineCount(): number {
    return this.nativeNewlineCount
  }

  getCoreNewlineCount(): number {
    return this.coreNewlineCount
  }

  getNativeSpanCount(): number {
    return this.lastReadNativeSpanCount
  }

  // 非热路径：在失焦/退出/导出前刷新一次完整事实（全文 Span 数、hash 与三方对齐）。
  captureDiagnosticsFacts(): void {
    this.validateNativeCoreAlignment('diagnostics')
  }

  isNativeCoreDiverged(): boolean {
    return this.nativeCoreDiverged
  }

  // 普通保存屏障：只等已提交队列事务，不调用 stopEditing()。
  // imeComposing 时不 resolve，等待 onDidIMEInput 将 imeComposing 设为 false 后再继续。
  // Issue #1034 评论 #18/#19：先 flush pending 选区意图，确保保存取得的 snapshot 与用户最后可见选区一致。
  // Issue #1053：屏障放行前记录原生全文镜像与 Core 正文的长度/换行/hash 对齐事实。
  whenIdle(): Promise<void> {
    this.barrierStage = 'save-when-idle'
    this.requestSelectionFlush()
    return new Promise<void>((resolve, reject) => {
      this.idleWaiters.push({ sessionId: this.sessionId, resolve: resolve, reject: reject })
      this.checkIdle()
    })
  }

  // 退出章节/同步屏障：先 stopEditing() 结束预编辑，再等队列稳定。
  // Issue #1034 评论 #18/#19：先 flush pending 选区意图，确保退出/同步取得的 snapshot 与用户最后可见选区一致。
  finishEditingAndWhenIdle(): Promise<void> {
    this.barrierStage = 'exit-finish-editing'
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

      // Issue #1034 评论 #18/#19/#22：有 pending 选区意图时检查等待条件，
      // 不无条件 requestSelectionFlush 避免无限循环。
      if (this.pendingSelectionIntent !== null) {
        if (this.imeComposing) {
          return
        }
        if (this.pendingEdits.length > 0 || this.drainRunning) {
          return
        }
        if (this.selectionDrainRunning) {
          return
        }
        const snap = this.coordinator.getSnapshot()
        if (snap !== null && snap.text !== this.currentText) {
          // Issue #1053 评论 7098652618：Core 落后于原生正文时，基于旧坐标的选区意图不再可靠；
          // 丢弃它，交给下方收敛事务对齐正文后再按原生选区重新上报。
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

      // Issue #1053 评论 7098652618：只有原生全文、本地镜像、Core 正文三方一致才放行。
      const alignment = this.validateNativeCoreAlignment(this.barrierStage)
      const nativeEditorLoaded = this.richEditorController !== null && this.nativeTextReady
      if (alignment.matched || !alignment.coreAvailable || !nativeEditorLoaded) {
        // 没有 Core 会话或原生编辑器尚未加载正文：没有可比对的原生编辑事实，保持原关闭语义；
        // 其它情况必须三方一致，不能把旧 Core 快照当最新正文放行。
        this.idleWaiters = remaining
        for (const w of currentWaiters) {
          w.resolve()
        }
        return
      }

      // Core 已由其它编辑路径推进（原生没有未结算编辑）：先把原生镜像刷新到 Core，
      // 再次校验三方一致后才放行；不把过期镜像反向收敛进 Core。
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
        // 镜像落后于原生全文：以原生最终文本恢复镜像，再由收敛事务对齐 Core。
        this.reportNativeFactError('barrier',
          `mirror diverged from native text firstMismatch=${alignment.firstMismatchUtf16}`)
        this.currentText = alignment.nativeText
        this.nativeNewlineCount = countNewlines(alignment.nativeText)
      }
      void this.convergeNativeTextToCore()
    })
  }

  // Core 先行推进时刷新原生正文，刷新后由 checkIdle 重新做三方校验；
  // 刷新后仍不一致（原生组件不接受程序性写入）则按失败处理，不放行保存。
  private async refreshNativeMirrorFromCore(): Promise<void> {
    const sessionAtEntry = this.sessionId
    try {
      // Core 是业务事实来源：清除反向分歧保护，允许 syncFromCore 覆盖原生镜像。
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

  // Issue #1053 评论 7098652618：原生正文 → Core 已提交正文的收敛事务。
  // 编辑事实已结算但 Core 落后于原生可见正文时，按最小差量只提交实际改变区间；
  // coordinator 在 commandQueue 出队闭包内复核 session/revision/text，
  // 失败时重新读取新基线再映射，不对整章盲目覆盖。
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
        // 新编辑事实仍在结算：交给 drain/IME 处理完成后再由 checkIdle 重新评估。
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
          // Core 在收敛前被其它路径推进：交给 checkIdle 的 coreAdvanced 分支刷新原生。
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
          // Core 已提交但文本仍未完全相等：重新读取基线再映射。
          continue
        }
        if (result.errorCode === 'SESSION_CHANGED') {
          return
        }
        // STALE_BASELINE 等失败：重新读取新基线再映射。
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

  // 收敛失败：保留原生正文与编辑状态，向当前会话的等待者返回真实错误，
  // 让保存/同步/关闭按失败处理，而不是落盘旧 Core 正文。
  private failNativeCoreAlignment(detail: string): void {
    // 不写 idleError：本次屏障失败不应毒化后续输入队列；保留原生编辑状态，
    // 用户继续输入仍按正常路径结算，下次保存/退出屏障会重新尝试对齐。
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
    this.drainRunning = false
    this.localSequence = 0
    // Issue #1034 评论 #18/#19：取消旧 session 的选区防抖定时器和意图
    if (this.selectionFlushTimerId !== -1) {
      clearTimeout(this.selectionFlushTimerId)
      this.selectionFlushTimerId = -1
    }
    this.pendingSelectionIntent = null
    this.selectionDrainRunning = false
    // Issue #1053：切章后旧的编辑事实不再对应当前会话。
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
    this.imeReplaceStartUtf16 = -1
    this.imeReplaceEndUtf16 = -1
    this.imeComposingStartNewlineCount = 0
    this.imeRangeBeforeStartUtf16 = -1
    this.imeRangeBeforeEndUtf16 = -1
    this.imeRangeBeforeRecorded = false
    this.lastImeCommit = null
  }

  onWillChange(_value: RichEditorChangeValue): boolean {
    return true
  }

  // onDidChange：内容已改变后，按 rangeBefore/rangeAfter 的实际 offset 严格重建编辑事实。
  // 先验证原生 Range 与本地镜像一致；不一致时用原生全文 Span 重建镜像并记录事实错误，
  // 不静默漏掉换行、截断段落或把错误镜像提交 Core。
  // IME 预编辑期间 rangeAfter 可能包含尚未落到 Span 的预上屏文本，只更新本地镜像，
  // 不提交 Core；提交差量由 onDidIMEInput 用提交前后的镜像计算。
  onDidChange(rangeBefore: TextRange, rangeAfter: TextRange): void {
    if (this.programmaticUpdate || this.richEditorController === null) {
      return
    }

    if (this.imeComposing) {
      this.applyImeCompositionChange(rangeBefore, rangeAfter)
      return
    }

    // Issue #1053 评论 7099290808：输入热路径只提取 rangeAfter 的局部片段，
    // 只有 Range 缺失/不一致、非文本 Span、原生异常等条件出现时才读取全文重建。
    const previousText = this.currentText
    const imeCommitSignature = this.lastImeCommit
    this.lastImeCommit = null
    if (imeCommitSignature !== null && this.isImeCommitDuplicate(imeCommitSignature, rangeAfter)) {
      // 同一次 IME 提交随后重复触发的 onDidChange：已由 onDidIMEInput 结算，忽略。
      return
    }

    const beforeRange = this.resolveTextRange(rangeBefore, previousText.length)
    const rangeText = this.extractRangeText(rangeAfter)

    let factDetail = ''
    if (beforeRange === null) {
      const rawStart = rangeBefore.start ?? 0
      const rawEnd = rangeBefore.end ?? previousText.length
      factDetail = `rangeBefore out of bounds start=${rawStart} end=${rawEnd} beforeLen=${previousText.length}`
    }
    if (rangeText === null) {
      if (factDetail.length > 0) {
        factDetail += '; '
      }
      factDetail += `rangeAfter extraction failed start=${rangeAfter.start ?? 0} end=${rangeAfter.end ?? -1}`
    } else {
      if (rangeText.nonTextSpanCount > 0) {
        if (factDetail.length > 0) {
          factDetail += '; '
        }
        factDetail += `rangeAfter contains ${rangeText.nonTextSpanCount} non-text span(s)`
      }
      const declaredLength = rangeText.endUtf16 - rangeText.startUtf16
      if (declaredLength !== rangeText.text.length) {
        if (factDetail.length > 0) {
          factDetail += '; '
        }
        factDetail += `rangeAfter declares ${declaredLength} utf16 units but spans yield ${rangeText.text.length}`
      }
    }

    const rawBeforeStart = rangeBefore.start ?? 0
    const rawBeforeEnd = rangeBefore.end ?? previousText.length
    const rawAfterStart = rangeAfter.start ?? 0
    const rawAfterEnd = rangeAfter.end ?? (rangeText !== null ? rangeText.endUtf16 : -1)

    let factError = factDetail.length > 0
    let startUtf16 = 0
    let endUtf16 = 0
    let insertedText = ''
    let oldText = ''

    if (!factError && beforeRange !== null && rangeText !== null) {
      startUtf16 = beforeRange.startUtf16
      endUtf16 = beforeRange.endUtf16
      insertedText = rangeText.text
      oldText = previousText.substring(startUtf16, endUtf16)
      this.currentText = previousText.substring(0, startUtf16)
        + insertedText
        + previousText.substring(endUtf16)
      this.updateNativeNewlineDelta(oldText, insertedText)
    } else {
      // 异常路径：以原生全文 Span 为真相完整重建（不进入逐键热路径）。
      const nativeFullText = this.readNativeFullText()
      if (nativeFullText === null) {
        this.reportNativeFactError('onDidChange', `${factDetail}; native full text unavailable`)
        this.idleError = `Native text fact mismatch and native full text unavailable: ${factDetail}`
        hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, this.idleError)
        this.rejectAllWaiters()
        return
      }
      if (nativeFullText === previousText) {
        // 原生正文没有净变化（例如 IME 提交已先行结算后的重复回调）：不提交 Core、不报持久错误。
        hilog.warn(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG,
          `onDidChange reported range but native text is unchanged: ${factDetail}`)
        startUtf16 = previousText.length
        endUtf16 = previousText.length
      } else {
        const recovered = this.computeDiff(previousText, nativeFullText)
        startUtf16 = recovered.start
        endUtf16 = recovered.end
        insertedText = recovered.replacement
        oldText = previousText.substring(startUtf16, endUtf16)
        this.currentText = nativeFullText
        this.nativeNewlineCount = countNewlines(nativeFullText)
        this.reportNativeFactError('onDidChange',
          `${factDetail}; recovered from native full text start=${startUtf16} end=${endUtf16} insertedLen=${insertedText.length}`)
      }
    }

    const paragraphsChanged = insertedText.indexOf('\n') >= 0 || oldText.indexOf('\n') >= 0

    this.recordNativeOperation({
      kind: this.pendingPaste ? 'paste' : 'typing',
      beforeUtf16Length: previousText.length,
      afterUtf16Length: this.currentText.length,
      rangeBeforeStartUtf16: rawBeforeStart,
      rangeBeforeEndUtf16: rawBeforeEnd,
      rangeAfterStartUtf16: rawAfterStart,
      rangeAfterEndUtf16: rawAfterEnd,
      startUtf16: startUtf16,
      endUtf16: endUtf16,
      replacedUtf16Length: endUtf16 - startUtf16,
      insertedUtf16Length: insertedText.length,
      spanCountInRange: rangeText !== null ? rangeText.spanCount : 0,
      nonTextSpanCount: rangeText !== null ? rangeText.nonTextSpanCount : 0,
      imeComposition: false,
      programmatic: false,
      factError: factError
    })

    if (startUtf16 >= endUtf16 && insertedText.length === 0) {
      // 原生只报告了空编辑（纯样式/重复回调/无文本变化）：不提交 Core。
      this.pendingPaste = false
      this.notifyStateListener('native-change', paragraphsChanged)
      return
    }

    const cause: EditorTransactionCause = this.pendingPaste ? EditorTransactionCause.Paste : EditorTransactionCause.Typing
    this.pendingPaste = false

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

  // IME 提交去重：同一提交的 range 与片段已在 onDidIMEInput 中结算，重复的 onDidChange 不再二次应用。
  private isImeCommitDuplicate(signature: ImeCommitSignature, rangeAfter: TextRange): boolean {
    if (signature.insertedText.length === 0) {
      return false
    }
    const afterStart = rangeAfter.start ?? -1
    // 缺省 end 按 TextRange 语义视为文本末尾；提交回调通常给出明确范围，
    // 这里用提交签名兜底，最终仍以镜像中是否已存在同一片段为准。
    const afterEnd = rangeAfter.end ?? signature.end
    if (afterStart !== signature.start || afterEnd !== signature.end) {
      return false
    }
    return this.currentText.substring(signature.start, signature.end) === signature.insertedText
  }

  // 按编辑增量维护换行数（O(插入/删除片段长度)，不遍历整章）。
  private updateNativeNewlineDelta(oldSlice: string, newSlice: string): void {
    this.nativeNewlineCount = Math.max(0,
      this.nativeNewlineCount + countNewlines(newSlice) - countNewlines(oldSlice))
  }

  // IME 预编辑期间的镜像更新：rangeAfter 的 Span 文本可能少于声明范围（预上屏未落到 Span）。
  // Issue #1053 评论 7099290808：预上屏不是已提交正文，取不到文本时保持镜像；
  // 提交后的最终正文由 onDidIMEInput 按本次输入 Range 片段 + 预编辑基线重建。
  private applyImeCompositionChange(rangeBefore: TextRange, rangeAfter: TextRange): void {
    const previousText = this.currentText
    const rangeText = this.extractRangeText(rangeAfter)
    const rawStart = rangeBefore.start ?? 0
    const rawEnd = rangeBefore.end ?? rawStart
    const start = Math.max(0, Math.min(rawStart, previousText.length))
    const end = Math.max(start, Math.min(rawEnd, previousText.length))
    const declaredLength = Math.max(0, (rangeAfter.end ?? start) - (rangeAfter.start ?? start))

    if (!this.imeRangeBeforeRecorded) {
      // 第一次预编辑变化：rangeBefore 是组件实际替换的范围（镜像此时仍是预编辑基线）。
      this.imeRangeBeforeRecorded = true
      this.imeRangeBeforeStartUtf16 = start
      this.imeRangeBeforeEndUtf16 = end
    }

    if (rangeText === null || (rangeText.text.length === 0 && declaredLength > 0)) {
      // 预上屏文字尚未落到 Span：只记录事实，镜像保持提交前正文。
      hilog.warn(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG,
        `IME preedit text unavailable start=${rawStart} end=${rawEnd} declared=${declaredLength}`)
      this.recordNativeOperation({
        kind: 'typing',
        beforeUtf16Length: previousText.length,
        afterUtf16Length: previousText.length,
        rangeBeforeStartUtf16: rawStart,
        rangeBeforeEndUtf16: rawEnd,
        rangeAfterStartUtf16: rangeAfter.start ?? 0,
        rangeAfterEndUtf16: rangeAfter.end ?? -1,
        startUtf16: start,
        endUtf16: end,
        replacedUtf16Length: 0,
        insertedUtf16Length: 0,
        spanCountInRange: rangeText !== null ? rangeText.spanCount : 0,
        nonTextSpanCount: rangeText !== null ? rangeText.nonTextSpanCount : 0,
        imeComposition: true,
        programmatic: false,
        factError: true
      })
      this.notifyStateListener('native-change', false)
      return
    }

    const insertedText = rangeText.text
    const replacedText = previousText.substring(start, end)
    this.currentText = previousText.substring(0, start) + insertedText + previousText.substring(end)
    this.updateNativeNewlineDelta(replacedText, insertedText)

    const paragraphsChanged = insertedText.indexOf('\n') >= 0 || replacedText.indexOf('\n') >= 0

    this.recordNativeOperation({
      kind: 'typing',
      beforeUtf16Length: previousText.length,
      afterUtf16Length: this.currentText.length,
      rangeBeforeStartUtf16: rawStart,
      rangeBeforeEndUtf16: rawEnd,
      rangeAfterStartUtf16: rangeAfter.start ?? 0,
      rangeAfterEndUtf16: rangeAfter.end ?? (rangeText.endUtf16),
      startUtf16: start,
      endUtf16: end,
      replacedUtf16Length: end - start,
      insertedUtf16Length: insertedText.length,
      spanCountInRange: rangeText.spanCount,
      nonTextSpanCount: rangeText.nonTextSpanCount,
      imeComposition: true,
      programmatic: false,
      factError: false
    })

    this.notifyStateListener('native-change', paragraphsChanged)
  }

  // aboutToIMEInput 读取 previewText 判断是否预编辑开始。
  // Issue #1053 评论 7099290808：冻结基线时同时记录预编辑替换范围（选区/插入点），
  // 供 onDidIMEInput 用本次输入 Range 片段重建最终正文，避免每次提交读取全文。
  aboutToIMEInput(insertValue: RichEditorInsertValue): boolean {
    if (insertValue.previewText !== undefined && insertValue.previewText.length > 0) {
      if (!this.imeComposing) {
        this.imeComposing = true
        this.imeComposingStartText = this.currentText
        this.imeComposingStartNewlineCount = this.nativeNewlineCount
        this.imeReplaceStartUtf16 = -1
        this.imeReplaceEndUtf16 = -1
        this.imeRangeBeforeStartUtf16 = -1
        this.imeRangeBeforeEndUtf16 = -1
        this.imeRangeBeforeRecorded = false
        if (this.richEditorController !== null) {
          try {
            const selection = this.richEditorController.getSelection()
            if (selection !== undefined && selection !== null && selection.selection.length >= 2) {
              const first = selection.selection[0]
              const second = selection.selection[1]
              const selectionStart = Math.min(first, second)
              const selectionEnd = Math.max(first, second)
              if (selectionStart >= 0 && selectionEnd <= this.imeComposingStartText.length) {
                this.imeReplaceStartUtf16 = selectionStart
                this.imeReplaceEndUtf16 = selectionEnd
              }
            }
          } catch (e) {
            hilog.warn(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, `aboutToIMEInput getSelection error: ${JSON.stringify(e)}`)
          }
        }
        if (this.imeReplaceStartUtf16 < 0) {
          const insertOffset = insertValue.insertOffset
          const offset = Math.max(0, Math.min(insertOffset, this.imeComposingStartText.length))
          this.imeReplaceStartUtf16 = offset
          this.imeReplaceEndUtf16 = offset
        }
      }
    }
    return true
  }

  // onDidIMEInput：回调 TextRange 就是「输入法本次输入内容的范围」。
  // 用该范围提取最终提交片段，结合预编辑开始时冻结的基线与替换范围重建最终正文；
  // 只有片段事实无法重建时才完整读取原生全文恢复。预编辑阶段不向 Core 发送半成品。
  onDidIMEInput(range: TextRange): void {
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

    const baselineText = this.imeComposingStartText
    const previousMirror = this.currentText
    // 优先使用预编辑第一次 onDidChange 的 rangeBefore（组件实际替换范围），
    // 没有则退回 aboutToIMEInput 冻结的选区/插入点。
    const replacementStartHint = this.imeRangeBeforeStartUtf16 >= 0
      ? this.imeRangeBeforeStartUtf16
      : this.imeReplaceStartUtf16
    const replacementEndHint = this.imeRangeBeforeEndUtf16 >= 0
      ? this.imeRangeBeforeEndUtf16
      : this.imeReplaceEndUtf16
    const hasReplacementHint = replacementStartHint >= 0
    const replaceStart = hasReplacementHint
      ? Math.max(0, Math.min(replacementStartHint, baselineText.length))
      : -1
    const replaceEnd = hasReplacementHint
      ? Math.max(replaceStart,
        Math.min(Math.max(replacementEndHint, replaceStart), baselineText.length))
      : -1
    const rawRangeStart = range.start ?? -1
    const rawRangeEnd = range.end ?? -1
    let committedText: string | null = null
    let insertedText = ''
    let usedFullRead = false
    let fragmentSpanCount = 0
    let fragmentNonTextSpanCount = 0

    if (hasReplacementHint && rawRangeStart >= 0 && rawRangeEnd >= rawRangeStart) {
      const rangeText = this.extractRangeText({ start: rawRangeStart, end: rawRangeEnd })
      if (rangeText !== null && rangeText.nonTextSpanCount === 0 &&
          rangeText.text.length === rawRangeEnd - rawRangeStart) {
        insertedText = rangeText.text
        fragmentSpanCount = rangeText.spanCount
        fragmentNonTextSpanCount = rangeText.nonTextSpanCount
        if (rawRangeStart === replaceStart) {
          committedText = baselineText.substring(0, replaceStart) + insertedText + baselineText.substring(replaceEnd)
        } else {
          this.reportNativeFactError('ime-commit',
            `IME range start mismatch rangeStart=${rawRangeStart} replaceStart=${replaceStart}`)
        }
      } else {
        this.reportNativeFactError('ime-commit',
          `IME range fragment extraction failed start=${rawRangeStart} end=${rawRangeEnd}`)
      }
    }

    if (committedText === null) {
      // 片段事实无法重建（异常路径）：完整读取原生全文恢复。
      usedFullRead = true
      committedText = this.readNativeFullText()
      if (committedText === null) {
        this.reportNativeFactError('ime-commit', 'native full text unavailable after IME commit')
        this.idleError = 'IME commit failed: native full text unavailable'
        hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, this.idleError)
        this.rejectAllWaiters()
        return
      }
    }

    // 预编辑期间镜像应保持提交前基线；若为其它内容（非本次提交结果），记录事实错误。
    // 提交回调先于 onDidIMEInput 到达时镜像已等于本次提交结果，属于正常情况。
    let imeFactError = usedFullRead
    if (previousMirror !== baselineText && previousMirror !== committedText) {
      imeFactError = true
      this.reportNativeFactError('ime-commit',
        `mirror diverged during composition mirrorLen=${previousMirror.length}`
          + ` baselineLen=${baselineText.length} nativeLen=${committedText.length}`)
    }

    this.currentText = committedText
    let diff: TextDiff
    if (usedFullRead) {
      diff = this.computeDiff(baselineText, committedText)
      insertedText = diff.replacement
      this.nativeNewlineCount = countNewlines(committedText)
    } else {
      diff = { start: replaceStart, end: replaceEnd, replacement: insertedText }
      // 以预编辑基线换行数 + 本次替换片段增量重算，避免预编辑期中间态计数叠加。
      this.nativeNewlineCount = Math.max(0,
        this.imeComposingStartNewlineCount
          + countNewlines(insertedText)
          - countNewlines(baselineText.substring(replaceStart, replaceEnd)))
    }
    // 同一次 IME 提交随后重复触发的 onDidChange 用「提交范围 + 片段」签名去重。
    this.lastImeCommit = usedFullRead
      ? { start: diff.start, end: diff.start + diff.replacement.length, insertedText: diff.replacement }
      : { start: rawRangeStart, end: rawRangeEnd, insertedText: insertedText }

    this.recordNativeOperation({
      kind: 'ime-commit',
      beforeUtf16Length: baselineText.length,
      afterUtf16Length: this.currentText.length,
      rangeBeforeStartUtf16: diff.start,
      rangeBeforeEndUtf16: diff.end,
      rangeAfterStartUtf16: rawRangeStart >= 0 ? rawRangeStart : diff.start,
      rangeAfterEndUtf16: rawRangeStart >= 0 ? rawRangeEnd : diff.start + diff.replacement.length,
      startUtf16: diff.start,
      endUtf16: diff.end,
      replacedUtf16Length: diff.end - diff.start,
      insertedUtf16Length: diff.replacement.length,
      spanCountInRange: fragmentSpanCount,
      nonTextSpanCount: fragmentNonTextSpanCount,
      imeComposition: false,
      programmatic: this.programmaticUpdate,
      factError: imeFactError
    })

    const paragraphsChanged = diff.replacement.indexOf('\n') >= 0
      || baselineText.substring(diff.start, diff.end).indexOf('\n') >= 0

    if (diff.start >= diff.end && diff.replacement.length === 0) {
      this.captureCurrentNativeSelection()
      this.requestSelectionFlush()
      this.notifyStateListener('ime-commit', paragraphsChanged)
      this.checkIdle()
      return
    }

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

    this.captureCurrentNativeSelection()
    this.requestSelectionFlush()
    this.notifyStateListener('ime-commit', paragraphsChanged)
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

  // Issue #1034 评论 #19/#20/#21/#22/#23：选区单飞消费者协程。
  // 关键语义：pendingSelectionIntent 只有在 Core 确认成功后才清空；
  // IME 活跃或 Core 正文暂不一致时保留意图并退出，等待后续唤醒。
  // Issue #1034 评论 #22：清空 pendingSelectionIntent 前用对象身份比较。
  // Issue #1034 评论 #23：严格 UTF-16→UTF-8 转换；按 outcome 分开处理，
  // InvalidOffset/InvalidRange/StaleRevision 不无限重试相同错误坐标。
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

      // 本地正文已变：选区坐标过期，重新采集原生选区
      if (intent.textForConversion !== this.currentText) {
        if (this.pendingSelectionIntent === intent) {
          this.pendingSelectionIntent = null
          this.captureCurrentNativeSelection()
        }
        continue
      }

      // Core 正文暂不一致：保留选区意图，等 drainPendingEdits/syncFromCore 结算后重新唤醒
      const snap = this.coordinator.getSnapshot()
      if (snap === null) {
        return
      }
      if (snap.text !== intent.textForConversion) {
        return
      }

      // IME 预编辑期间：保留选区意图，等 onDidIMEInput 唤醒
      if (this.imeComposing) {
        return
      }

      // Issue #1034 评论 #23：严格 UTF-16→UTF-8 转换，非法 offset 直接失败
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
        // Issue #1034 评论 #32：旧意图的异步回包失败不能覆盖新意图
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

      // Issue #1034 评论 #23：按 outcome 分开处理，不无限重试相同错误
      if (outcome === EditorEditOutcome.StaleRevision) {
        // Issue #1034 评论 #28/#31：StaleRevision 只表示 Core 版本不一致，不比较文本。
        // 与 InvalidOffset/InvalidRange 区分处理：版本过期时按版本恢复，坐标非法时按坐标恢复。
        // 重试记录关联到本意图的 retriedAtRevision，不跨意图、跨会话泄漏。
        const newSnap = this.coordinator.getSnapshot()
        if (newSnap !== null && newSnap.text !== intent.textForConversion) {
          // Core 正文确实变化：保留 latest-wins 意图，等正文收敛后重新计算字节坐标
          return
        }
        // Core 正文相同但 revision 已前进：以新 revision 重新提交选区
        const currentRevision = newSnap !== null ? newSnap.revision : null
        if (this.pendingSelectionIntent === intent) {
          // 当前意图仍然是 pendingSelectionIntent：检查本意图是否已在同一 revision 上失败过
          if (currentRevision !== null && intent.retriedAtRevision === currentRevision) {
            this.idleError = 'StaleRevision: same revision retry failed, Core snapshot recovery may have failed'
            hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, this.idleError)
            this.rejectAllWaiters()
            return
          }
          this.pendingSelectionIntent = null
          this.captureCurrentNativeSelection(currentRevision)
        }
        // pendingSelectionIntent 已被新意图覆盖：新意图有自己的 retriedAtRevision，不受旧意图影响
        continue
      }

      if (outcome === EditorEditOutcome.InvalidOffset || outcome === EditorEditOutcome.InvalidRange) {
        // Issue #1034 评论 #32：旧意图的失败不能覆盖新意图
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

      // Issue #1034 评论 #32：旧意图的异步回包失败不能覆盖新意图
      if (this.pendingSelectionIntent !== intent) {
        continue
      }
      this.idleError = `Unknown selection outcome: ${outcome}`
      hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, this.idleError)
      this.rejectAllWaiters()
      return
    }
  }

  // Issue #1034 评论 #23：比较选区意图的位置和正文是否相同（值比较，非身份比较）
  private isSameSelectionPosition(a: SelectionIntent | null, b: SelectionIntent): boolean {
    if (a === null) {
      return false
    }
    return a.startUtf16 === b.startUtf16 &&
           a.endUtf16 === b.endUtf16 &&
           a.textForConversion === b.textForConversion
  }

  // Issue #1034 评论 #23：reject 所有 IdleWaiter 的辅助方法
  private rejectAllWaiters(): void {
    const error = new Error(this.idleError ?? 'Unknown error')
    const waiters = this.idleWaiters
    this.idleWaiters = []
    for (const w of waiters) {
      w.reject(error)
    }
  }

  // Issue #1034 评论 #21/#22：从 RichEditorController 重新捕获当前原生选区。
  // RichEditorSelection.selection 是长度为 2 的数组 [start, end]，
  // 折叠光标用 getCaretOffset() 获取更准确的位置。
  private captureCurrentNativeSelection(retriedAtRevision: number | null = null): void {
    if (this.richEditorController === null) {
      return
    }
    const expectedCoreSessionId = this.coordinator.getSessionId()
    if (expectedCoreSessionId === null) {
      return
    }

    const sel = this.richEditorController.getSelection()
    if (sel === undefined || sel === null) {
      return
    }

    let startUtf16: number
    let endUtf16: number
    const selectionStart = sel.selection[0]
    const selectionEnd = sel.selection[1]
    if (selectionStart === selectionEnd) {
      const caretOffset = this.richEditorController.getCaretOffset()
      startUtf16 = caretOffset
      endUtf16 = caretOffset
    } else {
      startUtf16 = selectionStart
      endUtf16 = selectionEnd
    }

    this.pendingSelectionIntent = {
      localSessionToken: this.sessionId,
      expectedCoreSessionId: expectedCoreSessionId,
      startUtf16: startUtf16,
      endUtf16: endUtf16,
      textForConversion: this.currentText,
      retriedAtRevision: retriedAtRevision
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

      // Issue #1034 评论 #24：严格 UTF-16→UTF-8 转换，非法 offset 不提交 Core
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
        // Issue #1053 评论 7099290808：按已应用的替换片段增量维护 Core 换行数；
        // 长度不符（Core 归一化了正文）时才完整重算。
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
        if (result.data.outcome === EditorEditOutcome.AppliedWithAdjustedSelection &&
            this.richEditorController !== null) {
          const utf16Cursor = TextOffsetMapper.utf8ToUtf16(newSnap.text, newSnap.cursor)
          const utf16Anchor = TextOffsetMapper.utf8ToUtf16(newSnap.text, newSnap.selectionAnchor)
          this.richEditorController.setSelection(utf16Anchor, utf16Cursor)
        }
      }

      // Issue #1053：Core 确认后通知组件刷新诊断快照；段落结构变化时重新应用原生段落样式。
      if (edit.sessionId === this.sessionId) {
        const paragraphsChanged = edit.insertedText.indexOf('\n') >= 0 || edit.oldText.indexOf('\n') >= 0
        this.notifyStateListener('edit-settled', paragraphsChanged)
      }
    }
  }

  // 计算最小文本差量（按完整 Unicode code point 求前后缀）。
  // 纯逻辑在 editor_patch_logic.ts，收敛事务与平台差量共用同一实现。
  private computeDiff(oldText: string, newText: string): TextDiff {
    return computeMinimalTextDiff(oldText, newText)
  }

  // 将 edit 的偏移从 edit.beforeText 映射到 baselineText。
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

  // ── Issue #1053：原生编辑事实与运行时诊断（不含正文原文） ──────────

  // 把 TextRange 解析成镜像坐标；缺省 end 按 TextRange 语义视为文本末尾。
  private resolveTextRange(range: TextRange, textLength: number): EditRangeMap | null {
    const rawStart = range.start ?? 0
    const rawEnd = range.end ?? textLength
    if (rawStart < 0 || rawEnd < rawStart || rawEnd > textLength) {
      return null
    }
    return { startUtf16: rawStart, endUtf16: rawEnd }
  }

  // 按原生 Span 的实际 offset 提取 range 内文本；end 缺省表示到文档末尾。
  // offsetInSpan 是 Span 与请求范围交集的合法内容，跨越多个 Span/换行时按 spanRange 排序拼接。
  private extractRangeText(range: TextRange): NativeRangeText | null {
    if (this.richEditorController === null) {
      return null
    }
    const start = range.start ?? 0
    if (start < 0) {
      return null
    }
    const end = range.end
    try {
      const spans = end === undefined
        ? this.richEditorController.getSpans({ start: start })
        : this.richEditorController.getSpans({ start: start, end: end })
      // getSpans 在 controller 未绑定组件时返回 undefined。
      if (spans === undefined || spans === null) {
        return null
      }
      const textSpans: RichEditorTextSpanResult[] = []
      let nonTextSpanCount = 0
      for (const span of spans) {
        const textSpan = span as RichEditorTextSpanResult
        // SymbolSpan 的 value 是 symbol id 而不是正文，按非文本 Span 处理。
        if (textSpan.value !== undefined && textSpan.symbolSpanStyle === undefined) {
          textSpans.push(textSpan)
        } else {
          nonTextSpanCount++
        }
      }
      textSpans.sort((a: RichEditorTextSpanResult, b: RichEditorTextSpanResult): number => {
        return a.spanPosition.spanRange[0] - b.spanPosition.spanRange[0]
      })
      let text = ''
      for (const span of textSpans) {
        const valueLength = span.value.length
        const offsetStart = Math.max(0, Math.min(span.offsetInSpan[0], valueLength))
        const offsetEnd = Math.max(offsetStart, Math.min(span.offsetInSpan[1], valueLength))
        text += span.value.substring(offsetStart, offsetEnd)
      }
      return {
        text: text,
        spanCount: textSpans.length + nonTextSpanCount,
        nonTextSpanCount: nonTextSpanCount,
        startUtf16: start,
        endUtf16: end === undefined ? start + text.length : end
      }
    } catch (e) {
      hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, `extractRangeText error: ${JSON.stringify(e)}`)
      return null
    }
  }

  // 从原生 Span 重建全文镜像；失败返回 null。仅用于事实校验与非热路径恢复。
  private readNativeFullText(): string | null {
    if (this.richEditorController === null) {
      return null
    }
    try {
      const spans = this.richEditorController.getSpans()
      // getSpans 在 controller 未绑定组件时返回 undefined。
      if (spans === undefined || spans === null) {
        return null
      }
      const textSpans: RichEditorTextSpanResult[] = []
      let nonTextSpanCount = 0
      for (const span of spans) {
        const textSpan = span as RichEditorTextSpanResult
        // SymbolSpan 的 value 是 symbol id 而不是正文，按非文本 Span 处理。
        if (textSpan.value !== undefined && textSpan.symbolSpanStyle === undefined) {
          textSpans.push(textSpan)
        } else {
          nonTextSpanCount++
        }
      }
      textSpans.sort((a: RichEditorTextSpanResult, b: RichEditorTextSpanResult): number => {
        return a.spanPosition.spanRange[0] - b.spanPosition.spanRange[0]
      })
      let text = ''
      for (const span of textSpans) {
        text += span.value
      }
      this.lastReadNativeSpanCount = textSpans.length + nonTextSpanCount
      if (nonTextSpanCount > 0) {
        this.reportNativeFactError('readNativeFullText', `document contains ${nonTextSpanCount} non-text span(s)`)
      }
      return text
    } catch (e) {
      hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, `readNativeFullText error: ${JSON.stringify(e)}`)
      return null
    }
  }

  private recordNativeOperation(facts: NativeOperationFacts): void {
    this.lastNativeOperation = {
      kind: facts.kind,
      beforeUtf16Length: facts.beforeUtf16Length,
      afterUtf16Length: facts.afterUtf16Length,
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
      programmatic: facts.programmatic,
      coreRevision: this.lastAppliedRevision,
      factError: facts.factError,
      atMs: Date.now()
    }
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

  // Issue #1053 评论 7098652618：保存/退出屏障的三方校验。
  // 放行条件是「原生全文 readNativeFullText() === 本地镜像 currentText === Core snapshot.text」；
  // 同时记录长度、换行数、hash 与首个差异 offset（不含正文），供诊断与失败报告使用。
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
    // Core revision 超出原生已应用 revision：正文由其它编辑路径推进（如纠错采纳），
    // 原生侧没有未结算编辑，此时以 Core 为准刷新原生，不能反向收敛覆盖 Core。
    const coreAdvanced = coreAvailable && appliedRevision !== coreRevision
    const nativeNewlines = countNewlines(nativeText)
    const coreNewlines = countNewlines(coreText)
    const nativeHash = hashTextUtf16(nativeText)
    const coreHash = hashTextUtf16(coreText)

    if (coreAvailable && mirrorMatchesEditor && !matched && !coreAdvanced) {
      // 原生正文是用户可见真相而 Core 落后：保留原生内容，拒绝后续程序性回写覆盖。
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

  // 收敛成功后按原生当前选区重新上报：正文坐标已变化，旧选区意图已在收敛前丢弃。
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

    if (this.nativeCoreDiverged &&
        this.coordinator.getSessionId() !== this.nativeCoreDivergedSessionId) {
      // Core 已切换到新会话：上一章的分歧不再适用。
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
      // Issue #1053 评论 7098652618：原生可见正文还没有全部进入 Core（例如 IME 提交差量失败），
      // 此时用 Core 旧正文覆盖原生会把用户文字静默吞掉；保留原生正文，交给保存/退出屏障收敛。
      // 这是待收敛的中间态（例如收敛事务的 notifyListener 先于镜像确认），不记持久事实错误。
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
        beforeUtf16Length: this.currentText.length,
        afterUtf16Length: snap.text.length,
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
    let syncStart = 0
    let syncEnd = 0
    let syncInsertedLength = 0
    let syncSpanCount = 0
    let syncNonTextSpanCount = 0

    this.programmaticUpdate = true
    try {
      const diff = this.computeDiff(previousText, snap.text)
      syncStart = diff.start
      syncEnd = diff.end
      syncInsertedLength = diff.replacement.length
      if (diff.start < diff.end || diff.replacement.length > 0) {
        this.richEditorController.deleteSpans({ start: diff.start, end: diff.end })
        if (diff.replacement.length > 0) {
          this.richEditorController.addTextSpan(diff.replacement, { offset: diff.start })
        }
      }
      this.currentText = snap.text
      this.nativeNewlineCount = countNewlines(snap.text)
      this.coreNewlineCount = this.nativeNewlineCount

      const writtenRange = this.extractRangeText({
        start: diff.start,
        end: diff.start + diff.replacement.length
      })
      if (writtenRange !== null) {
        syncSpanCount = writtenRange.spanCount
        syncNonTextSpanCount = writtenRange.nonTextSpanCount
      }

      this.lastAppliedRevision = snap.revision
      this.confirmedText = snap.text
      this.confirmedRevision = snap.revision
      this.nativeCoreDiverged = false
      this.nativeCoreDivergedSessionId = null
    } catch (e) {
      hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, `syncFromCore error: ${JSON.stringify(e)}`)
    } finally {
      this.programmaticUpdate = false
    }

    this.recordNativeOperation({
      kind: 'programmatic-sync',
      beforeUtf16Length: previousText.length,
      afterUtf16Length: this.currentText.length,
      rangeBeforeStartUtf16: syncStart,
      rangeBeforeEndUtf16: syncEnd,
      rangeAfterStartUtf16: syncStart,
      rangeAfterEndUtf16: syncStart + syncInsertedLength,
      startUtf16: syncStart,
      endUtf16: syncEnd,
      replacedUtf16Length: syncEnd - syncStart,
      insertedUtf16Length: syncInsertedLength,
      spanCountInRange: syncSpanCount,
      nonTextSpanCount: syncNonTextSpanCount,
      imeComposition: false,
      programmatic: true,
      factError: false
    })
    // Issue #1053：段落在程序性替换中新增/删除后，通知组件重新应用原生段落样式。
    // 只按段落结构变化通知，不在每次选区变化时全文重新排版。
    this.nativeTextReady = true
    this.notifyStateListener('sync-from-core', paragraphsChanged)
    this.validateNativeCoreAlignment('sync-from-core')
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
  // Issue #1053：记录一次程序性写入事实，并通知组件在原生正文写完后应用段落样式。
  async loadInitialText(text: string, cursorByteOffset: number): Promise<void> {
    if (this.richEditorController === null) {
      return
    }

    const previousText = this.currentText
    let spanCount = 0
    let nonTextSpanCount = 0

    this.programmaticUpdate = true
    try {
      this.richEditorController.deleteSpans()
      this.richEditorController.addTextSpan(text, { offset: 0 })
      this.currentText = text
      this.nativeNewlineCount = countNewlines(text)
      this.coreNewlineCount = this.nativeNewlineCount

      const writtenRange = this.extractRangeText({ start: 0, end: text.length })
      if (writtenRange !== null) {
        spanCount = writtenRange.spanCount
        nonTextSpanCount = writtenRange.nonTextSpanCount
      }

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
      beforeUtf16Length: previousText.length,
      afterUtf16Length: text.length,
      rangeBeforeStartUtf16: 0,
      rangeBeforeEndUtf16: previousText.length,
      rangeAfterStartUtf16: 0,
      rangeAfterEndUtf16: text.length,
      startUtf16: 0,
      endUtf16: previousText.length,
      replacedUtf16Length: previousText.length,
      insertedUtf16Length: text.length,
      spanCountInRange: spanCount,
      nonTextSpanCount: nonTextSpanCount,
      imeComposition: false,
      programmatic: true,
      factError: false
    })
    this.nativeTextReady = true
    this.notifyStateListener('initial-load', true)
    this.validateNativeCoreAlignment('initial-load')
  }
}
