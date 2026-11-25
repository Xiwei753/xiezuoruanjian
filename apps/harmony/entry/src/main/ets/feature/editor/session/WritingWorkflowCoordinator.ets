// WritingWorkflowCoordinator — 写作页真正的编辑流程协调器（Issue #920 A）。
//
// 为什么需要它：
// WritingScreen 之前把「加载章节 → 打开编辑会话 → 保存 → 幂等关闭 → 离开 → 切章节」的
// 先后顺序和异步事务都写在页面里，页面同时承担 UI 渲染和业务流程，边界混在一起。
// 本类把这些流程的**顺序与异步事务**收进来，页面只把按钮 / 系统返回 / 生命周期事件
// 交给它，并把本类产出的显示状态映射到 @State。
//
// 边界：
// - 正文真值仍在 EditorSessionCoordinator（Core snapshot），本类**不重造编辑内核**，
//   也不持有第二套可编辑正文状态；`content` 只是回流给 UI 的显示投影。
// - 所有编辑命令入口仍是唯一的 EditorSessionCoordinator。
// - 平台文案（资源字符串）、接续 / 分享 payload、滚动 viewport 等
//   仍由页面处理；本类通过 WritingTextKey 文案键与会话打开回调与其协作。
//
// Issue #629 评论6/7/8/9/11/14 的各项 close / save / leave 语义原样迁移到这里。

import hilog from '@ohos.hilog'
import { util } from '@kit.ArkTS'
import { IWriterCoreBridge } from '../../../corebridge/IWriterCoreBridge'
import { EditorSessionSnapshot, EditorEditResult, EditorTransactionCause, EditorOperationKind } from '../../../corebridge/dto/EditorDtos'
import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'
import { ChapterVersion } from '../../../corebridge/dto/ProjectDtos'
import { EditorSessionCoordinator, EditorStateUpdate } from './EditorSessionCoordinator'
import { EditorStatsQueue } from '../stats/EditorStatsQueue'
import { WritingSessionTracker } from '../stats/WritingSessionTracker'
import { NavigationTransactionCoordinator } from '../../../app/navigation/NavigationTransactionCoordinator'
import { TextOffsetMapper } from '../input/TextOffsetMapper'
import { Routes, WritingRouteParams } from '../../../app/navigation/Routes'
import { ActiveWritingSession, SyncBarrierBeginResult } from './ActiveWritingSessionRegistry'

// 协调器需要页面展示的文案键（翻译成资源字符串由页面负责，协调器不 import UI 上下文）。
export enum WritingTextKey {
  Saved = 'Saved',
  AutoSaved = 'AutoSaved',
  SaveFailed = 'SaveFailed',
  SaveError = 'SaveError',
  LoadChapterFailed = 'LoadChapterFailed',
  LoadChapterDetailFailed = 'LoadChapterDetailFailed',
  ClearFailed = 'ClearFailed',
  Cleared = 'Cleared',
  LeaveRejected = 'LeaveRejected'
}

export type WritingTextProvider = (key: WritingTextKey) => string

// 协调器产出的显示状态。页面把这些字段原样映射到 @State（自定义 interface 避免对象字面量类型）。
export interface WritingWorkflowState {
  readonly chapterId: string
  readonly content: string
  readonly wordCount: number
  readonly hasUnsavedChanges: boolean
  readonly lastSavedContent: string
  readonly lastSavedContentHash: string | null
  readonly isLoading: boolean
  readonly errorMessage: string
  readonly isSaving: boolean
  readonly isAutoSaving: boolean
  readonly lastSaveFailed: boolean
  readonly saveMessage: string
}

// 一次回流：显示状态 + （编辑会话驱动的）快照 / 编辑结果。
// snapshot 只在编辑会话状态变化（open / 编辑命令 / 恢复）时非空，页面据此重建排版快照。
export interface WritingWorkflowUpdate {
  readonly state: WritingWorkflowState
  readonly snapshot: EditorSessionSnapshot | null
  readonly editResult: EditorEditResult | null
  // Issue #981：本条编辑事务固定下来的统计事件身份与发生时刻。
  // 页面据此用同一时刻启动真实写作计时，避免异步写盘补时刻把首笔字数漏掉。
  readonly statsEventId: string
  readonly statsOccurredAtMs: number
}

export type WritingWorkflowListener = (update: WritingWorkflowUpdate) => void

// 页面在 aboutToAppear 里把路由上下文交给协调器（正文 id / 作品 id / 卷 id / 导航栈）。
export interface WritingWorkflowConfig {
  chapterId: string
  projectId: string
  volumeId: string
  navPathStack: NavPathStack
}

// Issue #629 评论 5308748920 问题3：保存事务记录（命名 interface，避免对象字面量类型）。
// Issue #1054 复审：不可变保存请求。授权在入队前按这份快照的 session/revision
// 精确计算，doSaveChapter 不再读取保存期间更新的最新快照来判断旧请求。
interface SaveRequest {
  chapterId: string
  sessionId: number
  revision: number
  text: string
  allowEmptyOverwrite: boolean
  isAutoSave: boolean
  // 本次批次固定的持久化基线；等待前一笔保存后刷新为最新 durable version。
  baseVersion: ChapterVersion | null
  baseContent: string
}

// 调用方已结算的快照身份。缺省时只用于非空保存；空保存必须带精确来源。
interface SaveSnapshotSource {
  sessionId: number
  revision: number
}

interface ActiveSaveTask {
  request: SaveRequest
  task: Promise<boolean>
}

