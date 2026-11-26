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
// - whenIdle 为普通保存屏障：只等已提交 Core revision，预编辑保留在原生未提交层。
// - finishEditingAndWhenIdle 为退出/同步屏障：stopEditing() 后等真实 commit/cancel 信号；
//   没有确定结算就超时 reject，绝不把候选预编辑当正文或把超时当成功。
// - syncFromCore/loadInitialText 通过统一程序性入口 setStyledString，保留正文样式由组件构建。
// - 切章时以 sessionId 隔离旧回调，旧队列不可触碰新章节 UI。
//
// Issue #1063 评论第 4 条：会话健康状态机（替代只由 beginSession 重置的 idleError）
//   healthy ──镜像/事务与 Core 不一致──▶ reconciling ──可验证 rebase 收敛──▶ healthy
//                                   └──编辑事实无法证明/无法无歧义映射──▶ conflict
//   - reconciling：冻结 pendingEdits（消费游标不前进），把每笔编辑事实按队列顺序逐笔重基到
//     当前 Core（planPendingEditRebase 校验 beforeText/oldText/代理对边界），保留用户事务
//     cause 与事件事实；只有一侧变化时直接收敛。期间 idle waiters 保持挂起，syncFromCore
//     不回写覆盖原生正文。
//   - 复审评论第 2 条：提交 Core 成功 ≠ 原生已接受。Core 提交后先落 in-flight receipt；
//     原生之后的新输入只作为 receipt 原生基线的差量映射到 committedText，已经合并的 Core
//     改动不会被 native-only 覆盖；原生未读回一致前绝不把合并结果当下一轮共同基线。
//   - conflict：保留原生正文和全部未确认事件，不调用 syncFromCore 覆盖；waiters 立即以
//     明确原因 reject，诊断字段 nativeCoreDiverged 同步置 true。真实冲突把双方正文交给
//     Core 专用冲突材料入口持久保留，按「会话+内容版本」跟踪 pending/persisted/failed，
//     用户继续编辑会产生新版本材料，失败可重试。
//   - 复审评论第 2 条：conflict 不是终态。retryReconcile() 在当前会话内把状态切回
//     reconciling，基于当前 Core snapshot 与 receipt 重新重基；成功恢复 healthy，
//     失败保持 conflict 且一条事件不丢。beginSession 仍是会话替换语义，但不是唯一解锁路径。
//   - 失败不再用单一 idleError 锁死编辑：错误详情写在 conflictDetail（只读 getter 供诊断）。

import { EditorSessionCoordinator } from '../session/EditorSessionCoordinator'
import { computeMinimalTextDiff } from '../session/editor_patch_logic'
import {
  EditorSessionHealth,
  ReconcileSessionState,
  ConflictRecoveryResult
} from '../session/editor_reconcile_logic'
import {
  planPendingEditRebase,
  PendingEditFact,
  MappedPendingEdit,
  PendingEditRebasePlan
} from '../session/editor_merge_plan'
import { EditorConflictMaterialReceipt } from '../../../corebridge/dto/EditorConflictDtos'
import {
  ImeSettlementBarrier,
  ImeBarrierMode,
  ImeBarrierWaiter,
  ImeAlignmentFacts,
  ImeStateFacts,
  NativeEditorState,
  PreviewTextState,
  isBarrierWaiterSatisfied,
  IME_SETTLEMENT_SESSION_CHANGED,
  IME_SETTLEMENT_CANCELLED,
  IME_SETTLEMENT_CONFLICT,
  CORE_SNAPSHOT_UNAVAILABLE
} from './ime_settlement_barrier'
import { TextOffsetMapper } from './TextOffsetMapper'
import { describeLocalParagraphChange, LocalParagraphChange } from './text_document_rules'
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
// 退出屏障等待真实 IME commit/cancel 与 Core 应用回执的上限。
// Issue #1063 复审评论第 2 条：超时 reject IME_SETTLEMENT_TIMEOUT，不把退出 waiter 当成功，
// 也不按已提交 revision 冒充结算；每个 waiter 各自计时。
const BRIDGE_IME_SETTLEMENT_TIMEOUT_MS = 1200
// 普通保存屏障（committed-revision）的独立上限：Core snapshot/队列不推进时明确失败，
// 不把一次读取失败或未加载当成「已空闲」；会话/正文/pendingEdits 原样保留，可下次重试。
const BRIDGE_SAVE_BARRIER_TIMEOUT_MS = 3000
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

// Issue #1063 复审评论第 2、3 条：pendingEdits 逐笔重基到当前 Core 的纯逻辑在
// session/editor_merge_plan.ts（planPendingEditRebase）；本文件只负责冻结事实、逐笔提交、
// in-flight receipt 与原生写回确认。

// Issue #1063 复审评论第 4 条：Core 冲突材料成功写盘后的回执信息（含请求时的会话身份与
// 内容 key）。只有拿到非 null 回执才把该内容版本标记为 persisted。
export interface EditorConflictMaterialReceiptInfo {
  transactionId: string
  conflictDir: string
  coreContentSha256: string
  nativeContentSha256: string
  localSessionId: number
  coreSessionId: number
  conflictKey: string
}

// 冲突材料跟踪状态：以「会话身份 + 内容版本」为 key，而不是一次性 bool。
export type EditorConflictMaterialStatus = 'idle' | 'pending' | 'persisted' | 'failed'

export interface EditorConflictMaterialState {
  status: EditorConflictMaterialStatus
  key: string
  receipt: EditorConflictMaterialReceiptInfo | null
  reason: string
}

// Issue #1063 复审评论第 2 条：Core 已提交、原生尚未接受合并结果时的冻结回执。
//
// 关键不变量：committedText 只是 Core 侧的合并结果，不能当成下一轮两侧的共同基线。原生侧
// 共同基线是 nativeBaseText（「已确认进入 Core 的最后一笔 pending edit 之后」的原生全文）：
// 下一轮只把 queueSequence 更大的编辑事实从它逐笔重基到 committedText，已合并进 Core 的
// 改动不会被后续 native-only 合并覆盖。
interface InFlightMergeReceipt {
  localSessionToken: number
  coreSessionId: number
  nativeBaseText: string
  committedText: string
  committedRevision: number
  includedThroughSequence: number
  freezeChangeSequence: number
}

// Issue #1063 复审评论：真实冲突要保留双方正文。本类型只描述交给既有 Core 冲突材料
// 机制的输入，不引入新的持久化文件格式或第二套保存流程。
export interface EditorConflictPayload {
  // 原生编辑器里尚未进入 Core 的正文（Core 冲突材料的 incoming.md）。
  nativeText: string
  // 冲突发生时最近一次 Core 快照正文（Core 冲突材料的 local.md）。
  coreText: string
  // 冲突会话的共同基线（Core 冲突材料的 base.md）；有 in-flight receipt 时取原生侧基线。
  confirmedText: string
  // 进入冲突的原因（合并重叠/原生拒绝写入等）。
  detail: string
  // 本地会话令牌（beginSession 递增），用于诊断冲突属于哪一次打开章节。
  localSessionId: number
  // Core 编辑会话 id；无会话时为 -1。
  coreSessionId: number
  // 冲突时的 Core revision / 已确认 revision / 原生变更序号。
  coreRevision: number
  confirmedRevision: number
  changeSequence: number
  // 本次请求的内容版本 key（会话+revision+长度），用于 pending/persisted/failed 去重与重试。
  conflictKey: string
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
  // Issue #1063 复审评论第 2 条：在串行消费队列里的全局序号（跨 beginSession 重置）。
  // 屏障 waiter 绑定它，只有这笔事务真正进入 Core 后才允许放行退出屏障。
  queueSequence: number
  baseRevision: number
  beforeText: string
  startUtf16: number
  endUtf16: number
  insertedText: string
  oldText: string
  cause: EditorTransactionCause
}

