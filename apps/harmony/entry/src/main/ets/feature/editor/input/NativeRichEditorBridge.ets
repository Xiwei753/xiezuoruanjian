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
//
// Issue #1063 评论第 4 条：会话健康状态机（替代只由 beginSession 重置的 idleError）
//   healthy ──映射/事务失败──▶ reconciling ──可验证 rebase 成功──▶ healthy
//                                   └──映射失败/重叠/文本不一致──▶ conflict
//   - reconciling：冻结 pendingEdits（含每条 edit 的 beforeText 基线），从 Core 当前
//     snapshot 与原生实际全文计算可验证 rebase，由原串行 editQueue 提交；期间 idle waiters
//     保持挂起，syncFromCore 不回写覆盖原生正文。
//   - conflict：保留原生正文和全部未确认事件，不调用 syncFromCore 覆盖；waiters 立即以
//     明确原因 reject，诊断字段 nativeCoreDiverged 同步置 true；只有 beginSession 复位。
//   - 失败不再用单一 idleError 锁死编辑：错误详情写在 conflictDetail（只读 getter 供诊断）。

import { EditorSessionCoordinator } from '../session/EditorSessionCoordinator'
import { computeMinimalTextDiff } from '../session/editor_patch_logic'
import { TextOffsetMapper } from './TextOffsetMapper'
import { insertedParagraphStarts as computeInsertedParagraphStarts } from './text_document_rules'
import { EditorViewportState } from '../render/EditorViewportState'
import { PlatformApiResolver } from '../../../platform/version/PlatformApiResolver'
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
// 可验证 rebase 的最大尝试次数（Core 基线在映射期间被外部命令推进时重读再映射）。
const BRIDGE_RECONCILE_MAX_ATTEMPTS = 3
// 退出屏障等待 IME 结算回调的上限：超时后按“已提交 Core revision”结算（预编辑仍留在
// 原生未提交层，不写 Core），保证每个 idle waiter 都有确定结果，不会永久悬挂。
const BRIDGE_IME_SETTLEMENT_TIMEOUT_MS = 1200
// 接续恢复滚动：scrollToVisible 自 API 26 起可用；低版本按注释降级，不做假回写。
const BRIDGE_VIEWPORT_SCROLL_TO_VISIBLE_MIN_API = 26
// 章节布局完成（lineCount>0）前恢复视口会滚错位置，按帧重试到上限即放弃（约 2s）。
const BRIDGE_VIEWPORT_RESTORE_MAX_ATTEMPTS = 40
const BRIDGE_VIEWPORT_RESTORE_RETRY_MS = 50

// Issue #1054 复审：原生编辑器状态事件。
// paragraphsChanged 只在段落结构真正变化时置 true；newParagraphStarts 给出本次
// 插入文本中新段落的 UTF-16 起点，组件用它核验新段落是否继承了全文段落样式。
// 纯选区/光标变化不会触发全文重新排版。
export interface NativeEditorStateEvent {
  kind: string
  paragraphsChanged: boolean
  newParagraphStarts: number[]
  becameNonEmpty: boolean
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

// Issue #1063：冻结 pendingEdits 在 Core 当前正文上的模拟应用结果。
// reason 非空表示无法安全 rebase（调用方直接进入 conflict，不覆盖原生正文）。
interface FrozenEditSimulation {
  text: string
  reason: string
}

/**
 * Issue #1063 评论第 4 条：会话健康状态。
 * - healthy：正常消费 pendingEdits，三方（原生全文/镜像/Core）可校验。
 * - reconciling：发生映射或事务失败，消费游标冻结，正在从 Core 与原生全文计算可验证 rebase；
 *   idle waiters 保持挂起（不 reject）。
 * - conflict：无法安全 rebase。保留原生正文与全部未确认事件，不覆盖原生文本；
 *   idle waiters 以冲突原因 reject，诊断 nativeCoreDiverged 置 true。
 */
export type EditorSessionHealth = 'healthy' | 'reconciling' | 'conflict'

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
  // 创建时冻结的屏障 revision：只有 lastAppliedRevision >= barrierRevision 且三方一致才放行。
  barrierRevision: number
  // true：退出/同步屏障，必须等 IME 预编辑真正结算；false：普通保存屏障，
  // IME 预编辑保留在独立未提交层，可先保存已提交 Core revision。
  waitForImeSettlement: boolean
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
  // Issue #1063：显式会话状态机（替代只由 beginSession 重置的 idleError）。
  private sessionHealth: EditorSessionHealth = 'healthy'
  private conflictDetail: string | null = null
  // reconciling 期间的原因（诊断用；进入 conflict 时写进 conflictDetail）。
  private reconcileReason: string = ''
  private reconcileScheduled: boolean = false
  // 退出屏障等待 IME 结算回调（原生真正结束/取消预编辑时 resolve）。
  private imeSettlementWaiters: (() => void)[] = []
  // 退出屏障的 IME 结算回调是否超时：超时后按已提交 Core revision 放行。
  private imeSettlementTimedOut: boolean = false
  // 接续恢复滚动的重试句柄（beginSession/切章时取消）。
  private viewportRestoreTimerId: number = -1
  private viewportRestoreAttempts: number = 0
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