export class WritingWorkflowCoordinator implements ActiveWritingSession {
  private bridge: IWriterCoreBridge
  private editorCoordinator: EditorSessionCoordinator
  private statsQueue: EditorStatsQueue
  private writingSessionTracker: WritingSessionTracker
  private navigation: NavigationTransactionCoordinator
  private textProvider: WritingTextProvider

  // 页面监听器（显示状态回流）。
  private listener: WritingWorkflowListener | null = null
  // 页面在会话打开后恢复平台侧编辑器状态（接续选区 / viewport）的回调。
  private sessionOpenedHook: ((snap: EditorSessionSnapshot) => Promise<void>) | null = null
  // Issue #1034 评论 #7：RichEditor 桥接的 whenIdle，保存/同步/关闭前先 await 桥接队列空闲，
  // 再 await coordinator.whenIdle，确保用户最后几字已进入 Core。
  // Issue #1034 评论 #13：拆为两个入口：
  // - bridgeWhenIdle：普通保存，只等已提交队列事务，不调用 stopEditing()
  // - bridgeFinishEditingAndWhenIdle：退出/同步，先 stopEditing() 结束预编辑再等队列稳定
  private bridgeWhenIdle: (() => Promise<void>) | null = null
  private bridgeFinishEditingAndWhenIdle: (() => Promise<void>) | null = null
  // Issue #1054：合法空文档意图由原生编辑事务记录（精确 sessionId/revision）。
  // 保存路径只凭这个事实决定是否允许空覆盖，不用 UI 的初始 content='' 推断。
  private emptyDocumentIntentCheck: ((sessionId: number, revision: number) => boolean) | null = null

  // 路由上下文：chapterId/projectId/volumeId/navPathStack。
  // chapterId 公开：同步事务（ActiveWritingSession）需要按当前章节复核 Core 落盘内容。
  chapterId: string = ''
  private projectId: string = ''
  private volumeId: string = ''
  private navPathStack: NavPathStack = new NavPathStack()

  // ── 显示状态（页面映射到 @State；正文投影自 Core snapshot，不是第二套编辑状态）──
  private content: string = ''
  private wordCount: number = 0
  private hasUnsavedChanges: boolean = false
  private lastSavedContent: string = ''
  // 证明链起点只上报一次：本次打开章节时的完整正文；之后由 Core 链式 hash 承接。
  private proofBaselineSent: boolean = false
  private lastSavedContentHash: string | null = null
  private lastSavedVersion: ChapterVersion | null = null
  private isLoading: boolean = false
  private errorMessage: string = ''
  private isSaving: boolean = false
  private isAutoSaving: boolean = false
  private lastSaveFailed: boolean = false
  private saveMessage: string = ''

  // ── 异步事务 ──
  // Issue #629 评论7 第5项：幂等关闭事务 Promise；失败后重置允许重试。
  private closeTask: Promise<boolean> | null = null
  // Issue #629 评论 5308748920 问题2：离开事务（close + pop）整体幂等。
  private leaveTask: Promise<boolean> | null = null
  // Issue #629 评论 5308748920 问题3：保存事务复用 + 精确目标文本。
  private activeSave: ActiveSaveTask | null = null
  // Issue #923：同步屏障状态（与 endSyncBarrier 成对）。
  // held=本次同步持有本会话屏障；sealed=传输期间封锁输入（防止往将被替换的 live 章节写旧内存内容）。
  private syncBarrierHeld: boolean = false

  constructor(
    bridge: IWriterCoreBridge,
    editorCoordinator: EditorSessionCoordinator,
    statsQueue: EditorStatsQueue,
    writingSessionTracker: WritingSessionTracker,
    navigation: NavigationTransactionCoordinator,
    textProvider: WritingTextProvider
  ) {
    this.bridge = bridge
    this.editorCoordinator = editorCoordinator
    this.statsQueue = statsQueue
    this.writingSessionTracker = writingSessionTracker
    this.navigation = navigation
    this.textProvider = textProvider
    // Issue #629 评论9 第4项：listener 改 typed EditorStateUpdate；本类消费后回流显示状态。
    this.editorCoordinator.setStateListener((update: EditorStateUpdate): void => {
      this.onEditorStateUpdate(update)
    })
  }

  configure(config: WritingWorkflowConfig): void {
    this.chapterId = config.chapterId
    this.projectId = config.projectId
    this.volumeId = config.volumeId
    this.navPathStack = config.navPathStack
  }

  setListener(listener: WritingWorkflowListener | null): void {
    this.listener = listener
  }

  // Issue #1056：Core 编辑会话的唯一 target 身份。
  // `correction_session_identity()` 要求拆分后是 4 段 `chapter-body:projectId:volumeId:chapterId`，
  // 与 Android 端保持同一协议；裸 chapterId 会令校对请求被 Core 拒绝。
  // 只有编辑 session target 使用复合标识，业务 loadChapter/saveChapter 仍按真正章节 UUID 工作。
  // 公开给页面核对 Core snapshot.chapter_id（Core 原样回传 target_id）。
  chapterSessionTargetId(): string {
    return `chapter-body:${this.projectId}:${this.volumeId}:${this.chapterId}`
  }

  // 会话 open 成功后，页面恢复接续选区 / viewport（平台侧状态）用。
  setSessionOpenedHook(hook: ((snap: EditorSessionSnapshot) => Promise<void>) | null): void {
    this.sessionOpenedHook = hook
  }

  setBridgeWhenIdle(fn: (() => Promise<void>) | null): void {
    this.bridgeWhenIdle = fn
  }

  setBridgeFinishEditingAndWhenIdle(fn: (() => Promise<void>) | null): void {
    this.bridgeFinishEditingAndWhenIdle = fn
  }