// 屏障 waiter 的生命周期（timer/结算/会话终止）在 input/ime_settlement_barrier.ts，
// 本文件只提供 checkIdle 时的三方事实与结算条件。

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
  // Issue #1063 复审评论第 2 条：串行编辑队列的会话内序号。
  // nextEditSequence 在 enqueuePendingEdit 时分配，beginSession 时随会话重置；
  // appliedEditSequence 只在该笔事务真正应用进 Core（或 reconcile 确认已被 Core 覆盖）后推进。
  // 屏障 waiter 绑定「已入队到哪个序号」，而不是等下一次任意 onDidChange。
  private nextEditSequence: number = 0
  private appliedEditSequence: number = -1
  private confirmedText: string = ''
  private confirmedRevision: number = -1
  // Issue #1063：显式会话状态机（替代只由 beginSession 重置的 idleError）。
  // 复审评论第 2 条：conflict 不再是只能 beginSession 解锁的终态，retryReconcile 可在
  // 当前会话内重新 reconcile；状态机纯逻辑在 session/editor_reconcile_logic.ts。
  private sessionState: ReconcileSessionState = new ReconcileSessionState()
  private reconcileScheduled: boolean = false
  // 屏障 waiter（timer/结算/会话终止）全部在该账本里；每个 waiter 独立 deadline。
  // 普通保存 3s 超时 SAVE_BARRIER_TIMEOUT，退出/同步 1.2s 超时 IME_SETTLEMENT_TIMEOUT。
  private imeBarrier: ImeSettlementBarrier =
    new ImeSettlementBarrier(BRIDGE_IME_SETTLEMENT_TIMEOUT_MS, BRIDGE_SAVE_BARRIER_TIMEOUT_MS)
  // 接续恢复滚动的重试句柄（beginSession/切章时取消）。
  private viewportRestoreTimerId: number = -1
  private viewportRestoreAttempts: number = 0
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
  // 原生编辑器是否已加载过 Core 正文。未加载时 currentText 只是空占位，
  // 不能把它当成「原生正文与 Core 不一致」去收敛，否则会把 Core 正文清空。
  private nativeTextReady: boolean = false
  // 最近一次原生全文读取是否失败（nativeTextReady 之后仍读不到 → read-failed）。
  // 屏障用它区分「原生正文可读」与「读取失败」，后者绝不能当成三方一致。
  private lastNativeTextReadFailed: boolean = false
  // 组件 onEditingChange 的最新状态（失焦/取消这类无正文变化事件也会驱动 checkIdle）。
  private nativeEditing: boolean = false
  // Issue #1063 复审评论第 4 条：真实冲突时把双方正文交给 Core 专用材料入口的回调
  // （页面注入 WritingWorkflowCoordinator.persistEditorConflict）。
  // 只有返回非 null 回执才标记 persisted；null/reject 保持 failed 并可重试。
  private conflictPersistenceHandler:
    ((payload: EditorConflictPayload) => Promise<EditorConflictMaterialReceipt | null>) | null = null
  // 冲突材料按「会话身份 + 内容版本」跟踪 pending/persisted/failed；一次冲突内同 key 去重，
  // 用户冲突后继续编辑会产生新 key 并重新请求。失败可用 retryConflictPersistence() 重试。
  private conflictMaterialState: EditorConflictMaterialState = {
    status: 'idle',
    key: '',
    receipt: null,
    reason: ''
  }
  // Issue #1063 复审评论第 2 条：Core 已提交、原生尚未接受时的冻结回执（见 InFlightMergeReceipt）。
  private inFlightMerge: InFlightMergeReceipt | null = null
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

  // Issue #1063 复审评论第 4 条：真实冲突的持久化出口。页面只在会话存续期间注入；
  // 页面销毁时传 null，避免旧页面回调触碰已结束的写作流程。
  setConflictPersistenceHandler(
    handler: ((payload: EditorConflictPayload) => Promise<EditorConflictMaterialReceipt | null>) | null): void {
    this.conflictPersistenceHandler = handler
  }

  // Issue #1063 复审评论第 4 条：冲突材料的跟踪状态（诊断/页面展示与重试入口）。
  getConflictMaterialState(): EditorConflictMaterialState {
    return {
      status: this.conflictMaterialState.status,
      key: this.conflictMaterialState.key,
      receipt: this.conflictMaterialState.receipt,
      reason: this.conflictMaterialState.reason
    }
  }

  // 失败时用同一内容版本重发；内容已变化则由 requestConflictPersistence 自动算新 key。
  retryConflictPersistence(): void {
    if (this.conflictMaterialState.status !== 'failed') {
      return
    }
    const reason: string = this.conflictMaterialState.reason.length > 0
      ? this.conflictMaterialState.reason
      : (this.sessionState.getConflictDetail() ?? 'conflict')
    this.requestConflictPersistence(reason, true)
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
    return this.sessionState.getHealth()
  }

  // Issue #1063：conflict 的明确原因（healthy / 自动 reconciling 时为 null；
  // 手动 retryReconcile 进行中保留原冲突原因，直到本次重试给出结论）。
  getConflictDetail(): string | null {
    return this.sessionState.getConflictDetail()
  }

  // Issue #1063 复审评论第 2 条：会话内冲突重试入口。
  // 保留原生正文、原 base snapshot 与全部 pendingEdits；只有可验证 rebase 成功并写进
  // Core 后才恢复 healthy，失败仍保持 conflict 且事件一条不丢。
  async retryReconcile(): Promise<ConflictRecoveryResult> {
    if (!this.sessionState.isConflict()) {
      const health = this.sessionState.getHealth()
      return {
        recovered: health === 'healthy',
        errorCode: 'NOT_IN_CONFLICT',
        detail: `session is ${health}, nothing to retry`
      }
    }
    const sessionAtEntry = this.sessionId
    return new Promise<ConflictRecoveryResult>((resolve: (result: ConflictRecoveryResult) => void): void => {
      const task: Promise<void> = this.editQueue.then(async (): Promise<void> => {
        if (sessionAtEntry !== this.sessionId) {
          resolve({
            recovered: false,
            errorCode: IME_SETTLEMENT_SESSION_CHANGED,
            detail: 'session changed before manual conflict retry started'
          })
          return
        }
        if (!this.sessionState.beginManualRetry('manual conflict retry')) {
          resolve({
            recovered: this.sessionState.getHealth() === 'healthy',
            errorCode: 'NOT_IN_CONFLICT',
            detail: `session is ${this.sessionState.getHealth()}`
          })
          return
        }
        this.reconcileScheduled = false
        let deferred: boolean = false
        try {
          deferred = await this.reconcileSession()
        } catch (e) {
          this.enterConflict(`manual conflict retry error: ${JSON.stringify(e)}`)
        }
        if (sessionAtEntry !== this.sessionId) {
          resolve({
            recovered: false,
            errorCode: IME_SETTLEMENT_SESSION_CHANGED,
            detail: 'session changed during manual conflict retry'
          })
          return
        }
        if (deferred) {
          // 预编辑/原生变更尚未结算：保持 reconciling，等真实结算回调继续驱动；
          // 不在这里谎报恢复，也不丢弃 pendingEdits。
          this.scheduleReconcile()
          resolve({
            recovered: false,
            errorCode: 'RECONCILE_DEFERRED',
            detail: 'native change or IME composition still in flight'
          })
          return
        }
        const recovered: boolean = this.sessionState.getHealth() === 'healthy'
        resolve({
          recovered: recovered,
          errorCode: recovered ? '' : IME_SETTLEMENT_CONFLICT,
          detail: recovered ? '' : (this.sessionState.getConflictDetail() ?? 'reconcile did not converge')
        })
        if (!recovered) {
          // 既有保存重试也会到达这里：失败的材料按同一会话+内容版本重发一次。
          this.retryConflictPersistence()
        }
        this.checkIdle()
      }).catch((e: Object): void => {
        this.enterConflict(`manual conflict retry rejected: ${JSON.stringify(e)}`)
        resolve({
          recovered: false,
          errorCode: IME_SETTLEMENT_CONFLICT,
          detail: `manual conflict retry rejected: ${JSON.stringify(e)}`
        })
        this.checkIdle()
      })
      this.editQueue = task
    })
  }

  isImeComposing(): boolean {
    return this.imeComposing
  }

  // Issue #1063 复审评论第 2 条：组件 onEditingChange 的编辑态事实。
  // 失焦/取消这类没有正文变化的事件也要能驱动状态机：只更新内部状态并让 checkIdle
  // 重新读取 preview/三方事实，必要时把「preview 已明确取消且正文未变」结算成 cancel；
  // 绝不在这里 force 提交候选文本。
  notifyEditingStateChanged(isEditing: boolean): void {
    this.nativeEditing = isEditing
    hilog.info(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG,
      `editing state changed: isEditing=${this.nativeEditing} composing=${this.imeComposing}`)
    this.checkIdle()
    this.trySettleCancelledImeComposition()
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

  // 普通保存屏障（自动保存/退后台）：只等已提交 Core revision，不调用 stopEditing()。
  // Issue #1063 复审评论第 2 条：预编辑保留在独立未提交层，本屏障不检查 preview、
  // 不重置任何退出屏障的超时状态，因此用户停在拼音输入中也能保存已提交正文。
  whenIdle(): Promise<void> {
    this.barrierStage = 'save-when-idle'
    this.requestSelectionFlush()
    return this.pushIdleWaiter('committed-revision', false)
  }

  // 退出章节/同步屏障：stopEditing() 后只等待真实 IME commit/cancel 信号。
  // Issue #1063 复审评论第 2 条：
  // - 不调用 settleImeComposition(force) 强制提交候选文本；
  // - 每个 waiter 绑定 sessionId + 屏障 revision + 当前队列序号，各自独立超时；
  // - 超时 reject IME_SETTLEMENT_TIMEOUT，保留会话/正文/pendingEdits，由调用方失败收尾；
  // - 没有真实信号且 preview 仍非空时，绝不按已提交 revision 冒充成功。
  finishEditingAndWhenIdle(): Promise<void> {
    this.barrierStage = 'exit-finish-editing'
    if (this.richEditorController !== null) {
      try {
        this.richEditorController.stopEditing()
      } catch (e) {
        hilog.warn(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, `stopEditing failed: ${JSON.stringify(e)}`)
      }
    }
    // stopEditing() 可能在同步回调里结算预编辑；以调用后的真实状态为准。
    // - 已知仍在 composing，或 preview 明确存在：必须等真实 commit/cancel 信号；
    // - preview 读取失败（unknown）不设信号等待：后续事件会重新读取 preview，
    //   读到 empty 且三方同 revision 才放行，读不到则由各自 timer 超时失败；
    // - composing 已结束且 preview 明确为空：队列排空 + 同 revision 三方一致即可放行。
    const previewAfter: PreviewTextState = this.readPreviewTextState()
    const requireImeSignal: boolean = this.imeComposing || previewAfter === 'present'
    this.requestSelectionFlush()
    const waiterPromise: Promise<void> = this.pushIdleWaiter('ime-settlement', requireImeSignal)
    this.checkIdle()
    return waiterPromise
  }

  // Issue #1063：退出/切章/页面销毁时给所有 waiter 一个确定失败（不伪造空闲）。
  // 每个 waiter 的 timer/回调在账本里立即移除；调用方保留原会话与正文自行失败收尾。
  cancelIdleWaiters(reason: string): void {
    if (this.imeBarrier.pendingCount() === 0) {
      return
    }
    hilog.warn(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG,
      `cancel idle waiters: ${reason} count=${this.imeBarrier.pendingCount()}`)
    this.imeBarrier.rejectAll(IME_SETTLEMENT_CANCELLED, reason)
  }

  // ── Issue #1063：会话健康状态机 ────────────────────────────────

  /**
   * 进入 reconciling：本次会话的消费游标冻结（drain 不再取新 edit），保留全部 pendingEdits
   * 与失败发生前的 base snapshot（每条 edit 自带 beforeText）；由原串行 editQueue 提交
   * 可验证 rebase。reconciling 期间普通 waiter 按已提交 revision 放行，退出 waiter 等结算。
   */
  private enterReconciling(reason: string): void {
    if (!this.sessionState.enterReconciling(reason)) {
      // conflict 是确定结果：自动错误不降级，保留原冲突原因；只有 retryReconcile 能重试。
      return
    }
    hilog.warn(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG,
      `session entered reconciling: ${reason} pendingEdits=${this.pendingEdits.length}`)
    this.scheduleReconcile()
  }

  /**
   * 进入 conflict：保留原生正文与全部未确认事件，不调用 syncFromCore 回写覆盖原生文本；
   * 所有 idle waiters 以明确原因 reject。诊断字段 nativeCoreDiverged 与状态机保持一致，
   * 阻止 syncFromCore 覆盖仍在原生编辑器里的正文。
   *
   * Issue #1063 复审评论第 4 条：真实冲突把两份正文交给 Core 冲突材料入口；每次进入都
   * 按当前内容版本请求，同 key 的 pending/persisted 由 requestConflictPersistence 去重，
   * 用户继续编辑会生成新 key 并更新材料。
   */
  private enterConflict(reason: string): void {
    this.sessionState.enterConflict(reason)
    this.nativeCoreDiverged = true
    this.nativeCoreDivergedSessionId = this.coordinator.getSessionId()
    hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG,
      `session conflict: ${reason} pendingEdits=${this.pendingEdits.length}`
        + ` nativeLen=${this.currentText.length}`)
    this.rejectWaitersForConflict()
    this.requestConflictPersistence(reason)
  }

  /**
   * 把冲突双方的正文交给页面注入的 Core 冲突材料入口。
   *
   * Issue #1063 复审评论第 4 条：以「会话身份 + 冲突内容版本」为 key 跟踪
   * idle/pending/persisted/failed。同一内容版本 pending/persisted 时不重复提交；用户冲突后
   * 继续编辑会产生新 key 并提交最新材料；只有 handler 返回非 null 回执才标记 persisted，
   * null/reject 保持 failed（保留 key），可通过 retryConflictPersistence() 重发。
   */
  private requestConflictPersistence(reason: string, force: boolean = false): void {
    const snap: EditorSessionSnapshot | null = this.coordinator.getSnapshot()
    const coreSessionId: number = this.coordinator.getSessionId() ?? -1
    // 镜像可能落后于原生实际全文：持久化必须以原生当前正文为准，不能只存旧镜像。
    const liveNativeText: string | null = this.readNativeFullText()
    const nativeText: string = liveNativeText !== null ? liveNativeText : this.currentText
    const coreText: string = snap !== null ? snap.text : ''
    const coreRevision: number = snap !== null ? snap.revision : -1
    const key: string = this.conflictMaterialKey(this.sessionId, coreSessionId, coreRevision,
      this.confirmedRevision, this.changeSequence, nativeText.length, coreText.length)
    const state: EditorConflictMaterialState = this.conflictMaterialState
    if ((state.status === 'pending' || state.status === 'persisted') && state.key === key) {
      return
    }
    // failed 且内容没变时不自动重发：只有明确的重试入口（保存重试/retryConflictPersistence）
    // 才用同一内容版本再写一次，避免每次 checkIdle 都重复写 Core 材料。
    if (!force && state.status === 'failed' && state.key === key) {
      return
    }
    const handler = this.conflictPersistenceHandler
    if (handler === null) {
      // 页面未注入/已销毁：保持 idle，不伪造 persisted。
      this.conflictMaterialState = {
        status: 'idle',
        key: key,
        receipt: state.key === key ? state.receipt : null,
        reason: reason
      }
      return
    }
    const payload: EditorConflictPayload = {
      nativeText: nativeText,
      coreText: coreText,
      // 有 in-flight receipt 时，冲突是「已提交 Core 结果」与 receipt 之后原生新差量之间的
      // 三方问题，原生侧共同基线正是 receipt.nativeBaseText。
      confirmedText: this.inFlightMerge !== null ? this.inFlightMerge.nativeBaseText : this.confirmedText,
      detail: reason,
      localSessionId: this.sessionId,
      coreSessionId: coreSessionId,
      coreRevision: coreRevision,
      confirmedRevision: this.confirmedRevision,
      changeSequence: this.changeSequence,
      conflictKey: key
    }
    this.conflictMaterialState = { status: 'pending', key: key, receipt: null, reason: reason }
    hilog.warn(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG,
      `requesting conflict material persistence key=${key} nativeLen=${nativeText.length}`
        + ` confirmedLen=${payload.confirmedText.length} coreLen=${coreText.length}`)
    try {
      handler(payload).then((receipt: EditorConflictMaterialReceipt | null): void => {
        if (this.conflictMaterialState.key !== key
            || this.conflictMaterialState.status !== 'pending') {
          return
        }
        if (receipt === null) {
          // 没有成功回执绝不能标记 persisted：保留 key 供同版本重试。
          this.conflictMaterialState = { status: 'failed', key: key, receipt: null, reason: reason }
          hilog.warn(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG,
            `conflict material persistence returned null key=${key} detail=${reason}`)
          return
        }
        const info: EditorConflictMaterialReceiptInfo = {
          transactionId: receipt.transactionId,
          conflictDir: receipt.conflictDir,
          coreContentSha256: receipt.coreContentSha256,
          nativeContentSha256: receipt.nativeContentSha256,
          localSessionId: payload.localSessionId,
          coreSessionId: payload.coreSessionId,
          conflictKey: key
        }
        this.conflictMaterialState = { status: 'persisted', key: key, receipt: info, reason: reason }
        hilog.warn(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG,
          `conflict material persisted key=${key} transaction=${receipt.transactionId}`
            + ` dir=${receipt.conflictDir}`)
      }).catch((e: Object): void => {
        if (this.conflictMaterialState.key !== key
            || this.conflictMaterialState.status !== 'pending') {
          return
        }
        // 持久化失败只记录：冲突状态机与原生正文不受影响。
        this.conflictMaterialState = { status: 'failed', key: key, receipt: null, reason: reason }
        hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG,
          `conflict material persistence error key=${key}: ${JSON.stringify(e)}`)
      })
    } catch (e) {
      this.conflictMaterialState = { status: 'failed', key: key, receipt: null, reason: reason }
      hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG,
        `conflict material persistence threw key=${key}: ${JSON.stringify(e)}`)
    }
  }

  // 冲突内容版本 key：会话身份 + revision/变更序号 + 双方正文长度。任何一项变化都代表
  // 一份新的冲突材料版本，必须允许重新请求。
  private conflictMaterialKey(localSessionId: number, coreSessionId: number, coreRevision: number,
    confirmedRevision: number, changeSequence: number, nativeLength: number,
    coreLength: number): string {
    return `${localSessionId}:${coreSessionId}:${coreRevision}:${confirmedRevision}`
      + `:${changeSequence}:${nativeLength}:${coreLength}`
  }

  private rejectWaitersForConflict(): void {
    if (this.imeBarrier.pendingCount() === 0) {
      return
    }
    const detail = this.sessionState.getConflictDetail()
    const message = `Native/core session conflict: ${detail !== null ? detail : 'unknown'}`
    this.imeBarrier.rejectAll(IME_SETTLEMENT_CONFLICT, message)
  }

  // 把 reconcile 排到原串行 editQueue 尾部，保证不与 drain/选区事务并发。
  private scheduleReconcile(): void {
    if (this.sessionState.getHealth() !== 'reconciling' || this.reconcileScheduled) {
      return
    }
    this.reconcileScheduled = true
    const sessionToken = this.sessionId
    this.editQueue = this.editQueue.then(async (): Promise<void> => {
      this.reconcileScheduled = false
      if (sessionToken !== this.sessionId) {
        return
      }
      if (this.sessionState.getHealth() !== 'reconciling') {
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
   * 统一的会话三方合并（Issue #1063 复审评论第 2、3 条）。
   *
   * 关键不变量：**提交 Core 成功 ≠ 原生已接受合并结果**。Core 提交后到原生读回一致前，
   * 合并结果只记录在 in-flight receipt 的 committedText 里，不能当成下一轮两侧的共同基线；
   * 原生侧共同基线是「已确认进入 Core 的最后一笔 pending edit 之后」的原生全文。新的原生
   * 输入只作为它之后的差量逐笔重基到 committedText，已经合并进 Core 的改动不会被 native-only
   * 路径覆盖。
   *
   * 流程：
   *   1. 固定 localSessionToken + coreSessionId，本方法内不再换用别的 Core 会话；
   *   2. 冻结原生全文、Core snapshot、changeSequence，并按 receipt 取出尚未包含的 pendingEdits；
   *   3. 纯 converged 直接确认；没有编辑事实的原生差量只走显式 recovery；
   *   4. 其余交给 planPendingEditRebase 逐笔映射，按队列顺序逐笔提交 Core；每笔 await 后
   *      复核会话/IME/preview/原生变化，原生有变化就落 receipt 并 deferred；
   *   5. 全部提交且原生读回一致后才 confirm：按实际包含的 queueSequence 精确删除 pendingEdits，
   *      不按第一次冻结的数量盲切。
   *
   * @returns true 表示本轮因预编辑/原生变更未结算而推迟（调用方不要在同一条微任务链上
   *          立即重排，等结算回调再驱动），false 表示已给出确定结论（healthy 或 conflict）。
   */
  private async reconcileSession(): Promise<boolean> {
    if (this.sessionState.getHealth() !== 'reconciling') {
      return false
    }
    if (this.nativeInputInFlight()) {
      // 预编辑/原生变更未结算：镜像与 Core 都还不是稳定基线，等结算回调后重新合并。
      // 返回 true ≠ 已收敛：调用方不能在同一条微任务链上立即重排，否则会形成忙等循环。
      hilog.info(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG,
        'reconcile deferred: native change or IME composition still in flight')
      return true
    }
    const localToken: number = this.sessionId
    const coreSessionId: number | null = this.coordinator.getSessionId()
    if (coreSessionId === null) {
      this.enterConflict('no active core session during reconcile')
      return false
    }

    // 冻结本轮事实。原生实际全文是唯一真相；镜像落后只修正事实，不改 Core。
    const nativeText: string | null = this.readNativeFullText()
    if (nativeText === null) {
      this.enterConflict('native full text unavailable during reconcile')
      return false
    }
    if (nativeText !== this.currentText) {
      this.reportNativeFactError('reconcile',
        `mirror behind native text firstMismatch=${firstMismatchUtf16(this.currentText, nativeText)}`)
      this.currentText = nativeText
      this.nativeNewlineCount = countNewlines(nativeText)
    }
    const snapshot: EditorSessionSnapshot | null = this.coordinator.getSnapshot()
    if (snapshot === null) {
      this.enterConflict('core snapshot unavailable during reconcile')
      return false
    }
    const freezeChangeSequence: number = this.changeSequence

    const receipt: InFlightMergeReceipt | null = this.inFlightMerge
    let nativeBase: string
    let coreBase: string
    let included: number
    if (receipt === null) {
      nativeBase = this.confirmedText
      included = -1
    } else {
      if (receipt.localSessionToken !== localToken || receipt.coreSessionId !== coreSessionId) {
        // 不接受换一个会话继续合并：旧回执只属于它自己的会话。
        this.enterConflict('in-flight merge receipt belongs to another session')
        return false
      }
      nativeBase = receipt.nativeBaseText
      included = receipt.includedThroughSequence
    }
    // 一律以「当前」Core 正文为映射目标：receipt.committedText 只是上次提交的结果，
    // 期间 Core 可能又前进；nativeBase→当前 Core 的外部差量足以把剩余编辑逐笔映射，
    // 不会把已合并前缀重复提交。receipt 只提供原生侧基线与已包含序号。
    coreBase = snapshot.text

    // 纯 converged：Core 与原生都停在已确认基线，没有任何要合并的差量。
    if (receipt === null && snapshot.text === this.confirmedText
        && nativeText === this.confirmedText) {
      this.confirmRebaseConverged(nativeText, snapshot.revision)
      // 不凭文本相等推进未确认事件的序号；只有队首事实确实接在已确认基线上时才恢复
      // drain（否则保持冻结，等新的编辑/结算事件驱动，避免「等式收敛」反复重排）。
      if (this.pendingEdits.length > 0
          && this.pendingEdits[0].sessionId === localToken
          && this.pendingEdits[0].beforeText === nativeText) {
        this.wakeDrain()
      }
      return false
    }

    const facts: PendingEditFact[] = []
    for (let i = 0; i < this.pendingEdits.length; i++) {
      const edit: PendingEdit = this.pendingEdits[i]
      if (edit.sessionId !== localToken || edit.queueSequence <= included) {
        continue
      }
      facts.push({
        queueSequence: edit.queueSequence,
        beforeText: edit.beforeText,
        startUtf16: edit.startUtf16,
        endUtf16: edit.endUtf16,
        insertedText: edit.insertedText,
        oldText: edit.oldText
      })
    }

    // 没有任何原生编辑事实且原生仍停在 nativeBase：这是 Core-only 前进（同步/外部提交），
    // 直接把 Core 当前正文写回原生；receipt 场景下也由此完成「Core 已提交、原生未接受」的收尾。
    if (facts.length === 0 && nativeText === nativeBase) {
      return await this.convergeCoreOnly(
        localToken, coreSessionId, nativeText, snapshot, included, freezeChangeSequence)
    }

    // 没有原始编辑事实却有原生差量：只有 Core 仍停在原生侧基线时才能按明确恢复来源
    // 整段提交；Core 已包含其它改动时整段覆盖会丢掉它们，必须按冲突保留双方正文。
    if (facts.length === 0 && nativeText !== nativeBase && snapshot.text === nativeBase) {
      return await this.recoverExplicitNativeDelta(
        localToken, coreSessionId, snapshot, nativeText, included, freezeChangeSequence)
    }

    const plan: PendingEditRebasePlan =
      planPendingEditRebase(nativeBase, coreBase, nativeText, facts)
    if (!plan.ok) {
      // 重放链/映射不成立：保留原生正文、pendingEdits 与已有 receipt，进入真实冲突。
      this.enterConflict(plan.reason)
      return false
    }
    if (plan.edits.length !== facts.length) {
      // 纯逻辑契约：每笔 fact 必须产出一条 mapped（不跳过、不合并）。
      this.enterConflict('reconcile plan edit count mismatch')
      return false
    }

    let expectedText: string = coreBase
    let expectedRevision: number = snapshot.revision
    let lastIncluded: number = included
    // 已确认进入 Core 的那一段原生全文：中途 deferred 时它是下一轮原生侧共同基线。
    let includedNativeBase: string = nativeBase

    for (let i = 0; i < plan.edits.length; i++) {
      const mapped: MappedPendingEdit = plan.edits[i]
      // planPendingEditRebase 对每笔 fact 都产出一条 mapped（不跳过），顺序一致。
      const fact: PendingEditFact = facts[i]

      // 每笔事务前重新核对：会话、Core 基线与原生输入/预编辑都必须是冻结时的事实。
      if (!this.reconcileSessionFactsIntact(localToken, coreSessionId)) {
        return false
      }
      if (this.nativeInputInFlight()) {
        // 上一笔 await 期间原生又有新输入/预编辑：保留已提交事实，等结算后再继续。
        const currentSnap: EditorSessionSnapshot | null = this.coordinator.getSnapshot()
        this.saveInFlightMerge(localToken, coreSessionId, includedNativeBase,
          expectedText,
          currentSnap !== null ? currentSnap.revision : this.lastAppliedRevision,
          lastIncluded, freezeChangeSequence)
        return true
      }
      const before: EditorSessionSnapshot | null = this.coordinator.getSnapshot()
      if (before === null || before.text !== expectedText) {
        this.enterConflict('core text changed during reconcile')
        return false
      }
      const byteStart: number | null =
        TextOffsetMapper.utf16ToUtf8Checked(expectedText, mapped.startUtf16)
      const byteEnd: number | null =
        TextOffsetMapper.utf16ToUtf8Checked(expectedText, mapped.endUtf16)
      if (byteStart === null || byteEnd === null) {
        // 映射后的坐标切在代理对中间等非法位置：不能提交半个字符。
        this.enterConflict(`invalid mapped edit offset during reconcile`
          + ` start=${mapped.startUtf16} end=${mapped.endUtf16}`)
        return false
      }
      const cause: EditorTransactionCause | null = this.pendingEditCause(mapped.queueSequence)
      if (cause === null) {
        this.enterConflict(`pending edit #${mapped.queueSequence} disappeared during reconcile`)
        return false
      }

      // 必须按队列顺序逐笔提交：每笔 mapped 坐标都是提交到「当时的 Core 正文」的坐标。
      // Issue #1063 复审评论 #7194530106：不能先用冻结快照算坐标、再让 enqueueEdit 在出队时
      // 读最新 revision 直接执行；统一走 conditionalEdit，在同一个 commandQueue 闭包里核对
      // session/revision/整篇正文/oldText，通过后才按真实 cause 提交。
      let result: ResultEnvelope<EditorEditResult> | null = null
      if (mapped.insertedText.length > 0 || mapped.oldText.length > 0) {
        result = await this.coordinator.conditionalEdit(
          coreSessionId, expectedRevision, expectedText,
          byteStart, byteEnd, mapped.insertedText, mapped.oldText, cause)
      } else {
        // 事实本身没有正文变化（inserted/oldText 都为空）：不下发事务，直接视为已包含。
        lastIncluded = mapped.queueSequence
        includedNativeBase = includedNativeBase.substring(0, fact.startUtf16)
          + fact.insertedText
          + includedNativeBase.substring(fact.endUtf16)
        continue
      }

      // 每次 await 后都不跨事实使用旧结论。
      if (!this.reconcileSessionFactsIntact(localToken, coreSessionId)) {
        return false
      }
      if (result !== null && !result.success && result.errorCode === 'STALE_BASELINE') {
        // 出队核实时基线已前进、这笔事务尚未应用：保留已提交前缀的 receipt，
        // 重新调度本轮把剩余事实映射到新 Core 正文，不进入 conflict、也不重复提交。
        const staleSnap: EditorSessionSnapshot | null = this.coordinator.getSnapshot()
        this.saveInFlightMerge(localToken, coreSessionId, includedNativeBase,
          staleSnap !== null ? staleSnap.text : expectedText,
          staleSnap !== null ? staleSnap.revision : expectedRevision,
          lastIncluded, freezeChangeSequence)
        this.scheduleReconcile()
        return true
      }
      if (result !== null && !result.success && result.errorCode === 'SESSION_CHANGED') {
        // 原会话任务到此结束：本地会话已替换就静默放弃；Core 会话被换则明确冲突。
        if (this.sessionId === localToken && this.coordinator.getSessionId() !== coreSessionId) {
          this.enterConflict('core session changed during reconcile edit')
        }
        return false
      }
      if (result === null || !result.success || result.data === undefined
          || (result.data.outcome !== EditorEditOutcome.Applied
              && result.data.outcome !== EditorEditOutcome.AppliedWithAdjustedSelection
              && result.data.outcome !== EditorEditOutcome.NoChange)) {
        // 失败/非预期 outcome：保留 receipt 与 pendingEdits。
        const failureSnap: EditorSessionSnapshot | null = this.coordinator.getSnapshot()
        this.saveInFlightMerge(localToken, coreSessionId, includedNativeBase,
          failureSnap !== null ? failureSnap.text : expectedText,
          failureSnap !== null ? failureSnap.revision : expectedRevision,
          lastIncluded, freezeChangeSequence)
        this.enterConflict(`reconcile transaction failed:`
          + ` ${result !== null && result.errorCode !== undefined ? result.errorCode : 'UNKNOWN'}`)
        return false
      }

      const after: EditorSessionSnapshot | null = this.coordinator.getSnapshot()
      if (after === null) {
        this.saveInFlightMerge(localToken, coreSessionId, includedNativeBase,
          expectedText, expectedRevision, lastIncluded, freezeChangeSequence)
        this.enterConflict('core snapshot unavailable after reconcile transaction')
        return false
      }
      const noChange: boolean = result.data.outcome === EditorEditOutcome.NoChange
      const expectedAfter: string = noChange ? expectedText
        : expectedText.substring(0, mapped.startUtf16)
          + mapped.insertedText
          + expectedText.substring(mapped.endUtf16)
      const nextIncludedBase: string = includedNativeBase.substring(0, fact.startUtf16)
        + fact.insertedText
        + includedNativeBase.substring(fact.endUtf16)
      if (after.text !== expectedAfter) {
        // 事务已 Applied，不能把这笔编辑当成未提交：回执按实际已包含的 queueSequence 前进，
        // 下一轮只处理它之后的编辑；committedText 取 Core 实际正文，避免重复提交同一笔。
        this.saveInFlightMerge(localToken, coreSessionId, nextIncludedBase,
          after.text, after.revision, mapped.queueSequence, freezeChangeSequence)
        this.enterConflict('core text changed during reconcile transaction')
        return false
      }

      // 提交成功：立刻推进 receipt（已包含序号 + 原生侧基线），不能等整批结束才记录，
      // 否则后续分支进入 conflict 时这批已应用事件会丢，重试可能重复提交。
      // Issue #1063 复审 #7195630106：重基与正常输入共用同一已应用事务结算，
      // 用映射到真实 Core 基线的范围与 before/after 正文判断合法清空。
      this.settleAppliedTransaction(coreSessionId, mapped.queueSequence,
        expectedText, after.text, mapped.startUtf16, mapped.endUtf16,
        mapped.insertedText, after.revision, cause)
      expectedText = expectedAfter
      expectedRevision = after.revision
      lastIncluded = mapped.queueSequence
      includedNativeBase = nextIncludedBase
      this.saveInFlightMerge(localToken, coreSessionId, includedNativeBase,
        expectedText, expectedRevision, lastIncluded, freezeChangeSequence)
    }

    if (expectedText !== plan.resultText) {
      this.enterConflict('reconcile plan result mismatch after committing edits')
      return false
    }

    // 全部编辑事实已提交：重新读取原生/预览/Core 三方事实，不跨 await 用旧结论。
    const nativeTextNow: string | null = this.readNativeFullText()
    const previewNow: PreviewTextState = this.readPreviewTextState()
    const snapshotNow: EditorSessionSnapshot | null = this.coordinator.getSnapshot()
    if (snapshotNow === null) {
      this.enterConflict('core snapshot unavailable after reconcile')
      return false
    }
    if (snapshotNow.text !== expectedText) {
      this.enterConflict('core text changed during reconcile')
      return false
    }
    if (nativeTextNow === null
        || nativeTextNow !== nativeText
        || this.pendingNativeChange !== null
        || this.imeComposing
        || previewNow === 'present'
        || (this.changeSequence !== freezeChangeSequence && nativeTextNow !== expectedText)) {
      // 原生尚未接受/已变化：不能把合并结果当共同基线，也不能覆盖用户新输入。
      if (lastIncluded > included) {
        this.saveInFlightMerge(localToken, coreSessionId, includedNativeBase,
          expectedText, snapshotNow.revision, lastIncluded, freezeChangeSequence)
      }
      return true
    }

    // 写原生之前先落 receipt：原生若拒绝写入，下一轮仍从 committedText 继续，不重复提交。
    if (lastIncluded > included) {
      this.saveInFlightMerge(localToken, coreSessionId, includedNativeBase,
        expectedText, snapshotNow.revision, lastIncluded, freezeChangeSequence)
    }
    if (nativeTextNow !== expectedText) {
      const applied: boolean = this.applyProgrammaticText(expectedText)
      const acceptedText: string | null = applied ? this.readNativeFullText() : null
      if (!applied || acceptedText === null || acceptedText !== expectedText) {
        this.enterConflict(`native editor did not accept merged text applied=${applied}`
          + ` acceptedLen=${acceptedText !== null ? acceptedText.length : -1}`
          + ` mergedLen=${expectedText.length}`)
        return false
      }
      this.currentText = expectedText
      this.nativeNewlineCount = countNewlines(expectedText)
    }

    // 只有原生读回逐字一致后才确认共同基线，并精确删除已包含的 pendingEdits。
    this.confirmRebaseConverged(expectedText, snapshotNow.revision)
    this.confirmIncludedEdits(lastIncluded)
    hilog.info(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG,
      `reconcile converged: revision=${snapshotNow.revision} included=${lastIncluded}`
        + ` edits=${plan.edits.length} nativeBaseLen=${includedNativeBase.length}`)
    this.refreshSelectionAfterConvergence()
    this.wakeDrain()
    return false
  }

  /**
   * 没有原生编辑事实、原生停在共同基线时的 Core-only 收敛：Core 前进（同步/外部提交），
   * 原生只需要接受 Core 当前正文。receipt 存在时也走这里收尾，绝不再把 committedText
   * 当成新基线去和「没有差量的原生」做 native-only 比较。
   *
   * 检查与写原生之间没有 await；写入后读回必须逐字等于目标正文才确认。
   */
  private async convergeCoreOnly(localToken: number, coreSessionId: number,
    observedNativeText: string, snapshot: EditorSessionSnapshot, included: number,
    freezeChangeSequence: number): Promise<boolean> {
    if (this.sessionId !== localToken) {
      return false
    }
    if (this.coordinator.getSessionId() !== coreSessionId) {
      this.enterConflict('core session changed during core-only convergence')
      return false
    }
    const targetText: string = snapshot.text
    if (this.nativeInputInFlight()) {
      // 原生在判定期间又出现输入/预编辑：保留 receipt（原生基线仍是 observedNativeText），
      // 等真实结算后再收敛。
      this.saveInFlightMerge(localToken, coreSessionId, observedNativeText,
        targetText, snapshot.revision, included, freezeChangeSequence)
      return true
    }
    if (observedNativeText === targetText) {
      this.confirmRebaseConverged(targetText, snapshot.revision)
      this.confirmIncludedEdits(included)
      this.wakeDrain()
      return false
    }
    const applied: boolean = this.applyProgrammaticText(targetText)
    const acceptedText: string | null = applied ? this.readNativeFullText() : null
    if (!applied || acceptedText === null || acceptedText !== targetText) {
      this.enterConflict(`native editor did not accept core-only text applied=${applied}`
        + ` acceptedLen=${acceptedText !== null ? acceptedText.length : -1}`
        + ` targetLen=${targetText.length}`)
      return false
    }
    const after: EditorSessionSnapshot | null = this.coordinator.getSnapshot()
    if (after === null || after.text !== targetText
        || this.coordinator.getSessionId() !== coreSessionId) {
      this.enterConflict('core text changed during core-only convergence')
      return false
    }
    this.currentText = targetText
    this.nativeNewlineCount = countNewlines(targetText)
    this.confirmRebaseConverged(targetText, after.revision)
    this.confirmIncludedEdits(included)
    hilog.info(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG,
      `reconcile core-only converged: revision=${after.revision} included=${included}`)
    this.refreshSelectionAfterConvergence()
    this.wakeDrain()
    return false
  }

  /**
   * 没有原始编辑事实的原生差量（facts 为空、Core 仍停在 coreBase）按明确恢复来源处理：
   * 用程序性事务把原生全文提交为 Core 正文；成功且原生读回一致才确认。
   * 提交期间原生再次变化/出现预编辑则保存 receipt 并 deferred；Core 前进或事务失败则 conflict。
   */
  private async recoverExplicitNativeDelta(localToken: number, coreSessionId: number,
    snapshot: EditorSessionSnapshot, nativeText: string, included: number,
    freezeChangeSequence: number): Promise<boolean> {
    if (snapshot.text === nativeText) {
      // Core 已经是原生全文：无需事务，直接确认（没有任何编辑事实可提交）。
      this.confirmRebaseConverged(nativeText, snapshot.revision)
      this.confirmIncludedEdits(included)
      hilog.info(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG,
        `reconcile explicit native recovery: text already converged revision=${snapshot.revision}`)
      this.wakeDrain()
      return false
    }
    const result = await this.coordinator.replaceSliceIfUnchanged(
      coreSessionId, snapshot.revision, snapshot.text, nativeText, EditorTransactionCause.Programmatic)
    if (this.sessionId !== localToken) {
      return false
    }
    if (this.coordinator.getSessionId() !== coreSessionId) {
      this.enterConflict('core session changed during native recovery')
      return false
    }
    if (!result.success || result.data === undefined
        || (result.data.outcome !== EditorEditOutcome.Applied
            && result.data.outcome !== EditorEditOutcome.AppliedWithAdjustedSelection
            && result.data.outcome !== EditorEditOutcome.NoChange)) {
      this.enterConflict(`explicit native recovery failed:`
        + ` ${result.errorCode !== undefined ? result.errorCode : 'UNKNOWN'}`)
      return false
    }
    const after: EditorSessionSnapshot | null = this.coordinator.getSnapshot()
    if (after === null || after.text !== nativeText) {
      this.enterConflict('core text changed during native recovery')
      return false
    }
    const nativeTextNow: string | null = this.readNativeFullText()
    const previewNow: PreviewTextState = this.readPreviewTextState()
    if (nativeTextNow !== nativeText
        || this.pendingNativeChange !== null
        || this.imeComposing
        || previewNow === 'present') {
      // Core 已提交这份原生全文，但原生在提交期间又变了：保留 receipt，等结算后再处理差量。
      this.saveInFlightMerge(localToken, coreSessionId, nativeText,
        nativeText, after.revision, included, freezeChangeSequence)
      return true
    }
    this.confirmRebaseConverged(nativeText, after.revision)
    this.confirmIncludedEdits(included)
    hilog.info(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG,
      `reconcile explicit native recovery: committed revision=${after.revision}`
        + ` nativeLen=${nativeText.length}`)
    this.refreshSelectionAfterConvergence()
    this.wakeDrain()
    return false
  }

  // 原生是否有尚未结算的输入事实：任何 await 之后都必须复核。
  private nativeInputInFlight(): boolean {
    return this.pendingNativeChange !== null || this.imeComposing
      || this.readPreviewTextState() === 'present'
  }

  // 固定会话在 await 后是否仍然成立。本地会话被替换直接放弃本轮（新会话由 beginSession
  // 重置状态机）；Core 会话被替换属于失配，按 conflict 收尾，不换用别的 session 继续合并。
  private reconcileSessionFactsIntact(localToken: number, coreSessionId: number): boolean {
    if (this.sessionId !== localToken) {
      return false
    }
    if (this.coordinator.getSessionId() !== coreSessionId) {
      this.enterConflict('core session changed during reconcile')
      return false
    }
    return true
  }

  private pendingEditCause(queueSequence: number): EditorTransactionCause | null {
    for (let i = 0; i < this.pendingEdits.length; i++) {
      if (this.pendingEdits[i].queueSequence === queueSequence) {
        return this.pendingEdits[i].cause
      }
    }
    return null
  }

  // 记下「Core 已提交到 includedThroughSequence、原生尚未接受」的冻结回执。
  private saveInFlightMerge(localToken: number, coreSessionId: number, nativeBaseText: string,
    committedText: string, committedRevision: number, includedThroughSequence: number,
    freezeChangeSequence: number): void {
    this.inFlightMerge = {
      localSessionToken: localToken,
      coreSessionId: coreSessionId,
      nativeBaseText: nativeBaseText,
      committedText: committedText,
      committedRevision: committedRevision,
      includedThroughSequence: includedThroughSequence,
      freezeChangeSequence: freezeChangeSequence
    }
  }

  // 按实际已包含的 queueSequence 精确删除 pendingEdits（不再按第一次冻结数量盲切），
  // 同时推进退出屏障使用的 appliedEditSequence。
  private confirmIncludedEdits(lastIncluded: number): void {
    if (lastIncluded > this.appliedEditSequence) {
      this.appliedEditSequence = lastIncluded
    }
    if (lastIncluded < 0) {
      return
    }
    const remaining: PendingEdit[] = []
    for (let i = 0; i < this.pendingEdits.length; i++) {
      if (this.pendingEdits[i].queueSequence > lastIncluded) {
        remaining.push(this.pendingEdits[i])
      }
    }
    this.pendingEdits = remaining
  }

  // 三方合并收敛：三方状态、revision 与 confirmed 基线一起对齐后恢复 healthy。
  // 原生全文在这一刻已被读回核对，因此编辑器正文一定已加载（不许再当空占位）。
  // 收敛即清除 in-flight receipt：下一轮从 confirmedText 重新开始。
  private confirmRebaseConverged(text: string, revision: number): void {
    this.lastAppliedRevision = revision
    this.confirmedText = text
    this.confirmedRevision = revision
    this.coreNewlineCount = countNewlines(text)
    this.nativeCoreDiverged = false
    this.nativeCoreDivergedSessionId = null
    this.inFlightMerge = null
    this.nativeTextReady = true
    this.sessionState.confirmHealthy()
  }

  // 普通保存屏障只冻结已提交 Core revision：预编辑留在原生未提交层。
  // 每次都从 Core snapshot 现算 committedStable，不读共享超时布尔。
  // previewState 传 'present' 表示「按 IME 结算未完成处理」：普通 waiter 走已提交 revision，
  // 退出 waiter 继续等待，不会被这里的放行逻辑误结算。
  private settleCommittedRevisionWaiters(): void {
    this.settleWaiters(this.buildCoreOnlyAlignmentFacts(), 'present', true)
  }

  // 原生编辑器当前状态（屏障事实）：未绑定 / 未加载 / 已加载但读不到 / 可读。
  private coreOnlyNativeEditorState(): NativeEditorState {
    if (this.richEditorController === null) {
      return 'unbound'
    }
    if (!this.nativeTextReady) {
      return 'loading'
    }
    return this.lastNativeTextReadFailed ? 'read-failed' : 'ready'
  }

  // 没有原生全文读取的需求时（普通保存 + 预编辑中），只需 Core revision/文本与 confirmed 基线。
  private buildCoreOnlyAlignmentFacts(): ImeAlignmentFacts {
    const snap = this.coordinator.getSnapshot()
    return {
      coreAvailable: snap !== null,
      nativeEditorState: this.coreOnlyNativeEditorState(),
      aligned: false,
      coreRevision: snap !== null ? snap.revision : -1,
      appliedRevision: this.lastAppliedRevision,
      coreText: snap !== null ? snap.text : '',
      confirmedText: this.confirmedText,
      coreSessionId: this.coordinator.getSessionId() ?? -1
    }
  }

  // 三方校验结果 → 屏障事实。nativeAvailable 来自同一次原生全文读取：
  // 已加载但读不到就是 read-failed，不能当成「没有原生编辑器」或「已对齐」。
  private buildAlignmentFacts(alignment: NativeCoreAlignmentResult): ImeAlignmentFacts {
    const snap = this.coordinator.getSnapshot()
    let nativeEditorState: NativeEditorState = 'unbound'
    if (this.richEditorController !== null) {
      if (!this.nativeTextReady) {
        nativeEditorState = 'loading'
      } else {
        nativeEditorState = alignment.nativeAvailable ? 'ready' : 'read-failed'
      }
    }
    return {
      coreAvailable: alignment.coreAvailable,
      nativeEditorState: nativeEditorState,
      aligned: alignment.matched,
      coreRevision: alignment.coreRevision,
      appliedRevision: alignment.appliedRevision,
      coreText: snap !== null ? snap.text : '',
      confirmedText: this.confirmedText,
      coreSessionId: this.coordinator.getSessionId() ?? -1
    }
  }

  // 按纯逻辑判定结算所有满足条件的 waiter。返回本轮结算数量。
  // imeComposing 由调用方给出真实 composition 状态；previewState 单独交给退出屏障判定。
  private settleWaiters(alignment: ImeAlignmentFacts, previewState: PreviewTextState,
    imeComposing: boolean): number {
    const waiters: ImeBarrierWaiter[] = this.imeBarrier.waiters()
    const state: ImeStateFacts = {
      imeComposing: imeComposing,
      previewState: previewState
    }
    let settled: number = 0
    for (const waiter of waiters) {
      if (waiter.sessionId !== this.sessionId) {
        continue
      }
      if (!isBarrierWaiterSatisfied(waiter, alignment, state, this.appliedEditSequence)) {
        continue
      }
      if (this.imeBarrier.settle(waiter.id)) {
        settled++
      }
    }
    return settled
  }

  private pushIdleWaiter(mode: ImeBarrierMode, requireImeSignal: boolean): Promise<void> {
    const registration = this.imeBarrier.register(
      this.sessionId, this.coordinator.getSessionId() ?? -1,
      this.lastAppliedRevision, this.nextEditSequence - 1, mode, requireImeSignal)
    this.checkIdle()
    return registration.promise
  }

  // 真实 IME commit（有正文差量）绑定到队列序号；cancel/空提交只清除信号标记。
  // 结算动作本身由 checkIdle 在 Core 应用并三方对齐后完成。
  private notifyImeSettled(kind: string, queueSequence: number): void {
    if (kind === 'commit' && queueSequence >= 0) {
      this.imeBarrier.bindImeCommitSequence(this.sessionId, queueSequence)
    } else {
      this.imeBarrier.clearImeSignal(this.sessionId)
    }
    this.checkIdle()
  }

  // 预编辑已明确取消且正文未变：结束 composition，但不产生 Core 事务。
  // 只在 previewText 成功读到空、且基线到当前镜像没有差量时调用。
  private trySettleCancelledImeComposition(): void {
    if (!this.imeComposing) {
      return
    }
    if (this.readPreviewTextState() !== 'empty') {
      return
    }
    const diff = this.computeDiff(this.imeComposingStartText, this.currentText)
    if (diff.start < diff.end || diff.replacement.length > 0) {
      // 正文已有差量：这可能是尚未到达的 commit 回调，等真实信号，绝不在这里提交。
      return
    }
    this.commitImeCompositionFromBaseline('ime-cancel')
  }

  // 事件驱动检查：先按会话状态机处理确定结果，再等 editQueue tail 读取三方事实。
  private checkIdle(): void {
    const health: EditorSessionHealth = this.sessionState.getHealth()
    if (health === 'reconciling') {
      // reconciling：退出 waiter 保持挂起（不 reject）；即使暂无 waiter 也要推进 reconcile，
      // 否则冻结的 pendingEdits 与消费游标会永远停住。
      this.scheduleReconcile()
      if (this.imeComposing) {
        // 预编辑使 reconcile 暂时无法收敛：普通保存屏障按已提交 Core revision 放行，
        // 不把自动保存挂在用户拼音输入时长上；退出屏障继续等真实结算。
        this.settleCommittedRevisionWaiters()
      }
      return
    }
    if (health === 'conflict') {
      // conflict 是确定结果：waiters 立刻拿到明确原因，不伪装空闲。
      // 用户冲突后继续编辑（changeSequence/长度变化）会生成新 key，触发最新材料持久化。
      const detail: string = this.sessionState.getConflictDetail() ?? 'conflict active'
      this.requestConflictPersistence(detail)
      this.rejectWaitersForConflict()
      return
    }
    if (this.imeBarrier.pendingCount() === 0) {
      return
    }

    // 退出屏障：preview 已明确取消且正文未变时结束 composition（不提交候选正文）。
    if (this.imeBarrier.hasImeSettlementWaiter(this.sessionId)) {
      this.trySettleCancelledImeComposition()
    }

    const currentTail = this.editQueue
    currentTail.then(() => {
      if (this.imeBarrier.pendingCount() === 0) {
        return
      }

      if (this.editQueue !== currentTail) {
        this.checkIdle()
        return
      }

      const currentHealth: EditorSessionHealth = this.sessionState.getHealth()
      if (currentHealth === 'conflict') {
        this.requestConflictPersistence(this.sessionState.getConflictDetail() ?? 'conflict active')
        this.rejectWaitersForConflict()
        return
      }
      if (currentHealth === 'reconciling') {
        this.scheduleReconcile()
        if (this.imeComposing) {
          this.settleCommittedRevisionWaiters()
        }
        return
      }

      if (this.pendingEdits.length > 0 || this.drainRunning) {
        return
      }

      if (this.imeComposing) {
        // 普通保存屏障可先保存已提交 Core revision；退出屏障等真实 commit/cancel。
        // 这里不再读取任何共享超时布尔，也不把退出 waiter 当成功。
        this.settleCommittedRevisionWaiters()
        if (this.imeBarrier.hasImeSettlementWaiter(this.sessionId)) {
          this.trySettleCancelledImeComposition()
        }
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

      // 三方事实只读一次：普通保存要 native/mirror/core 一致；退出屏障还要求
      // preview 明确为空且 Core revision 等于已应用 revision。
      const alignment = this.validateNativeCoreAlignment(this.barrierStage)
      if (alignment.matched) {
        // 三方已一致：解除阻止 Core → 原生回写的分歧标记。
        this.nativeCoreDiverged = false
        this.nativeCoreDivergedSessionId = null
      }
      const previewState: PreviewTextState = this.readPreviewTextState()
      // composition 状态与 preview 分开传递：退出屏障要求 preview 明确为空，
      // 普通保存只看已提交 revision，不因预编辑/读取失败被拖住。
      this.settleWaiters(this.buildAlignmentFacts(alignment), previewState, this.imeComposing)
      if (this.imeBarrier.pendingCount() === 0) {
        return
      }

      // Core 会话还在但读不到快照：没有任何可对齐的基线。等待者必须拿到确定失败，
      // 不能静默 return 挂到超时（各自 timer 只是最后兜底）。
      if (!alignment.coreAvailable) {
        if (this.coordinator.getSessionId() !== null) {
          this.imeBarrier.rejectSession(this.sessionId, CORE_SNAPSHOT_UNAVAILABLE,
            `core snapshot unavailable at ${this.barrierStage}`
              + ` mirrorLen=${alignment.nativeUtf16Length}`)
        }
        return
      }

      const nativeEditorLoaded = this.richEditorController !== null && this.nativeTextReady
      if (alignment.matched || !nativeEditorLoaded) {
        // 已对齐：没有待收敛；原生未加载/不可读：等待者由自己的 timer 给出确定结果。
        return
      }

      // preview 读取失败或仍有预编辑：不能把原生候选当正文收敛进 Core。
      if (this.imeComposing || previewState !== 'empty') {
        return
      }

      // Issue #1063 复审评论：未对齐时不再有「Core 前进就刷新原生 / 原生前进就收敛 Core」
      // 两套独立覆盖路径。统一进入三方合并 reconcile：只有确实没有待结算的原生事实时才调度，
      // 否则保持原 gating（等待输入/IME/选区结算后再由回调驱动）。
      if (this.sessionState.isConflict()) {
        return
      }
      const blocked: boolean = this.pendingEdits.length > 0 || this.drainRunning ||
        this.imeComposing || this.pendingNativeChange !== null || this.pendingSelectionIntent !== null
      if (blocked) {
        return
      }
      this.enterReconciling(`native/core mismatch: stage=${this.barrierStage}`
        + ` mirrorLen=${alignment.nativeUtf16Length} coreLen=${alignment.coreUtf16Length}`
        + ` firstMismatch=${alignment.firstMismatchUtf16}`
        + ` nativeAvailable=${alignment.nativeAvailable}`)
      // enterReconciling 已调度一次；这里显式再调一次只是钉住「同一轮只调度一次」的语义
      // （scheduleReconcile 自身幂等）。
      this.scheduleReconcile()
    })
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

    // Issue #1063 复审评论第 2 条：会话结束立即移除旧 waiter 的回调与 timer。
    this.imeBarrier.rejectSession(oldSession, IME_SETTLEMENT_SESSION_CHANGED,
      `Session ${oldSession} cancelled by beginSession`)

    this.pendingEdits = []
    // Issue #1063：会话替换语义复位状态机（conflict 的另一条解锁路径是 retryReconcile）。
    this.sessionState.reset()
    this.reconcileScheduled = false
    this.nextEditSequence = 0
    this.appliedEditSequence = -1
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
    // Issue #1063 复审评论第 2 条：会话替换时清空 in-flight merge receipt（连同 pendingEdits
    // 已在上方复位），新会话不允许沿用旧会话的 Core 提交结果。
    this.inFlightMerge = null
    // Issue #1063 复审评论第 4 条：新会话的冲突材料从 idle 重新跟踪（旧材料仍归 Core 保留）。
    this.conflictMaterialState = { status: 'idle', key: '', receipt: null, reason: '' }
    this.nativeTextReady = false
    this.lastNativeTextReadFailed = false
    this.nativeEditing = false
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
    // Issue #1063 复审评论第 3 条：段落变化按统一规则在局部窗口判定，
    // 覆盖跨编辑边界形成/拆开 CRLF 与 U+2029；newlineDelta 直接维护换行计数。
    const change: LocalParagraphChange =
      describeLocalParagraphChange(previousText, insertedText, startUtf16, endUtf16)
    this.nativeNewlineCount = Math.max(0, this.nativeNewlineCount + change.newlineDelta)
    // 空文档第一次有字时需要重新发布段落/字体样式：空属性字符串没有可承载样式的位置。
    const becameNonEmpty: boolean = previousText.length === 0 && this.currentText.length > 0
    const paragraphsChanged: boolean = becameNonEmpty || change.paragraphsChanged

    if (pending !== null && pending.preview) {
      if (!this.imeComposing) {
        this.imeComposing = true
        this.imeComposingStartText = previousText
        this.imeComposingStartNewlineCount = this.nativeNewlineCount - change.newlineDelta
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
        change.newParagraphStarts, becameNonEmpty)
      // 预编辑态也可能有待收敛的状态机：给 checkIdle 一次事件机会（无 waiter 时立即返回）。
      this.checkIdle()
      return
    }

    if (this.imeComposing) {
      if (pending === null) {
        // 缺少同一笔 onWillChange 事实：不能证明 preview 已结束，绝不按镜像差量提交。
        // 只更新镜像并等下一次真实 commit/cancel 信号（退出屏障由各自 timer 兜底）。
        this.reportNativeFactError('onContentDidChange',
          'ime composition ended without will-change facts; refusing to commit mirror diff')
        this.notifyStateListener('native-change', paragraphsChanged,
          change.newParagraphStarts, becameNonEmpty)
        this.checkIdle()
        return
      }
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
        change.newParagraphStarts, becameNonEmpty)
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
      // enqueuePendingEdit 统一分配全局队列序号。
      queueSequence: -1,
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
      change.newParagraphStarts, becameNonEmpty)
  }

  // previewText 读取：null 表示读取失败/接口不可用（unknown），绝不能当成 0。
  // controller 未绑定代表没有原生编辑器可读取，此时没有预上屏，返回 0 是已知事实。
  private readPreviewTextLength(): number | null {
    if (this.richEditorController === null) {
      return 0
    }
    try {
      const previewText = this.richEditorController.getPreviewText()
      if (previewText === undefined || previewText === null) {
        return null
      }
      return previewText.value.length
    } catch (e) {
      hilog.warn(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, `readPreviewTextLength error: ${JSON.stringify(e)}`)
      return null
    }
  }

  private readPreviewTextState(): PreviewTextState {
    const length = this.readPreviewTextLength()
    if (length === null) {
      return 'unknown'
    }
    return length > 0 ? 'present' : 'empty'
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
      // 预编辑取消/空提交：正文未变，不产生 Core 事务，只清除退出屏障的信号等待。
      this.notifyImeSettled('cancel', -1)
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
      // enqueuePendingEdit 统一分配全局队列序号。
      queueSequence: -1,
      baseRevision: this.lastAppliedRevision,
      beforeText: baselineText,
      startUtf16: diff.start,
      endUtf16: diff.end,
      insertedText: diff.replacement,
      oldText: oldText,
      cause: EditorTransactionCause.TypingCommit
    }
    this.enqueuePendingEdit(pendingEdit)
    // 真实 commit 回调：把退出屏障绑定到这笔提交的队列序号；真正放行仍要等
    // Core 应用完成且 native/mirror/core 同 revision 一致。
    this.notifyImeSettled('commit', pendingEdit.queueSequence)
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
      if (this.sessionState.getHealth() !== 'healthy') {
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
    // 队列序号在这里统一分配：调用方只准备编辑事实，屏障序号与入队顺序严格一致。
    pendingEdit.queueSequence = this.nextEditSequence
    this.nextEditSequence++
    this.pendingEdits.push(pendingEdit)
    if (this.sessionState.getHealth() === 'healthy') {
      this.wakeDrain()
      return
    }
    // reconciling：冻结消费，保留全部未确认事件，并推进 reconcile；conflict 保留原状态。
    if (this.sessionState.getHealth() === 'reconciling') {
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
      if (this.sessionState.getHealth() !== 'healthy') {
        return
      }

      const edit = this.pendingEdits[0]
      if (edit.sessionId !== this.sessionId) {
        this.pendingEdits.shift()
        continue
      }

      const snap = this.coordinator.getSnapshot()
      const coreSessionId: number | null = this.coordinator.getSessionId()
      if (snap === null || coreSessionId === null) {
        this.enterConflict('no active core session during pending edit drain')
        return
      }

      const baselineText = snap.text
      if (edit.insertedText.length === 0 && edit.oldText.length === 0) {
        // 纯选择/无正文变化事件不进入编辑事务；直接消费，避免条件入口返回参数错误。
        this.pendingEdits.shift()
        if (edit.queueSequence > this.appliedEditSequence) {
          this.appliedEditSequence = edit.queueSequence
        }
        continue
      }
      if (baselineText !== edit.beforeText) {
        // 排队期间 Core 基线被推进：不在增量路径上猜编辑范围，交给统一三方合并处理。
        // 消费游标冻结（edit 保留在队首），reconcile 会把全部待提交编辑与 Core 侧改动合并。
        this.enterReconciling(
          `core baseline advanced before pending edit at offset ${edit.startUtf16}`)
        return
      }
      const mappedStart: number = edit.startUtf16
      const mappedEnd: number = edit.endUtf16

      const byteStart = TextOffsetMapper.utf16ToUtf8Checked(baselineText, mappedStart)
      const byteEnd = TextOffsetMapper.utf16ToUtf8Checked(baselineText, mappedEnd)
      if (byteStart === null || byteEnd === null) {
        this.enterReconciling(`invalid edit offset: start=${mappedStart}, end=${mappedEnd},`
          + ` textLen=${baselineText.length}`)
        return
      }

      // Issue #1063 复审评论 #7195630106：正常输入与重基共用同一个条件事务入口。
      // 出队时再核对 session/revision/整篇正文/oldText；STALE 时不能消费事件。
      const result: ResultEnvelope<EditorEditResult> =
        await this.coordinator.conditionalEdit(
          coreSessionId, snap.revision, snap.text,
          byteStart, byteEnd, edit.insertedText, edit.oldText, edit.cause)

      if (edit.sessionId !== this.sessionId) {
        return
      }

      if (!result.success && result.errorCode === 'STALE_BASELINE') {
        // 基线已前进但该编辑尚未应用：保留队首事件，交给统一重基重新映射。
        this.enterReconciling(
          `core baseline advanced during pending edit at offset ${edit.startUtf16}`)
        return
      }
      if (!result.success && result.errorCode === 'SESSION_CHANGED') {
        // 不把旧坐标提交到新会话；本地会话已替换时下一轮会丢弃该旧事件。
        if (this.sessionId === edit.sessionId) {
          this.enterConflict('core session changed during pending edit')
        }
        return
      }
      if (!result.success || result.data === undefined ||
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
        // 正常输入与重基共用同一已应用事务结算（队列确认 + 合法清空意图）。
        this.settleAppliedTransaction(coreSessionId, edit.queueSequence,
          baselineText, newSnap.text, mappedStart, mappedEnd, edit.insertedText,
          newSnap.revision, edit.cause)
        if (result.data.outcome === EditorEditOutcome.AppliedWithAdjustedSelection &&
            this.richEditorController !== null) {
          const utf16Cursor = TextOffsetMapper.utf8ToUtf16(newSnap.text, newSnap.cursor)
          const utf16Anchor = TextOffsetMapper.utf8ToUtf16(newSnap.text, newSnap.selectionAnchor)
          this.richEditorController.setSelection(utf16Anchor, utf16Cursor)
        }
      } else if (edit.queueSequence > this.appliedEditSequence) {
        // 事务已 Applied 但快照暂不可读：队列确认仍要前进，避免退出屏障永久等待。
        this.appliedEditSequence = edit.queueSequence
      }

      if (edit.sessionId === this.sessionId) {
        // Issue #1063 复审评论第 3 条：结算后的段落通知与原生通知使用同一套局部规则
        // （baselineText 是这笔事务提交前的 Core 正文）。
        const settledChange: LocalParagraphChange = describeLocalParagraphChange(
          baselineText, edit.insertedText, edit.startUtf16, edit.startUtf16 + edit.oldText.length)
        this.notifyStateListener('edit-settled', settledChange.paragraphsChanged,
          settledChange.newParagraphStarts)
      }
    }
  }

  // 正常输入与重基共用的“已应用事务”结算入口：
  // - 推进本地队列确认（退出屏障据此判断 commit 已进入 Core）；
  // - 仅在真实事务把非空整章删成空、且删除范围覆盖全部旧正文时记录合法清空意图；
  // - 非空提交清除旧意图；程序性初始空视图/恢复占位不调用这里。
  private settleAppliedTransaction(coreSessionId: number, queueSequence: number,
    beforeText: string, afterText: string, startUtf16: number, endUtf16: number,
    insertedText: string, revision: number, cause: EditorTransactionCause): void {
    if (queueSequence > this.appliedEditSequence) {
      this.appliedEditSequence = queueSequence
    }
    if (afterText.length > 0) {
      // 只有真实改动了正文的非空提交才清除旧意图；NoChange 不伪造一次提交。
      if (afterText !== beforeText) {
        this.emptyDocumentIntent = null
      }
      return
    }
    if (beforeText.length === 0 || insertedText.length > 0) {
      return
    }
    // 程序性恢复/占位不是用户清空事务，不能生成合法清空意图。
    if (cause === EditorTransactionCause.Programmatic) {
      return
    }
    if (startUtf16 === 0 && endUtf16 >= beforeText.length) {
      this.emptyDocumentIntent = { sessionId: coreSessionId, revision: revision }
    }
  }

  private computeDiff(oldText: string, newText: string): TextDiff {
    return computeMinimalTextDiff(oldText, newText)
  }

  // ── 原生编辑事实与运行时诊断（不含正文原文） ─────────────────────

  // 从属性字符串读取全文；失败返回 null（controller 未绑定组件时为 undefined）。
  // 已绑定且已加载正文时，读取失败记为 read-failed：屏障不能把它当成三方一致。
  private readNativeFullText(): string | null {
    if (this.richEditorController === null) {
      return null
    }
    try {
      const styled: MutableStyledString = this.richEditorController.getStyledString()
      if (styled === undefined || styled === null) {
        if (this.nativeTextReady) {
          this.lastNativeTextReadFailed = true
        }
        return null
      }
      const text: string = styled.getString()
      // 属性字符串模式没有 Span 列表；Span 数不再作为可读事实。
      this.lastReadNativeSpanCount = -1
      this.lastNativeTextReadFailed = false
      return text
    } catch (e) {
      hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, `readNativeFullText error: ${JSON.stringify(e)}`)
      if (this.nativeTextReady) {
        this.lastNativeTextReadFailed = true
      }
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

  // 程序回放：从 Core snapshot 同步正文到 RichEditor。
  // Issue #1063：reconciling/conflict 或还有 pendingEdits/IME 预编辑时不覆盖 RichEditor，
  // 保留用户可见正文（与状态机一致，不再读已删除的 idleError）。
  // Issue #1063 复审评论：外部 Core 刷新不得覆盖尚未进入镜像的原生正文。
  // - 写原生之前先读原生实际全文：读不到按对齐失败收尾；与镜像不一致只做事实修正并
  //   交给三方合并 reconcile，绝不调用 applyProgrammaticText 覆盖用户的未确认输入；
  // - 真正写入前后重新核对会话/revision/原生全文，写入后读回必须逐字等于 Core 正文。
  async syncFromCore(): Promise<void> {
    const sessionAtEntry = this.sessionId
    await this.editQueue

    if (this.sessionId !== sessionAtEntry) {
      return
    }

    if (this.sessionState.getHealth() !== 'healthy') {
      hilog.warn(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG,
        `syncFromCore: deferred because session is ${this.sessionState.getHealth()}`)
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

    // Issue #1063 复审评论：写原生之前以原生实际全文为准核对镜像。
    const nativeText: string | null = this.readNativeFullText()
    if (nativeText === null) {
      this.failNativeCoreAlignment(
        `syncFromCore cannot read native full text mirrorLen=${this.currentText.length}`
          + ` coreLen=${snap.text.length}`)
      return
    }
    if (nativeText !== this.currentText) {
      // 镜像落后或有未确认的原生差量：只按事实修正镜像，绝不在这里用 Core 正文覆盖；
      // 是否能把两侧改动合并交给统一三方合并（可能需要真实冲突持久化）。
      this.reportNativeFactError('syncFromCore',
        `unmirrored native delta firstMismatch=${firstMismatchUtf16(this.currentText, nativeText)}`)
      this.enterReconciling('syncFromCore: native text has unmirrored delta')
      this.scheduleReconcile()
      return
    }

    if (this.currentText === snap.text) {
      // 原生实际全文已在上方核对等于镜像，这里只推进版本与诊断事实。
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
    let factsStable: boolean = false
    let writeAccepted: boolean = false

    this.programmaticUpdate = true
    try {
      // 真正写原生之前重新核对事实：会话/revision/Core 正文/原生全文都必须与上面一致。
      const checkSessionId = this.coordinator.getSessionId()
      const checkSnap = this.coordinator.getSnapshot()
      const checkNativeText: string | null = this.readNativeFullText()
      factsStable = this.sessionId === sessionAtEntry
        && checkSessionId !== null
        && checkSnap !== null
        && checkSnap.revision === snap.revision
        && checkSnap.text === snap.text
        && checkNativeText === nativeText
      if (factsStable) {
        const diff = this.computeDiff(previousText, snap.text)
        syncStart = diff.start
        syncEnd = diff.end
        syncInsertedLength = diff.replacement.length
        const applied: boolean = this.applyProgrammaticText(snap.text)
        // 写入必须读回核对：读不到或读回不一致都不算写入成功。
        const acceptedText: string | null = applied ? this.readNativeFullText() : null
        writeAccepted = applied && acceptedText !== null && acceptedText === snap.text
        if (writeAccepted) {
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
        }
      }
    } catch (e) {
      hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, `syncFromCore error: ${JSON.stringify(e)}`)
    } finally {
      this.programmaticUpdate = false
    }

    if (!factsStable) {
      // 等待期间会话/基线/原生正文变化：本次不写、不推进任何基线，交回下一次同步或状态机。
      hilog.warn(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG,
        'syncFromCore: facts changed before programmatic write, skipped')
      return
    }
    if (!writeAccepted) {
      this.failNativeCoreAlignment(
        `syncFromCore: native editor did not accept core text mirrorLen=${this.currentText.length}`
          + ` coreLen=${snap.text.length}`)
      return
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