  // Issue #1063：会话健康状态（诊断只读）。
  getSessionHealth(): EditorSessionHealth {
    return this.sessionHealth
  }

  // Issue #1063：conflict 的明确原因（healthy/reconciling 时为 null）。
  getConflictDetail(): string | null {
    return this.conflictDetail
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
  // 保存协调器在入队时会把该判定冻结进不可变 SaveRequest；后续非空编辑可以清掉这里的
  // 当前意图，但不会撤销已经入队的合法空快照授权。
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

  // 普通保存屏障（自动保存）：只等已提交 Core revision，不调用 stopEditing()。
  // Issue #1063：IME 预编辑保留在独立未提交层，waitForImeSettlement=false，
  // 因此预编辑不会让普通保存长期悬挂。
  whenIdle(): Promise<void> {
    this.barrierStage = 'save-when-idle'
    this.imeSettlementTimedOut = false
    // 组件已无预览文本时先结算；仍有预编辑则保留在原生未提交层，不写 Core。
    this.settleImeComposition(false)
    this.requestSelectionFlush()
    return this.pushIdleWaiter(false)
  }

  // 退出章节/同步屏障：先 stopEditing() 结束预编辑，再等队列稳定。
  // Issue #1063：结算与 checkIdle 绑定为队列尾部的一步，不依赖下一次任意 onDidChange；
  // 若此时仍有预编辑，等这次真正的结算回调（有上限），不会永久挂起。
  finishEditingAndWhenIdle(): Promise<void> {
    this.barrierStage = 'exit-finish-editing'
    this.imeSettlementTimedOut = false
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
    const waiterPromise = this.pushIdleWaiter(true)
    const sessionToken = this.sessionId
    this.editQueue = this.editQueue.then(async (): Promise<void> => {
      if (sessionToken !== this.sessionId) {
        return
      }
      const settled = await this.awaitImeSettlement()
      if (sessionToken !== this.sessionId) {
        return
      }
      if (settled) {
        this.settleImeComposition(true)
      } else {
        // 结算回调没按时到达：不把未提交的预编辑文本写进 Core，
        // 只按已提交 Core revision 放行屏障（见 resolveCommittedWaiters）。
        this.imeSettlementTimedOut = true
        hilog.warn(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG,
          `IME settlement did not arrive within ${BRIDGE_IME_SETTLEMENT_TIMEOUT_MS}ms;`
            + ' resolving idle barrier with committed core revision')
        this.settleImeComposition(false)
      }
      this.checkIdle()
    }).catch((e: Object): void => {
      hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, `ime settlement step error: ${JSON.stringify(e)}`)
      this.checkIdle()
    })
    this.checkIdle()
    return waiterPromise
  }

  // Issue #1063：退出/切章时给所有 idle waiter 一个确定结果（不伪造空闲）。
  cancelIdleWaiters(reason: string): void {
    if (this.idleWaiters.length === 0) {
      return
    }
    hilog.warn(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, `cancel idle waiters: ${reason}`)
    const waiters = this.idleWaiters
    this.idleWaiters = []
    const error = new Error(reason)
    for (const w of waiters) {
      w.reject(error)
    }
  }

  // ── Issue #1063：会话健康状态机 ────────────────────────────────

  /**
   * 进入 reconciling：本次会话的消费游标冻结（drain 不再取新 edit），保留全部 pendingEdits
   * 与失败发生前的 base snapshot（每条 edit 自带 beforeText）；由原串行 editQueue 提交
   * 可验证 rebase。reconciling 期间 idle waiters 保持挂起，不 reject。
   */
  private enterReconciling(reason: string): void {
    if (this.sessionHealth === 'conflict') {
      // conflict 是终态：不降级，保留原冲突原因（只有 beginSession 重新开始）。
      return
    }
    this.sessionHealth = 'reconciling'
    this.reconcileReason = reason
    this.conflictDetail = null
    hilog.warn(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG,
      `session entered reconciling: ${reason} pendingEdits=${this.pendingEdits.length}`)
    this.scheduleReconcile()
  }