  // Issue #1054：由页面把原生桥接的“合法清空意图”校验注入保存链路。
  setEmptyDocumentIntentCheck(fn: ((sessionId: number, revision: number) => boolean) | null): void {
    this.emptyDocumentIntentCheck = fn
  }

  // 目标正文为空时，授权只认入队时冻结的 session/revision：
  // - clearDocument 等显式清空指令直接 allowEmptyOverwrite=true；
  // - 普通输入法删除到空必须匹配原生编辑事务记录的精确清空意图。
  // 保存期间用户又输入了新字，不会改写已入队旧请求的授权。
  private resolveEmptyOverwriteAuthorization(targetText: string, allowEmptyOverwrite: boolean,
    sessionId: number, revision: number): boolean {
    if (targetText.length > 0) {
      return false
    }
    if (allowEmptyOverwrite) {
      return true
    }
    if (sessionId < 0 || revision < 0 || this.emptyDocumentIntentCheck === null) {
      return false
    }
    return this.emptyDocumentIntentCheck(sessionId, revision)
  }

  private sameSaveRequest(a: SaveRequest, b: SaveRequest): boolean {
    return a.chapterId === b.chapterId && a.sessionId === b.sessionId &&
      a.revision === b.revision && a.text === b.text &&
      a.allowEmptyOverwrite === b.allowEmptyOverwrite &&
      a.baseContent === b.baseContent
  }

  // 从调用方已结算的 Core 快照冻结保存身份；session 必须仍属于同一编辑会话。
  private snapshotSaveSource(snap: EditorSessionSnapshot | null): SaveSnapshotSource | null {
    const sessionId = this.editorCoordinator.getSessionId()
    if (snap === null || sessionId === null) {
      return null
    }
    return { sessionId: sessionId, revision: snap.revision }
  }

  // Issue #1053 评论 7098652618：屏障结果决定保存/同步/关闭能否继续。
  // 原生正文、本地镜像与 Core 正文未对齐时 bridge 会 reject，这里必须返回 false，
  // 不能把「回调已调用」当成「正文已结算」继续落盘旧 Core 正文。
  private async awaitBridgeIdle(): Promise<boolean> {
    const bridgeWhenIdle = this.bridgeWhenIdle
    if (bridgeWhenIdle === null) {
      return true
    }
    try {
      await bridgeWhenIdle()
      return true
    } catch (err) {
      this.reportBridgeAlignmentFailure('idle', err)
      return false
    }
  }

  private async awaitBridgeFinishEditing(): Promise<boolean> {
    const bridgeFinishEditingAndWhenIdle = this.bridgeFinishEditingAndWhenIdle
    if (bridgeFinishEditingAndWhenIdle === null) {
      return true
    }
    try {
      await bridgeFinishEditingAndWhenIdle()
      return true
    } catch (err) {
      this.reportBridgeAlignmentFailure('finishEditing', err)
      return false
    }
  }

  private reportBridgeAlignmentFailure(phase: string, err: Object): void {
    const detail = err instanceof Error ? err.message : String(err)
    hilog.error(0x0000, 'WritingWorkflowCoordinator',
      `editor barrier failed phase=${phase}: ${detail}`)
  }

  // 编辑屏障失败：保留原生编辑状态，返回保存失败（不落盘旧 Core 正文）。
  private failSaveAfterBarrierFailure(): boolean {
    this.lastSaveFailed = true
    this.saveMessage = this.textProvider(WritingTextKey.SaveFailed)
    this.emit(null, null)
    return false
  }

  // 页面离开时调用：停止编辑会话状态回流，避免 close 后仍有回调。
  dispose(): void {
    this.editorCoordinator.setStateListener(null)
    this.listener = null
  }

  // ── 显示状态回流 ──

  private emit(snapshot: EditorSessionSnapshot | null, editResult: EditorEditResult | null,
    statsEventId: string = '', statsOccurredAtMs: number = 0): void {
    const listener = this.listener
    if (listener === null) {
      return
    }
    const state: WritingWorkflowState = {
      chapterId: this.chapterId,
      content: this.content,
      wordCount: this.wordCount,
      hasUnsavedChanges: this.hasUnsavedChanges,
      lastSavedContent: this.lastSavedContent,
      lastSavedContentHash: this.lastSavedContentHash,
      isLoading: this.isLoading,
      errorMessage: this.errorMessage,
      isSaving: this.isSaving,
      isAutoSaving: this.isAutoSaving,
      lastSaveFailed: this.lastSaveFailed,
      saveMessage: this.saveMessage
    }
    const update: WritingWorkflowUpdate = {
      state: state,
      snapshot: snapshot,
      editResult: editResult,
      statsEventId: statsEventId,
      statsOccurredAtMs: statsOccurredAtMs
    }
    listener(update)
  }

  // 编辑会话状态回流：更新正文投影 / 未保存标记 / 实时字数，并把编辑事务入统计队列。
  // Issue #629 评论9 第4项：wordCount 来自 editResult.contentDelta（Core 非空白字符增量），
  // 不用 snap.text.length（JS UTF-16 code unit 数）。
  private onEditorStateUpdate(update: EditorStateUpdate): void {
    const snap = update.snapshot
    this.content = snap.text
    this.hasUnsavedChanges = snap.text !== this.lastSavedContent
    if (update.editResult !== null) {
      // Issue #1034 评论 #18：CursorOnly（选区上报）跳过 stats UUID/enqueueChange 等仅供正文内容变更的路径
      if (update.editResult.operationKind === EditorOperationKind.CursorOnly) {
        this.emit(snap, update.editResult)
        return
      }
      const delta = update.editResult.contentDelta
      this.wordCount = Math.max(0, this.wordCount + delta.insertedNonWhitespaceChars - delta.deletedNonWhitespaceChars)
      // Issue #981：统计事件身份和发生时刻在编辑事务当场固定，异步队列不再补时刻；
      // 页面用同一时刻锚定真实写作计时，首笔输入不会落在片段起点之前。
      const statsOccurredAtMs: number = Date.now()
      const statsEventId: string = util.generateRandomUUID(false)
      const proofBaseline: string = this.proofBaselineSent ? '' : this.lastSavedContent
      // Issue #981 / #1008：真实会话在编辑发生时先建立，再让同一事务事件携带 sessionId，
      // 保证证明事件与前台计时切片指向同一会话。
      if ((update.editResult.cause === EditorTransactionCause.Typing
        || update.editResult.cause === EditorTransactionCause.TypingCommit
        || update.editResult.cause === EditorTransactionCause.ImeComposition
        || update.editResult.cause === EditorTransactionCause.Delete)
        && (delta.insertedChars > 0 || delta.deletedChars > 0)) {
        this.writingSessionTracker.onHumanEdit(
          this.projectId, this.volumeId, this.chapterId, statsOccurredAtMs)
      }
      // Issue #829 评论 #5995146525：正文每次真实变化就在这里产出统计事件，
      // 不再等保存后比较整章。enqueueChange 只入内存队列，不跨 NAPI、不 await。
      this.statsQueue.enqueueChange(
        this.projectId, this.volumeId, this.chapterId,
        update.editResult.cause, delta.insertedChars, delta.deletedChars,
        statsEventId, statsOccurredAtMs,
        update.editResult.changes ?? [],
        this.writingSessionTracker.currentSessionId(),
        update.editResult.baseRevision,
        update.editResult.newRevision,
        update.editResult.oldSelectionAnchor,
        update.editResult.oldSelectionHead,
        update.editResult.newSelectionAnchor,
        update.editResult.newSelectionHead,
        proofBaseline,
        update.editResult.beforeTextSha256 ?? '',
        update.editResult.afterTextSha256 ?? ''
      )
      if (delta.insertedChars > 0 || delta.deletedChars > 0) {
        this.proofBaselineSent = true
      }
      this.emit(snap, update.editResult, statsEventId, statsOccurredAtMs)
      return
    }
    this.emit(snap, update.editResult)
  }

  // ── 加载 ──

  /** 手写证明回放：原样取 Core 计算结果，页面只渲染。 */
  async replayChapterProof(requestJson: string): Promise<ResultEnvelope<Object>> {
    return this.bridge.replayChapterProof(requestJson)
  }

  /** 导出证明包（包含历史正文，展示前需明确提示）。 */
  async exportChapterProof(chapterId: string, deviceId: string): Promise<ResultEnvelope<Object>> {
    return this.bridge.exportChapterProof(chapterId, deviceId)
  }

  // Issue #768 评论5838296806：先加载排版设置，再打开章节会话（由页面在外面先 loadSettings 再调本方法）。
  async loadChapter(): Promise<void> {
    this.isLoading = true
    this.errorMessage = ''
    this.emit(null, null)

    try {
      const result = await this.bridge.loadChapter(this.chapterId)
      if (result.success && result.data) {
        this.content = result.data.content
        this.lastSavedContent = result.data.content
        this.proofBaselineSent = false
        // Issue #1059：内容身份来自 open 返回的实际正文版本摘要，而不是可能
        // 滞后的 meta.hash；清空/删除等场景也不会把旧 hash 当成已保存。
        this.lastSavedContentHash = result.data.version.contentSha256
        this.lastSavedVersion = result.data.version
        this.wordCount = result.data.meta.wordCount
        this.hasUnsavedChanges = false
        // 打开编辑会话，用 Core 管理正文。open 成功后 listener 自动回流显示状态。
        // Issue #653 评论 5608056955：close 在离开时统一做，open 不接收 isPersistent。
        // Issue #1056：编辑会话 target 使用与 Core/Android 统一的复合章节身份。
        const snapResult = await this.editorCoordinator.open(this.chapterSessionTargetId(), result.data.content)
        if (!snapResult.success) {
          this.errorMessage = this.textProvider(WritingTextKey.LoadChapterFailed)
        } else if (snapResult.data) {
          // 页面恢复接续选区 / viewport（在 coordinator.open 成功后调用）。
          const hook = this.sessionOpenedHook
          if (hook !== null) {
            await hook(snapResult.data)
          }
        }
      } else {
        this.errorMessage = this.textProvider(WritingTextKey.LoadChapterFailed)
      }
    } catch (err) {
      this.errorMessage = this.textProvider(WritingTextKey.LoadChapterDetailFailed)
    } finally {
      this.isLoading = false
      this.emit(null, null)
    }
  }

  // ── 保存 ──

  // Issue #629 评论 5308748920 问题3：手动/自动保存入口。
  // 先取得稳定 snapshot（flush + whenIdle），再调 ensureSnapshotSaved 精确保存这份文本。
  async saveChapter(isAutoSave: boolean = false): Promise<boolean> {
    if (!await this.awaitBridgeIdle()) {
      return this.failSaveAfterBarrierFailure()
    }
    await this.editorCoordinator.whenIdle()
    const snap = this.editorCoordinator.getSnapshot()
    const targetText: string = snap ? snap.text : this.content
    return this.ensureSnapshotSaved(targetText, isAutoSave, false, this.snapshotSaveSource(snap))
  }