  /**
   * 进入 conflict：保留原生正文与全部未确认事件，不调用 syncFromCore 回写覆盖原生文本；
   * 所有 idle waiters 以明确原因 reject。诊断字段 nativeCoreDiverged 与状态机保持一致，
   * 阻止 refreshNativeMirrorFromCore/syncFromCore 覆盖仍在原生编辑器里的正文。
   */
  private enterConflict(reason: string): void {
    this.sessionHealth = 'conflict'
    this.conflictDetail = reason
    this.reconcileReason = ''
    this.nativeCoreDiverged = true
    this.nativeCoreDivergedSessionId = this.coordinator.getSessionId()
    hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG,
      `session conflict: ${reason} pendingEdits=${this.pendingEdits.length}`
        + ` nativeLen=${this.currentText.length}`)
    this.rejectWaitersForConflict()
  }

  private rejectWaitersForConflict(): void {
    if (this.idleWaiters.length === 0) {
      return
    }
    const message = `Native/core session conflict: ${this.conflictDetail !== null ? this.conflictDetail : 'unknown'}`
    const waiters = this.idleWaiters
    this.idleWaiters = []
    const error = new Error(message)
    for (const w of waiters) {
      w.reject(error)
    }
  }

  // 把 reconcile 排到原串行 editQueue 尾部，保证不与 drain/选区事务并发。
  private scheduleReconcile(): void {
    if (this.sessionHealth !== 'reconciling' || this.reconcileScheduled) {
      return
    }
    this.reconcileScheduled = true
    const sessionToken = this.sessionId
    this.editQueue = this.editQueue.then(async (): Promise<void> => {
      this.reconcileScheduled = false
      if (sessionToken !== this.sessionId) {
        return
      }
      if (this.sessionHealth !== 'reconciling') {
        this.checkIdle()
        return
      }
      let deferred: boolean = false
      try {
        deferred = await this.reconcileSession()
      } catch (e) {
        this.enterConflict(`reconcile error: ${JSON.stringify(e)}`)
      }
      if (deferred) {
        // 等原生变更/预编辑的结算回调再次驱动 reconcile，不在微任务链上自旋重排。
        return
      }
      this.checkIdle()
    }).catch((e: Object): void => {
      this.reconcileScheduled = false
      hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, `reconcile step error: ${JSON.stringify(e)}`)
      if (sessionToken === this.sessionId) {
        this.enterConflict(`reconcile step error: ${JSON.stringify(e)}`)
      }
      this.checkIdle()
    })
  }

  /**
   * 从 Core 当前 snapshot 与原生实际全文计算可验证 rebase。
   *
   * 判定顺序：
   *   1. 原生全文 === Core 快照 → 直接收敛 healthy（丢弃已确认的冻结 edit，恢复消费）；
   *   2. 否则按 edit 原始顺序把冻结 pendingEdits 映射到 Core 当前正文，逐笔校验 oldText，
   *      模拟结果必须与原生全文完全一致；全部通过才提交一笔 Programmatic replaceSlice；
   *   3. 提交后再次校验 Core 快照文本 === 原生全文、revision === lastAppliedRevision，
   *      才恢复 healthy 并清空已确认的 pendingEdits；
   *   4. 任一映射失败/重叠/文本不一致/事务失败 → conflict（保留原生正文与未确认事件）。
   *
   * @returns true 表示本轮因预编辑/原生变更未结算而推迟（调用方不要在同一条微任务链上
   *          立即重排，等结算回调再驱动），false 表示已给出确定结论（healthy 或 conflict）。
   */
  private async reconcileSession(): Promise<boolean> {
    if (this.sessionHealth !== 'reconciling') {
      return false
    }
    if (this.pendingNativeChange !== null || this.imeComposing) {
      // 预编辑/原生变更未结算：镜像与 Core 都还不是稳定基线，等结算回调后再 rebase
      // （commitImeCompositionFromBaseline / onContentDidChange 会再次触发）。
      // 返回 true ≠ 已收敛：调用方不能在同一条微任务链上立即重排，否则会形成忙等循环。
      hilog.info(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG,
        'reconcile deferred: native change or IME composition still in flight')
      return true
    }
    const sessionAtEntry = this.sessionId
    // 冻结这一批 edit：drain 在 reconciling 期间不消费，新 edit 排在它们之后。
    const frozen: PendingEdit[] = this.pendingEdits.slice()
    const frozenCount: number = frozen.length
    let attempts: number = 0
    while (attempts < BRIDGE_RECONCILE_MAX_ATTEMPTS) {
      attempts++
      // 每轮重读 Core 会话/快照：会话在 reconcile 期间可能被替换（重开章节），
      // 不能沿用上一轮的 sessionId，否则会一直 SESSION_CHANGED。
      const coreSessionId = this.coordinator.getSessionId()
      if (coreSessionId === null) {
        this.enterConflict('no active core session during reconcile')
        return false
      }
      const snap = this.coordinator.getSnapshot()
      if (snap === null) {
        this.enterConflict('core snapshot unavailable during reconcile')
        return false
      }
      const nativeText = this.readNativeFullText()
      if (nativeText === null) {
        this.enterConflict('native full text unavailable during reconcile')
        return false
      }
      if (nativeText !== this.currentText) {
        // 镜像落后：以原生实际全文为真相，只更新本地镜像，不改 Core。
        this.reportNativeFactError('reconcile',
          `mirror behind native text firstMismatch=${firstMismatchUtf16(this.currentText, nativeText)}`)
        this.currentText = nativeText
        this.nativeNewlineCount = countNewlines(nativeText)
      }
      if (nativeText === snap.text) {
        // 原生全文与 Core 快照逐字相等：直接收敛，不提交任何 rebase 事务。
        this.confirmRebaseConverged(snap.text, snap.revision)
        this.pendingEdits = this.pendingEdits.slice(frozenCount)
        hilog.info(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG,
          `reconcile converged without rebase: coreRevision=${snap.revision} droppedEdits=${frozenCount}`)
        this.refreshSelectionAfterConvergence()
        this.wakeDrain()
        return false
      }
      const simulated: FrozenEditSimulation = this.simulateFrozenEdits(frozen, snap.text)
      if (simulated.reason !== '') {
        this.enterConflict(simulated.reason)
        return false
      }
      if (simulated.text !== nativeText) {
        this.enterConflict(
          `frozen edits do not reproduce native text`
            + ` firstMismatch=${firstMismatchUtf16(simulated.text, nativeText)}`
            + ` simulatedLen=${simulated.text.length} nativeLen=${nativeText.length}`)
        return false
      }
      // diff 只用于诊断：真正提交的是「Core 当前正文 → 原生全文」的最小差量，
      // replaceSliceIfUnchanged 会在 Core 侧再算一次并核对 expectedText。
      const diff = this.computeDiff(snap.text, nativeText)
      const result = await this.coordinator.replaceSliceIfUnchanged(
        coreSessionId, snap.revision, snap.text, nativeText, EditorTransactionCause.Programmatic)
      if (sessionAtEntry !== this.sessionId) {
        return false
      }
      if (result.success && result.data !== undefined) {
        const after = this.coordinator.getSnapshot()
        if (after !== null && after.text === nativeText && after.revision === result.data.newRevision) {
          this.confirmRebaseConverged(after.text, after.revision)
          this.pendingEdits = this.pendingEdits.slice(frozenCount)
          hilog.info(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG,
            `reconcile committed rebase revision=${snap.revision}->${after.revision}`
              + ` diff=[${diff.start}, ${diff.end}) insertedUtf16=${diff.replacement.length}`
              + ` confirmedEdits=${frozenCount}`)
          this.refreshSelectionAfterConvergence()
          this.wakeDrain()
          return false
        }
        // Core 回流与原生全文不一致：重读基线再试。
        continue
      }
      if (result.errorCode === 'SESSION_CHANGED') {
        // Core 会话在 reconcile 期间被替换：重读会话/基线再判定（次数有上限）。
        continue
      }
      if (result.errorCode === 'STALE_BASELINE' && attempts < BRIDGE_RECONCILE_MAX_ATTEMPTS) {
        // Core 正文在映射期间被推进：重读基线重算映射。
        continue
      }
      this.enterConflict(`reconcile transaction failed: ${result.errorCode !== undefined ? result.errorCode : 'UNKNOWN'}`)
      return false
    }
    this.enterConflict(`reconcile did not converge after ${attempts} attempt(s)`)
    return false
  }

  // rebase 收敛：三方状态、revision 与 confirmed 基线一起对齐后恢复 healthy。
  private confirmRebaseConverged(text: string, revision: number): void {
    this.lastAppliedRevision = revision
    this.confirmedText = text
    this.confirmedRevision = revision
    this.coreNewlineCount = countNewlines(text)
    this.nativeCoreDiverged = false
    this.nativeCoreDivergedSessionId = null
    this.sessionHealth = 'healthy'
    this.conflictDetail = null
    this.reconcileReason = ''
  }

  // 按 edit 原始顺序把冻结 edit 映射到 Core 当前正文并模拟应用；失败返回非空 reason。
  private simulateFrozenEdits(frozen: PendingEdit[], coreText: string): FrozenEditSimulation {
    let simulated: string = coreText
    for (let i = 0; i < frozen.length; i++) {
      const edit: PendingEdit = frozen[i]
      const mapped: EditRangeMap | null = this.mapEditToBaseline(edit, simulated)
      if (mapped === null) {
        return {
          text: simulated,
          reason: `cannot map frozen edit #${edit.localSequence} at offset ${edit.startUtf16}: overlapping diff`
        }
      }
      const slice: string = simulated.substring(mapped.startUtf16, mapped.endUtf16)
      if (slice !== edit.oldText) {
        return {
          text: simulated,
          reason: `cannot map frozen edit #${edit.localSequence}: oldText mismatch`
            + ` expectedLen=${edit.oldText.length} actualLen=${slice.length}`
        }
      }
      simulated = simulated.substring(0, mapped.startUtf16) + edit.insertedText
        + simulated.substring(mapped.endUtf16)
    }
    return { text: simulated, reason: '' }
  }

  // 普通保存/超时屏障：冻结并保存已提交 Core revision。
  // committedStable 要求 Core 快照与确认基线一致；预编辑文本留在原生未提交层。
  private resolveCommittedWaiters(includeImeWaiters: boolean): void {
    const snap = this.coordinator.getSnapshot()
    const committedStable: boolean = snap !== null &&
      snap.revision === this.lastAppliedRevision && snap.text === this.confirmedText
    if (!committedStable) {
      return
    }
    const remaining: IdleWaiter[] = []
    const resolved: IdleWaiter[] = []
    for (const w of this.idleWaiters) {
      const eligible: boolean = w.sessionId === this.sessionId &&
        this.lastAppliedRevision >= w.barrierRevision &&
        (!w.waitForImeSettlement || includeImeWaiters)
      if (eligible) {
        resolved.push(w)
      } else {
        remaining.push(w)
      }
    }
    if (resolved.length === 0) {
      return
    }
    this.idleWaiters = remaining
    for (const w of resolved) {
      w.resolve()
    }
  }

  private pushIdleWaiter(waitForImeSettlement: boolean): Promise<void> {
    return new Promise<void>((resolve, reject) => {
      this.idleWaiters.push({
        sessionId: this.sessionId,
        barrierRevision: this.lastAppliedRevision,
        waitForImeSettlement: waitForImeSettlement,
        resolve: resolve,
        reject: reject
      })
      this.checkIdle()
    })
  }

  // 等真正的 IME 结算回调（原生结束/取消预编辑）；返回 false 表示超时未结算。
  private awaitImeSettlement(): Promise<boolean> {
    if (!this.imeComposing) {
      return Promise.resolve(true)
    }
    return new Promise<boolean>((resolve) => {
      let finished: boolean = false
      const finish = (settled: boolean): void => {
        if (finished) {
          return
        }
        finished = true
        resolve(settled)
      }
      this.imeSettlementWaiters.push((): void => {
        finish(true)
      })
      setTimeout((): void => {
        finish(!this.imeComposing)
      }, BRIDGE_IME_SETTLEMENT_TIMEOUT_MS)
    })
  }

  private notifyImeSettled(): void {
    const waiters = this.imeSettlementWaiters
    this.imeSettlementWaiters = []
    for (const w of waiters) {
      w()
    }
  }

  // 事件驱动检查：先看会话状态机，再等 editQueue tail，条件不满足时等待状态变化通知。
  private checkIdle(): void {
    if (this.sessionHealth === 'reconciling') {
      // reconciling：waiters 保持挂起（不 reject）；即使暂无 waiter 也要推进 reconcile，
      // 否则冻结的 pendingEdits 与消费游标会永远停住。
      this.scheduleReconcile()
      if (this.imeComposing) {
        // 预编辑使 reconcile 暂时无法收敛：普通保存屏障按已提交 Core revision 放行，
        // 不把自动保存挂在用户拼音输入时长上；退出屏障仍等结算回调/超时。
        this.resolveCommittedWaiters(false)
      }
      return
    }
    if (this.idleWaiters.length === 0) {
      return
    }

    if (this.sessionHealth === 'conflict') {
      // conflict 是确定结果：waiters 立刻拿到明确原因，不伪装空闲。
      this.rejectWaitersForConflict()
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

      if (this.sessionHealth === 'conflict') {
        this.rejectWaitersForConflict()
        return
      }
      if (this.sessionHealth === 'reconciling') {
        this.scheduleReconcile()
        if (this.imeComposing) {
          this.resolveCommittedWaiters(false)
        }
        return
      }

      if (this.pendingEdits.length > 0 || this.drainRunning) {
        return
      }

      if (this.imeComposing) {
        // 普通保存屏障可先保存已提交 Core revision；退出屏障等预编辑真正结算，
        // 结算回调超时后同样按已提交 revision 放行（预编辑文本不写 Core）。
        this.resolveCommittedWaiters(this.imeSettlementTimedOut)
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
        // 屏障 revision 已推进到当前 confirmed revision 的 waiter 才放行。
        const stillPending: IdleWaiter[] = []
        const resolved: IdleWaiter[] = []
        for (const w of currentWaiters) {
          if (this.lastAppliedRevision >= w.barrierRevision) {
            resolved.push(w)
          } else {
            stillPending.push(w)
          }
        }
        this.idleWaiters = remaining.concat(stillPending)
        for (const w of resolved) {
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
  // Issue #1063：只有 healthy 才允许 Core → 原生回写；reconciling/conflict 时绝不能覆盖原生正文。
  private async refreshNativeMirrorFromCore(): Promise<void> {
    if (this.sessionHealth !== 'healthy') {
      hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG,
        `refreshNativeMirrorFromCore refused: session is ${this.sessionHealth}`)
      return
    }
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

  // 无法把原生全文与 Core 收敛：进入 conflict（保留原生正文与未确认事件），
  // 并按会话 reject 所有 idle waiters；错误信息含明确原因。
  private failNativeCoreAlignment(detail: string): void {
    const message = `Native/core text alignment failed stage=${this.barrierStage}: ${detail}`
    this.enterConflict(message)
  }

  setPendingPaste(): void {
    this.pendingPaste = true
  }

  // 切章时调用：增加 sessionId 隔离旧回调，reject 旧 session 等待者，清空队列并复位状态机。
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
    // Issue #1063：会话终止复位状态机（conflict 只在这里解除）。
    this.sessionHealth = 'healthy'
    this.conflictDetail = null
    this.reconcileReason = ''
    this.reconcileScheduled = false
    this.imeSettlementWaiters = []
    this.imeSettlementTimedOut = false
    this.cancelViewportRestore()
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
    let insertedText: string = ''
    if (value.replacementString !== undefined && value.replacementString !== null) {
      try {
        insertedText = value.replacementString.getString()
      } catch (e) {
        insertedText = ''
        this.reportNativeFactError('onContentWillChange', `replacementString unavailable: ${JSON.stringify(e)}`)
      }
    }
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
        // 原生事实不可用：冻结消费并交由 reconcile 给出确定结果（可 rebase → healthy，否则 conflict）。
        this.enterReconciling('native text fact mismatch and native full text unavailable')
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
      this.notifyStateListener('native-change', paragraphsChanged,
        this.insertedParagraphStarts(insertedText, startUtf16), becameNonEmpty)
      // 预编辑态也可能有待收敛的状态机：给 checkIdle 一次事件机会（无 waiter 时立即返回）。
      this.checkIdle()
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
      this.notifyStateListener('native-change', paragraphsChanged,
        this.insertedParagraphStarts(insertedText, startUtf16), becameNonEmpty)
      this.checkIdle()
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
    this.notifyStateListener('native-change', paragraphsChanged,
      this.insertedParagraphStarts(insertedText, startUtf16), becameNonEmpty)
  }

  // 预编辑结算：从冻结基线到当前镜像只提交一笔差量。
  // 组件不支持 onDidIMEInput，因此保存/退出屏障会主动结算仍在进行的预编辑。
  private settleImeComposition(force: boolean): void {
    if (!this.imeComposing) {
      return
    }
    if (!force && this.readPreviewTextLength() > 0) {
      // 预上屏文本仍在：不是已提交正文，普通保存屏障保留在原生未提交层；
      // 退出屏障另有 stopEditing() + 结算回调/超时路径，不会永久挂起。
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
    this.imeSettlementTimedOut = false
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
      // 预编辑已结束（无正文变化）：唤醒退出屏障的结算等待。
      this.notifyImeSettled()
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
    // 预编辑已结算并已入队：唤醒退出屏障的结算等待（真正的消费仍由串行 drain 完成）。
    this.notifyImeSettled()
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
          this.enterReconciling(`selection drain error: ${JSON.stringify(e)}`)
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
        this.enterReconciling(`selection flush error: ${JSON.stringify(e)}`)
      }
      this.checkIdle()
    })
  }

  private async drainSelectionIntent(sessionToken: number): Promise<void> {
    while (true) {
      if (sessionToken !== this.sessionId) {
        return
      }
      // Issue #1063：reconciling/conflict 时不提交选区事务（会话身份/正文基线都在变动）。
      if (this.sessionHealth !== 'healthy') {
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
        // 选区落点拆开了代理对等非法位置：本笔选区无法安全提交，进入 conflict。
        this.enterConflict(`invalid selection offset: anchor=${intent.startUtf16}, head=${intent.endUtf16},`
          + ` textLen=${intent.textForConversion.length}`)
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
        this.enterReconciling(`selection transaction failed: ${result.errorCode !== undefined ? result.errorCode : 'UNKNOWN'}`)
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
            // 同一 revision 重试仍 StaleRevision：Core 快照恢复无法自愈，进入 conflict。
            this.enterConflict('staleRevision: same revision retry failed, core snapshot recovery may have failed')
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
          this.enterConflict(`${outcome === EditorEditOutcome.InvalidOffset ? 'invalidOffset' : 'invalidRange'}:`
            + ' selection cannot be corrected')
          return
        }
        continue
      }

      if (this.pendingSelectionIntent !== intent) {
        continue
      }
      this.enterConflict(`unknown selection outcome: ${outcome}`)
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
    if (this.sessionHealth === 'healthy') {
      this.wakeDrain()
      return
    }
    // reconciling：冻结消费，保留全部未确认事件，并推进 reconcile；conflict 保留原状态。
    if (this.sessionHealth === 'reconciling') {
      this.scheduleReconcile()
    }
    this.checkIdle()
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
          this.enterReconciling(`unexpected error in drainPendingEdits: ${JSON.stringify(e)}`)
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
        this.enterReconciling(`unexpected error in wakeDrain: ${JSON.stringify(e)}`)
      }
      this.checkIdle()
    })
  }

  // 唯一消费者协程：从队首处理，单笔事务确定 Applied 后才取下一笔。
  private async drainPendingEdits(): Promise<void> {
    while (this.pendingEdits.length > 0) {
      // Issue #1063：reconciling/conflict 时冻结消费（pendingEdits 全部保留）。
      if (this.sessionHealth !== 'healthy') {
        return
      }

      const edit = this.pendingEdits[0]
      if (edit.sessionId !== this.sessionId) {
        this.pendingEdits.shift()
        continue
      }

      const snap = this.coordinator.getSnapshot()
      if (snap === null) {
        this.enterConflict('no active core session during pending edit drain')
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
          // 映射失败：不在这里丢事件，交给 reconcile 判定可验证 rebase 或 conflict。
          this.enterReconciling(`cannot map pending edit at offset ${edit.startUtf16}: overlapping diff`)
          return
        }
        const currentSlice = baselineText.substring(mapped.startUtf16, mapped.endUtf16)
        if (currentSlice !== edit.oldText) {
          this.enterReconciling(`cannot map pending edit at offset ${edit.startUtf16}: text mismatch`)
          return
        }
        mappedStart = mapped.startUtf16
        mappedEnd = mapped.endUtf16
      }

      const byteStart = TextOffsetMapper.utf16ToUtf8Checked(baselineText, mappedStart)
      const byteEnd = TextOffsetMapper.utf16ToUtf8Checked(baselineText, mappedEnd)
      if (byteStart === null || byteEnd === null) {
        this.enterReconciling(`invalid edit offset: start=${mappedStart}, end=${mappedEnd},`
          + ` textLen=${baselineText.length}`)
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
        // 事务失败：冻结游标，保留该 edit 与 base snapshot，进入可验证 rebase。
        this.enterReconciling(`transaction failed for edit at offset ${edit.startUtf16}`)
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
        this.updateEmptyDocumentIntent(edit, baselineText, newSnap.text, mappedStart, mappedEnd)
        if (result.data.outcome === EditorEditOutcome.AppliedWithAdjustedSelection &&
            this.richEditorController !== null) {
          const utf16Cursor = TextOffsetMapper.utf8ToUtf16(newSnap.text, newSnap.cursor)
          const utf16Anchor = TextOffsetMapper.utf8ToUtf16(newSnap.text, newSnap.selectionAnchor)
          this.richEditorController.setSelection(utf16Anchor, utf16Cursor)
        }
      }

      if (edit.sessionId === this.sessionId) {
        const paragraphsChanged = edit.insertedText.indexOf('\n') >= 0 || edit.oldText.indexOf('\n') >= 0
        this.notifyStateListener('edit-settled', paragraphsChanged,
          this.insertedParagraphStarts(edit.insertedText, edit.startUtf16))
      }
    }
  }

  // 只在编辑事务把整章正文由非空删成空、且删除范围覆盖全部旧正文时记录意图。
  // 程序性初始空视图、卸载、切章不会进入这里；新的非空提交会清理旧意图。
  private updateEmptyDocumentIntent(edit: PendingEdit, baselineText: string, newText: string,
    mappedStart: number, mappedEnd: number): void {
    if (newText.length > 0) {
      this.emptyDocumentIntent = null
      return
    }
    const coreSessionId = this.coordinator.getSessionId()
    if (coreSessionId === null || baselineText.length === 0) {
      return
    }
    // 以映射到当前 Core 基线的删除范围判断：一次性覆盖整章旧正文才算合法清空事务。
    const deletesWholeDocument: boolean = edit.insertedText.length === 0 &&
      mappedStart === 0 && mappedEnd >= baselineText.length
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

  private notifyStateListener(kind: string, paragraphsChanged: boolean,
    newParagraphStarts: number[] = [], becameNonEmpty: boolean = false): void {
    if (this.stateListener === null) {
      return
    }
    try {
      this.stateListener({
        kind: kind,
        paragraphsChanged: paragraphsChanged,
        newParagraphStarts: newParagraphStarts,
        becameNonEmpty: becameNonEmpty
      })
    } catch (e) {
      hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, `state listener error: ${JSON.stringify(e)}`)
    }
  }

  // 本次替换文本里由统一段落规则形成的新段落起点（UTF-16，绝对位置）。
  // 走 text_document_rules.ts 的统一段落规则（CRLF 是一个边界、U+2029 也是、
  // 孤立 CR 不是），与 Core paragraph_byte_ranges 一致；
  // 只用于核对段落样式继承，不参与 Core 正文事务。
  private insertedParagraphStarts(insertedText: string, insertionStart: number): number[] {
    return computeInsertedParagraphStarts(insertedText, insertionStart)
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
  // Issue #1063：reconciling/conflict 或还有 pendingEdits/IME 预编辑时不覆盖 RichEditor，
  // 保留用户可见正文（与状态机一致，不再读已删除的 idleError）。
  async syncFromCore(): Promise<void> {
    const sessionAtEntry = this.sessionId
    await this.editQueue

    if (this.sessionId !== sessionAtEntry) {
      return
    }

    if (this.sessionHealth !== 'healthy') {
      hilog.warn(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG,
        `syncFromCore: deferred because session is ${this.sessionHealth}`)
      return
    }

    if (this.pendingEdits.length > 0 || this.imeComposing) {
      hilog.warn(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, 'syncFromCore: deferred due to pending edits/IME')
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

  // ── Issue #1063 评论第 6 条：接续恢复滚动位置 ─────────────────────

  // 取消未完成的视口恢复重试（切章/会话终止/重新打开时成对清理）。
  private cancelViewportRestore(): void {
    if (this.viewportRestoreTimerId !== -1) {
      clearTimeout(this.viewportRestoreTimerId)
      this.viewportRestoreTimerId = -1
    }
    this.viewportRestoreAttempts = 0
  }

  /**
   * 接续恢复：把视口左上角的 Core UTF-8 byte offset 转成 UTF-16，滚到该字符可见。
   *
   * 约束：
   *   - 只有正文已加载（nativeTextReady）且章节布局完成（LayoutManager.lineCount > 0）才执行；
   *     布局未就绪按 BRIDGE_VIEWPORT_RESTORE_RETRY_MS 重试，上限 BRIDGE_VIEWPORT_RESTORE_MAX_ATTEMPTS。
   *   - byte offset 先 clamp 到当前正文 byte 长度，再转 UTF-16，并保证不落在代理对中间；
   *     topVisibleByteOffset < 0（未知）直接不动。
   *   - API >= 26 用 richEditorController.scrollToVisible({ start, end })，低版本按注释降级：
   *     不伪造 scrollTop 回写，保持原生当前位置。
   */
  restoreViewport(viewport: EditorViewportState): void {
    if (viewport.topVisibleByteOffset < 0) {
      return
    }
    this.cancelViewportRestore()
    const sessionAtEntry = this.sessionId
    const attemptRestore = (): void => {
      this.viewportRestoreTimerId = -1
      if (sessionAtEntry !== this.sessionId) {
        return
      }
      if (!this.nativeTextReady || this.richEditorController === null) {
        this.retryViewportRestore(sessionAtEntry, attemptRestore)
        return
      }
      let lineCount: number = 0
      try {
        const layoutManager = this.richEditorController.getLayoutManager()
        if (layoutManager === undefined || layoutManager === null) {
          this.retryViewportRestore(sessionAtEntry, attemptRestore)
          return
        }
        lineCount = layoutManager.getLineCount()
      } catch (e) {
        lineCount = -1
      }
      if (lineCount <= 0) {
        // 章节布局还没完成：滚到旧/空布局会落到错误位置，重试而不是假回写。
        this.retryViewportRestore(sessionAtEntry, attemptRestore)
        return
      }
      const text: string = this.currentText
      const textByteLength: number = TextOffsetMapper.utf16ToUtf8(text, text.length)
      const clampedByte: number = Math.max(0, Math.min(viewport.topVisibleByteOffset, textByteLength))
      let utf16Offset: number = TextOffsetMapper.utf8ToUtf16(text, clampedByte)
      // 保证不落在完整代理对中间（utf8ToUtf16 已左对齐，这里再按严格口径核对一次）。
      if (TextOffsetMapper.utf16ToUtf8Checked(text, utf16Offset) === null && utf16Offset > 0) {
        utf16Offset -= 1
      }
      const start: number = Math.max(0, Math.min(utf16Offset, text.length))
      const end: number = Math.min(text.length, start + 1)
      if (!PlatformApiResolver.getInstance().isApiAtLeast(BRIDGE_VIEWPORT_SCROLL_TO_VISIBLE_MIN_API)) {
        // 低版本降级：scrollToVisible 自 API 26 起可用；不做像素假回写，保持原生位置。
        hilog.info(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG,
          `restoreViewport skipped: scrollToVisible requires API ${BRIDGE_VIEWPORT_SCROLL_TO_VISIBLE_MIN_API}`
            + ` byteOffset=${clampedByte}`)
        return
      }
      try {
        this.richEditorController.scrollToVisible({ start: start, end: end })
        hilog.info(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG,
          `restoreViewport applied byteOffset=${clampedByte} utf16=${start} lineCount=${lineCount}`)
      } catch (e) {
        hilog.warn(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, `restoreViewport failed: ${JSON.stringify(e)}`)
      }
    }
    attemptRestore()
  }

  private retryViewportRestore(sessionToken: number, attemptRestore: () => void): void {
    if (sessionToken !== this.sessionId ||
        this.viewportRestoreAttempts >= BRIDGE_VIEWPORT_RESTORE_MAX_ATTEMPTS) {
      return
    }
    this.viewportRestoreAttempts++
    this.viewportRestoreTimerId = setTimeout((): void => {
      attemptRestore()
    }, BRIDGE_VIEWPORT_RESTORE_RETRY_MS)
  }
}