  // Issue #629 评论 5308748920 问题3：统一保存接口，保证指定 targetText 落盘。
  // Issue #1054 复审：入队时把章节/session/revision/文本/空覆盖授权冻结成不可变请求；
  // 等待旧保存期间即使正文继续前进，也不会重新解释旧请求的删除意图。
  async ensureSnapshotSaved(targetText: string, isAutoSave: boolean,
    allowEmptyOverwrite: boolean = false, source: SaveSnapshotSource | null = null): Promise<boolean> {
    const sessionId: number = source !== null ? source.sessionId
      : (this.editorCoordinator.getSessionId() ?? -1)
    const revision: number = source !== null ? source.revision
      : (this.editorCoordinator.getSnapshot()?.revision ?? -1)
    const request: SaveRequest = {
      chapterId: this.chapterId,
      sessionId: sessionId,
      revision: revision,
      text: targetText,
      allowEmptyOverwrite: this.resolveEmptyOverwriteAuthorization(
        targetText, allowEmptyOverwrite, sessionId, revision),
      isAutoSave: isAutoSave,
      baseVersion: this.lastSavedVersion,
      baseContent: this.lastSavedContent
    }
    // 若有进行中保存任务，先等它结束
    if (this.activeSave !== null) {
      const prev = this.activeSave
      let prevOk: boolean = false
      try {
        prevOk = await prev.task
      } catch (err) {
        // 旧任务异常，继续启动新事务
      }
      // 章节/会话/修订/文本/授权完全相同且成功 → 直接复用结果
      if (this.sameSaveRequest(prev.request, request) && prevOk) {
        return true
      }
      // 目标不同或旧任务失败：清 activeSave，启动新事务
      if (this.activeSave === prev) {
        this.activeSave = null
      }
    }
    // 前一笔保存已结束：本批基线刷新为最新 durable version，避免用自己的
    // 上一笔提交制造假冲突。
    request.baseVersion = this.lastSavedVersion
    request.baseContent = this.lastSavedContent
    // 启动一次精确保存新事务
    const task: Promise<boolean> = this.doSaveChapter(request)
    const saveEntry: ActiveSaveTask = { request: request, task: task }
    this.activeSave = saveEntry
    task.then(
      () => {
        if (this.activeSave !== null && this.activeSave.task === task) {
          this.activeSave = null
        }
      },
      () => {
        if (this.activeSave !== null && this.activeSave.task === task) {
          this.activeSave = null
        }
      }
    )
    return task
  }

  // Issue #629 评论 5308748920 问题3：真实持久化函数，只接受显式保存请求。
  // 不在里面重新抓"当前正文"，也不重新读取较新的快照判断旧请求的空覆盖授权。
  // 结算：lastSavedContent 按 request.text；再取 latestSnapshot 结算 hasUnsavedChanges / wordCount。
  private async doSaveChapter(request: SaveRequest): Promise<boolean> {
    this.isSaving = true
    this.isAutoSaving = request.isAutoSave
    this.saveMessage = ''
    // Issue #829 评论 #5996577737 第 2 项：在这份正文定型之前记下统计 barrier。
    const statsBarrier: number = this.statsQueue.currentSeq()
    this.emit(null, null)

    try {
      // 先让本批编辑证明进入 Core 内存队列，再提交正文；正文 journal 才会
      // 携带这批事件，不会出现“保存成功但最后一批证明等下一次保存”。
      const statsFlushed: boolean = await this.flushEditorStatsQueue(statsBarrier)
      if (!statsFlushed) {
        this.lastSaveFailed = true
        this.saveMessage = this.textProvider(WritingTextKey.SaveError)
        this.isSaving = false
        this.isAutoSaving = false
        this.emit(null, null)
        return false
      }
      if (request.baseVersion === null) {
        this.lastSaveFailed = true
        this.saveMessage = this.textProvider(WritingTextKey.SaveError)
        this.isSaving = false
        this.isAutoSaving = false
        this.emit(null, null)
        return false
      }
      // 空正文只有在入队时已确认的授权下才走显式 clear 入口。
      const result = request.allowEmptyOverwrite
        ? await this.bridge.clearChapterVersioned(
            request.chapterId, request.baseVersion, request.baseContent)
        : await this.bridge.saveChapterVersioned(
            request.chapterId, request.text, request.baseVersion, request.baseContent, false)
      if (result.success && result.data) {
        // Issue #829 评论 #5995146525：这里不再上报写作统计（改由编辑事务同源上报）。
        this.lastSavedContent = request.text
        // Issue #1059：保存成功后以 receipt.version.contentSha256 作为内容身份。
        this.lastSavedContentHash = result.data.version.contentSha256
        this.lastSavedVersion = result.data.version
        this.hasUnsavedChanges = false
        this.lastSaveFailed = false
        // 重新取 latestSnapshot 结算：保存期间若又产生新输入，hasUnsavedChanges 保持 true。
        // Issue #629 评论9 第4项：只有保存期间无新输入时才用 receipt.wordCount 校正实时字数。
        const latest = this.editorCoordinator.getSnapshot()
        if (latest) {
          this.content = latest.text
          this.hasUnsavedChanges = latest.text !== request.text
          if (latest.text === request.text) {
            this.wordCount = result.data.wordCount
          }
        } else {
          this.wordCount = result.data.wordCount
        }
        this.saveMessage = request.isAutoSave
          ? this.textProvider(WritingTextKey.AutoSaved)
          : this.textProvider(WritingTextKey.Saved)
        this.scheduleSaveMessageClear()
        // 保存成功后回流（contentHash 前进，此时 hasUnsavedChanges=false，页面据此更新接续状态）。
        this.emit(null, null)
        return true
      } else {
        this.lastSaveFailed = true
        // 空覆盖被 Core 安全机制拒绝时给出可区分的原因，不把合法清空和存储错误混在一起。
        this.saveMessage = result.errorCode === 'EMPTY_OVERWRITE_BLOCKED'
          ? '正文为空但未确认清空意图，本次未覆盖磁盘内容'
          : this.textProvider(WritingTextKey.SaveFailed)
        this.emit(null, null)
        return false
      }
    } catch (err) {
      this.lastSaveFailed = true
      this.saveMessage = this.textProvider(WritingTextKey.SaveError)
      this.emit(null, null)
      return false
    } finally {
      this.isSaving = false
      this.isAutoSaving = false
      this.emit(null, null)
    }
  }

  // Issue #629 评论6 Part G：退后台保存事务。
  // flush input → await coordinator idle → save latest snapshot；不 detach IME、不 close session。
  // Issue #927：返回真实成功/失败结果（只开一次保存事务），失败时保留 hasUnsavedChanges，
  // 调用方不得把「回调已调用」当成「已经落盘」。
  async performBackgroundSave(): Promise<boolean> {
    if (!await this.awaitBridgeIdle()) {
      return this.failSaveAfterBarrierFailure()
    }
    await this.editorCoordinator.whenIdle()
    // 无待保存变化：本次后台事务直接成功，不产生多余 Core 写入。
    if (!this.hasUnsavedChanges) {
      return true
    }
    const snap = this.editorCoordinator.getSnapshot()
    const targetText: string = snap ? snap.text : this.content
    return this.ensureSnapshotSaved(targetText, true, false, this.snapshotSaveSource(snap))
  }

  // ── 同步保存屏障（Issue #923）──

  // 同步事务（手动 / 自动 / 顶栏 / 设置页）的唯一入口是 ActiveWritingSessionRegistry.withSyncBarrier：
  // beginSyncBarrier 结算当前输入并落盘 → Core 全量同步 → reconcileAfterSync 按落盘身份回填本会话
  // → endSyncBarrier 释放。三个方法必须在同一笔同步事务内配对使用。

  // 同步前保存屏障：封输入 → finish composition → drain 输入 → 稳定 snapshot → 精确落盘。
  // 与 performGracefulClose 的区别：**不** close session、不 detach IME、不换章节。
  // 保存失败返回 ok:false 并恢复编辑输入；只有确实完成当前 snapshot 落盘才放行 Core 全量同步。
  async beginSyncBarrier(): Promise<SyncBarrierBeginResult> {
    // 无编辑会话（零章节工作台 / 尚未打开正文）：无正文可被同步替换，直接放行。
    if (this.editorCoordinator.getSessionId() === null) {
      return { ok: true }
    }
    this.syncBarrierHeld = true
    try {
      // 同一会话边界：先提交 IME composition，再等输入队列空闲，再取稳定 snapshot。
      const finishResult = await this.finishActiveComposition()
      const bridgeSettled: boolean = await this.awaitBridgeFinishEditing()
      if (!bridgeSettled) {
        // 原生正文与 Core 未对齐：不放行同步，避免把旧 Core 正文同步出去。
        this.endSyncBarrier()
        return { ok: false }
      }
      await this.editorCoordinator.whenIdle()
      const snap = this.editorCoordinator.getSnapshot()
      if (finishResult.success === false || snap === null ||
        (snap.composition !== null && snap.composition !== undefined)) {
        // composition 未结算 / snapshot 丢失：不放行同步（不静默用旧文本覆盖）。
        this.endSyncBarrier()
        return { ok: false }
      }
      const saved: boolean = await this.ensureSnapshotSaved(
        snap.text, false, false, this.snapshotSaveSource(snap))
      if (!saved) {
        // 保存失败：不进入同步，恢复编辑输入。
        this.endSyncBarrier()
        return { ok: false }
      }
      const hash: string | null = this.lastSavedContentHash
      if (hash === null) {
        return { ok: true }
      }
      return { ok: true, contentHash: hash }
    } catch (err) {
      this.lastSaveFailed = true
      this.saveMessage = this.textProvider(WritingTextKey.SaveError)
      this.emit(null, null)
      this.endSyncBarrier()
      return { ok: false }
    }
  }

  // 同步后复核当前章节的 Core 落盘身份：
  // - 未变：直接返回 true（endSyncBarrier 统一释放输入封锁）。
  // - 变化：在已无待提交编辑命令前提下，用同一现存 EditorSessionCoordinator 与新内容重新对齐
  //   （close → open 新内容，不新增第二份正文源），更新 lastSavedContent/hash、字数与排版快照。
  // - 发现未提交本地变化 / 无法确认落盘身份：返回 false（明确冲突），
  //   不把远端内容悄悄覆盖成旧文本，也不丢本地输入。
  async reconcileAfterSync(): Promise<boolean> {
    if (!this.syncBarrierHeld) {
      return true
    }
    const result = await this.bridge.loadChapter(this.chapterId)
    if (!result.success || !result.data) {
      // 无法确认 Core 落盘身份：不放行，交由上层按冲突处理。
      return false
    }
    const diskContent: string = result.data.content
    const diskHash: string = result.data.version.contentSha256
    if (this.lastSavedVersion !== null
        && diskHash === this.lastSavedVersion.contentSha256
        && result.data.version.revision === this.lastSavedVersion.revision
        && result.data.version.objectGeneration === this.lastSavedVersion.objectGeneration) {
      // Core 落盘完整版本与屏障放行时一致：本次同步未改动本章。
      return true
    }
    // 磁盘已变化：确认没有仍在排队的编辑命令，再决定能否回填。
    if (!await this.awaitBridgeIdle()) {
      // 原生编辑事实未能结算：不放行回填，交由上层按冲突处理。
      return false
    }
    await this.editorCoordinator.whenIdle()
    const snap = this.editorCoordinator.getSnapshot()
    if (snap === null) {
      // 会话已不存在（页面正在关闭）：只更新投影基线，正文以 Core 落盘为准。
      this.content = diskContent
      this.lastSavedContent = diskContent
      this.lastSavedContentHash = diskHash
      this.lastSavedVersion = result.data.version
      this.hasUnsavedChanges = false
      this.wordCount = result.data.meta.wordCount
      this.emit(null, null)
      return true
    }
    if (snap.text !== this.lastSavedContent) {
      // 屏障之后又出现了未提交的本地变化：明确冲突，由上层提示用户处理。
      return false
    }
    // 同一现存 EditorSessionCoordinator 与新内容重新对齐：close → open 下载后的正文。
    const closed = await this.editorCoordinator.closeAsync()
    if (!closed.success || closed.data !== true) {
      return false
    }
    const reopened = await this.editorCoordinator.open(this.chapterSessionTargetId(), diskContent)
    if (!reopened.success || !reopened.data) {
      return false
    }
    this.content = diskContent
    this.lastSavedContent = diskContent
    this.lastSavedContentHash = diskHash
    this.lastSavedVersion = result.data.version
    this.hasUnsavedChanges = false
    this.wordCount = result.data.meta.wordCount
    // 页面恢复接续选区 / viewport（与 loadChapter 同一 hook），再统一回流重绘。
    const hook = this.sessionOpenedHook
    if (hook !== null) {
      await hook(reopened.data)
    }
    this.emit(reopened.data, null)
    return true
  }

  // 释放同步屏障（同步成功 / 失败 / 冲突都必须调用；与 beginSyncBarrier 成对且幂等）。
  endSyncBarrier(): void {
    this.syncBarrierHeld = false
  }

  // ── 清空正文 ──

  // Issue #629 评论 5 第 4 节 + 评论9 第4项 + 评论11 第1项：
  // 清空正文：先等已有输入命令完成，再 replace 整个文本为空。
  async clearDocument(): Promise<void> {
    this.isSaving = true
    this.emit(null, null)
    const bridgeSettled: boolean = await this.awaitBridgeFinishEditing()
    if (!bridgeSettled) {
      this.isSaving = false
      this.lastSaveFailed = true
      this.saveMessage = this.textProvider(WritingTextKey.ClearFailed)
      this.emit(null, null)
      return
    }
    await this.editorCoordinator.whenIdle()
    const snap = this.editorCoordinator.getSnapshot()
    if (!snap) {
      this.isSaving = false
      this.lastSaveFailed = true
      this.saveMessage = this.textProvider(WritingTextKey.ClearFailed)
      this.emit(null, null)
      return
    }
    // 若有活跃 composition，先取消再清空。
    if (snap.composition !== null && snap.composition !== undefined) {
      const cancelResult = await this.editorCoordinator.cancelComposition(
        snap.composition.sessionId, snap.composition.generation)
      if (!cancelResult.success) {
        this.isSaving = false
        this.lastSaveFailed = true
        this.saveMessage = this.textProvider(WritingTextKey.ClearFailed)
        this.emit(null, null)
        return
      }
    }
    const fullText: string = snap.text
    const utf8EndByte: number = TextOffsetMapper.utf16ToUtf8(fullText, fullText.length)
    const result = await this.editorCoordinator.replace(
      0, utf8EndByte, '', fullText, EditorTransactionCause.Programmatic)
    this.isSaving = false
    this.emit(null, null)
    if (!result.success) {
      this.lastSaveFailed = true
      this.saveMessage = this.textProvider(WritingTextKey.ClearFailed)
      this.emit(null, null)
      return
    }
    // clear 之后用户立刻打的新字允许继续进入 Dispatcher；ensureSnapshotSaved('') 只保存空基线。
    // 显式“清空章节”是独立用户指令，允许空覆盖；普通输入法删除仍依赖编辑事务意图。
    const afterClear = this.editorCoordinator.getSnapshot()
    const saved: boolean = await this.ensureSnapshotSaved(
      '', false, true, this.snapshotSaveSource(afterClear))
    if (saved) {
      this.saveMessage = this.textProvider(WritingTextKey.Cleared)
      this.scheduleSaveMessageClear()
      this.emit(null, null)
    } else {
      this.lastSaveFailed = true
      this.saveMessage = this.textProvider(WritingTextKey.ClearFailed)
      this.emit(null, null)
    }
  }

  // ── 关闭 / 离开 ──

  // Issue #629 评论6 Part G + 评论7 第5项 + 评论8 第2/3项：统一幂等关闭事务。
  // finishActiveComposition → whenIdle → save → closeAsync。
  // 幂等：closeTask 复用同一 Promise；失败后重置允许重试。
  async performGracefulClose(): Promise<boolean> {
    // 无编辑会话（零章节工作台）：没有 session 需要 close/save，直接允许离开。
    if (this.chapterId.length === 0 && this.editorCoordinator.getSessionId() === null) {
      return true
    }
    if (this.closeTask !== null) {
      return this.closeTask
    }
    const task: Promise<boolean> = this.doGracefulClose()
    this.closeTask = task
    task.then(
      (closed: boolean) => {
        // 成功关闭：页面即将销毁，保留缓存（不重跑）。失败：重置，允许下次返回重试。
        if (!closed && this.closeTask === task) {
          this.closeTask = null
        }
      },
      () => {
        if (this.closeTask === task) {
          this.closeTask = null
        }
      }
    )
    return task
  }

  // Issue #629 评论7 第5项：真实关闭逻辑（拆出，由 performGracefulClose 幂等包装）。
  // 顺序：finishActiveComposition → whenIdle → save → closeAsync。
  private async doGracefulClose(): Promise<boolean> {
    try {
      // 1. finish 当前 composition：最后的中文 preedit 提交进 committed 正文。
      const finishResult = await this.finishActiveComposition()
      // 2. 等 coordinator 命令队列空闲（snapshot 已包含最后输入）
      if (!await this.awaitBridgeFinishEditing()) {
        // 原生正文未能与 Core 对齐：保留原生编辑状态，拒绝关闭。
        return false
      }
      await this.editorCoordinator.whenIdle()
      // 3. composition 唯一真源是 Core snapshot。finish 失败 / snapshot 丢失 / composition 仍在 → 拒绝关闭。
      const afterFinish = this.editorCoordinator.getSnapshot()
      if (finishResult.success === false || afterFinish === null ||
        (afterFinish.composition !== null && afterFinish.composition !== undefined)) {
        return false
      }
      // 4. Issue #829：等写作统计真正写完再离开。统计没写进去不该拦住用户离开，失败只记日志。
      const statsBarrier: number = this.statsQueue.currentSeq()
      const statsFlushed: boolean = await this.flushEditorStatsQueue(statsBarrier)
      if (!statsFlushed) {
        console.warn(`[WritingWorkflowCoordinator] 写作统计未全部落盘（丢弃 ${this.statsQueue.dropped()} 条）`)
      }
      // 5. 保存稳定 snapshot。即使全文与上次相同也要调用：输入后还原会留下
      // 尚未 durable 的证明批次，Core 需要建仅事件 journal。
      const savedText: string = afterFinish.text
      const settled: boolean = await this.ensureSnapshotSaved(
        savedText, false, false, this.snapshotSaveSource(afterFinish))
      if (!settled) {
        return false
      }
      // 6. 关闭编辑会话。Issue #629 评论9 第2项：先 closeAsync。
      // Issue #629 评论11 第3项：close 必须 success===true && data===true 才算成功。
      const closeResult = await this.editorCoordinator.closeAsync()
      if (!closeResult.success || closeResult.data !== true) {
        return false
      }
      return true
    } catch (err) {
      return false
    }
  }

  // Issue #629 评论9 第3项 + 评论11 第3项：统一的离开请求入口。
  // 走 NavigationTransactionCoordinator.safePopWithStack（内部先 await leaveGuard=performGracefulClose），
  // 成功才 pop；失败留在本页并提示。
  requestLeave(): Promise<boolean> {
    if (this.leaveTask !== null) {
      return this.leaveTask
    }

    const task: Promise<boolean> = (async () => {
      const ok = await this.navigation.safePopWithStack(this.navPathStack)

      if (!ok) {
        hilog.error(0x0000, 'WritingWorkflowCoordinator', 'requestLeave: graceful close rejected')
        this.saveMessage = this.textProvider(WritingTextKey.LeaveRejected)
        this.emit(null, null)
      }
      return ok
    })()

    this.leaveTask = task
    task.finally(() => {
      if (this.leaveTask === task) {
        this.leaveTask = null
      }
    })
    return task
  }

  // Issue #629 评论14 第1项：宽屏左侧切章节。
  // 先完成当前 Writing 的 performGracefulClose()（safeReplacePathWithStack 内部 guard），
  // 成功后把当前 Writing route 替换成目标章节的 Writing route；失败保持原章节。
  async switchChapter(chapterId: string, chapterTitle: string, volumeId: string): Promise<void> {
    if (chapterId === this.chapterId) {
      return // 已是当前章节
    }
    const targetKey = `writing:${this.projectId}:${volumeId}:${chapterId}`
    await this.navigation.safeReplacePathWithStack(this.navPathStack, targetKey, () => {
      const replaceParam: WritingRouteParams = {
        chapterId: chapterId,
        chapterTitle: chapterTitle,
        projectId: this.projectId,
        volumeId: volumeId
      }
      this.navPathStack.replacePath({
        name: Routes.Writing,
        param: replaceParam
      })
    })
  }

  // ── 内部工具 ──

  // 提交当前活跃的 IME composition（如果有）。
  // composition 唯一真源是 Core snapshot：从 snapshot.composition 读 sessionId/generation。
  // 无 composition 时 no-op（返回 success）。
  private async finishActiveComposition(): Promise<ResultEnvelope<EditorEditResult>> {
    const snap = this.editorCoordinator.getSnapshot()
    if (!snap || snap.composition === null || snap.composition === undefined) {
      return { success: true }
    }
    const comp = snap.composition
    return this.editorCoordinator.finishComposition(comp.sessionId, comp.generation)
  }

  // 等写作统计队列写完 barrier 及之前入队的那些事件。
  private flushEditorStatsQueue(barrier: number): Promise<boolean> {
    return this.statsQueue.flushThrough(barrier)
  }

  // 保存 / 清空成功提示 2s 后自动清除（与页面原行为一致）。
  private scheduleSaveMessageClear(): void {
    setTimeout(() => {
      this.saveMessage = ''
      this.emit(null, null)
    }, 2000)
  }
}
