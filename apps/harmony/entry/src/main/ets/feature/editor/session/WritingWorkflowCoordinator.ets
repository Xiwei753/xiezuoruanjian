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
// - 所有编辑命令入口仍是唯一的 EditorSessionCoordinator / EditorSemanticDispatcher。
// - 平台文案（资源字符串）、IME 键盘收起、接续 / 分享 payload、滚动 viewport 等
//   仍由页面处理；本类通过 WritingTextKey 文案键与会话打开回调与其协作。
//
// Issue #629 评论6/7/8/9/11/14 的各项 close / save / leave 语义原样迁移到这里。

import hilog from '@ohos.hilog'
import { util } from '@kit.ArkTS'
import { IWriterCoreBridge } from '../../../corebridge/IWriterCoreBridge'
import { EditorSessionSnapshot, EditorEditResult, EditorTransactionCause } from '../../../corebridge/dto/EditorDtos'
import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'
import { EditorSessionCoordinator, EditorStateUpdate } from './EditorSessionCoordinator'
import { EditorSemanticDispatcher } from '../input/EditorSemanticDispatcher'
import { EditorStatsQueue } from '../stats/EditorStatsQueue'
import { WritingSessionTracker } from '../stats/WritingSessionTracker'
import { NavigationTransactionCoordinator } from '../../../app/navigation/NavigationTransactionCoordinator'
import { HarmonyImeConnection } from '../input/HarmonyImeConnection'
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
interface ActiveSaveTask {
  text: string
  task: Promise<boolean>
}

export class WritingWorkflowCoordinator implements ActiveWritingSession {
  private bridge: IWriterCoreBridge
  private editorCoordinator: EditorSessionCoordinator
  private dispatcher: EditorSemanticDispatcher
  private statsQueue: EditorStatsQueue
  private writingSessionTracker: WritingSessionTracker
  private navigation: NavigationTransactionCoordinator
  private imeConnection: HarmonyImeConnection
  private textProvider: WritingTextProvider

  // 页面监听器（显示状态回流）。
  private listener: WritingWorkflowListener | null = null
  // 页面在会话打开后恢复平台侧编辑器状态（接续选区 / viewport）的回调。
  private sessionOpenedHook: ((snap: EditorSessionSnapshot) => Promise<void>) | null = null

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
  private syncBarrierSealed: boolean = false

  constructor(
    bridge: IWriterCoreBridge,
    editorCoordinator: EditorSessionCoordinator,
    dispatcher: EditorSemanticDispatcher,
    statsQueue: EditorStatsQueue,
    writingSessionTracker: WritingSessionTracker,
    navigation: NavigationTransactionCoordinator,
    imeConnection: HarmonyImeConnection,
    textProvider: WritingTextProvider
  ) {
    this.bridge = bridge
    this.editorCoordinator = editorCoordinator
    this.dispatcher = dispatcher
    this.statsQueue = statsQueue
    this.writingSessionTracker = writingSessionTracker
    this.navigation = navigation
    this.imeConnection = imeConnection
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

  // 会话 open 成功后，页面恢复接续选区 / viewport（平台侧状态）用。
  setSessionOpenedHook(hook: ((snap: EditorSessionSnapshot) => Promise<void>) | null): void {
    this.sessionOpenedHook = hook
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
        proofBaseline
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
        // Issue #629 评论 5 第 8 节：跨设备内容身份来自 ChapterContent.meta.hash。
        this.lastSavedContentHash = result.data.meta.hash
        this.wordCount = result.data.meta.wordCount
        this.hasUnsavedChanges = false
        // 打开编辑会话，用 Core 管理正文。open 成功后 listener 自动回流显示状态。
        // Issue #653 评论 5608056955：close 在离开时统一做，open 不接收 isPersistent。
        const snapResult = await this.editorCoordinator.open(this.chapterId, result.data.content)
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
    await this.dispatcher.flush()
    await this.editorCoordinator.whenIdle()
    const snap = this.editorCoordinator.getSnapshot()
    const targetText: string = snap ? snap.text : this.content
    return this.ensureSnapshotSaved(targetText, isAutoSave)
  }

  // Issue #629 评论 5308748920 问题3：统一保存接口，保证指定 targetText 落盘。
  // 若 activeSave 正好保存 targetText 且成功，直接 true；
  // 若目标不同，先 await 旧任务，再启动一次精确保存 targetText 的新事务。
  async ensureSnapshotSaved(targetText: string, isAutoSave: boolean): Promise<boolean> {
    // 若有进行中保存任务，先等它结束
    if (this.activeSave !== null) {
      const prev = this.activeSave
      let prevOk: boolean = false
      try {
        prevOk = await prev.task
      } catch (err) {
        // 旧任务异常，继续启动新事务
      }
      // 旧任务保存的正是 targetText 且成功 → 直接复用结果
      if (prev.text === targetText && prevOk) {
        return true
      }
      // 目标不同或旧任务失败：清 activeSave，启动新事务
      if (this.activeSave === prev) {
        this.activeSave = null
      }
    }
    // 启动一次精确保存 targetText 的新事务
    const task: Promise<boolean> = this.doSaveChapter(targetText, isAutoSave)
    const saveEntry: ActiveSaveTask = { text: targetText, task: task }
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

  // Issue #629 评论 5308748920 问题3：真实持久化函数，只接受显式 targetText。
  // 不在里面重新抓"当前正文"（避免保存期间新输入被误存）。
  // 结算：lastSavedContent 按 targetText；再取 latestSnapshot 结算 hasUnsavedChanges / wordCount。
  private async doSaveChapter(targetText: string, isAutoSave: boolean): Promise<boolean> {
    this.isSaving = true
    this.isAutoSaving = isAutoSave
    this.saveMessage = ''
    // Issue #829 评论 #5996577737 第 2 项：在这份正文定型之前记下统计 barrier。
    const statsBarrier: number = this.statsQueue.currentSeq()
    this.emit(null, null)

    try {
      // 只保存这份 targetText（不重新抓当前正文）
      const result = await this.bridge.saveChapter(this.chapterId, targetText)
      if (result.success && result.data) {
        // Issue #829 评论 #5995146525：这里不再上报写作统计（改由编辑事务同源上报）。
        this.lastSavedContent = targetText
        // Issue #629 评论 5 第 8 节：保存成功后跨设备内容身份来自 ChapterSaveReceipt.contentHash。
        this.lastSavedContentHash = result.data.contentHash
        this.hasUnsavedChanges = false
        this.lastSaveFailed = false
        // 重新取 latestSnapshot 结算：保存期间若又产生新输入，hasUnsavedChanges 保持 true。
        // Issue #629 评论9 第4项：只有保存期间无新输入时才用 receipt.wordCount 校正实时字数。
        const latest = this.editorCoordinator.getSnapshot()
        if (latest) {
          this.content = latest.text
          this.hasUnsavedChanges = latest.text !== targetText
          if (latest.text === targetText) {
            this.wordCount = result.data.wordCount
          }
        } else {
          this.wordCount = result.data.wordCount
        }
        this.saveMessage = isAutoSave
          ? this.textProvider(WritingTextKey.AutoSaved)
          : this.textProvider(WritingTextKey.Saved)
        this.scheduleSaveMessageClear()
        // Issue #829 评论 #5996028848：正文已经落盘，等这份正文对应的统计事件也写完再离开。
        const statsFlushed: boolean = await this.flushEditorStatsQueue(statsBarrier)
        if (!statsFlushed) {
          console.warn(`[WritingWorkflowCoordinator] 本次保存对应的写作统计未全部落盘（丢弃 ${this.statsQueue.dropped()} 条）`)
        }
        // 保存成功后回流（contentHash 前进，此时 hasUnsavedChanges=false，页面据此更新接续状态）。
        this.emit(null, null)
        return true
      } else {
        this.lastSaveFailed = true
        this.saveMessage = this.textProvider(WritingTextKey.SaveFailed)
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
    await this.dispatcher.flush()
    await this.editorCoordinator.whenIdle()
    // 无待保存变化：本次后台事务直接成功，不产生多余 Core 写入。
    if (!this.hasUnsavedChanges) {
      return true
    }
    const snap = this.editorCoordinator.getSnapshot()
    const targetText: string = snap ? snap.text : this.content
    return this.ensureSnapshotSaved(targetText, true)
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
      // 先封住输入：传输期间不得继续往将被替换的 live 章节写旧内存内容。
      // sealed 只拒绝新命令，已排队命令仍可 drain（与关闭事务同一语义）。
      this.dispatcher.seal()
      this.syncBarrierSealed = true
      // 同一会话边界：先提交 IME composition，再 drain 输入，再取稳定 snapshot。
      const finishResult = await this.dispatcher.finishActiveComposition()
      await this.dispatcher.flush()
      await this.editorCoordinator.whenIdle()
      const snap = this.editorCoordinator.getSnapshot()
      if (finishResult.success === false || snap === null ||
        (snap.composition !== null && snap.composition !== undefined)) {
        // composition 未结算 / snapshot 丢失：不放行同步（不静默用旧文本覆盖）。
        this.endSyncBarrier()
        return { ok: false }
      }
      const saved: boolean = await this.ensureSnapshotSaved(snap.text, false)
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
    const diskHash: string = result.data.meta.hash
    if (this.lastSavedContentHash !== null && diskHash === this.lastSavedContentHash) {
      // Core 落盘身份与屏障放行时一致：本次同步未改动本章。
      return true
    }
    // 磁盘已变化：确认没有仍在排队的编辑命令，再决定能否回填。
    await this.dispatcher.flush()
    await this.editorCoordinator.whenIdle()
    const snap = this.editorCoordinator.getSnapshot()
    if (snap === null) {
      // 会话已不存在（页面正在关闭）：只更新投影基线，正文以 Core 落盘为准。
      this.content = diskContent
      this.lastSavedContent = diskContent
      this.lastSavedContentHash = diskHash
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
    const reopened = await this.editorCoordinator.open(this.chapterId, diskContent)
    if (!reopened.success || !reopened.data) {
      return false
    }
    this.content = diskContent
    this.lastSavedContent = diskContent
    this.lastSavedContentHash = diskHash
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
    if (this.syncBarrierSealed) {
      this.dispatcher.unseal()
      this.syncBarrierSealed = false
    }
    this.syncBarrierHeld = false
  }

  // ── 清空正文 ──

  // Issue #629 评论 5 第 4 节 + 评论9 第4项 + 评论11 第1项：
  // 清空正文走 Dispatcher 的 clearDocument 语义命令，不绕过队列直接 replace。
  async clearDocument(): Promise<void> {
    // dispatch clearDocument：命令进 Dispatcher 队列，保证之前的输入先执行。
    this.isSaving = true
    this.emit(null, null)
    const result = await this.dispatcher.dispatch({ kind: 'clearDocument' })
    this.isSaving = false
    this.emit(null, null)
    if (!result.success) {
      this.lastSaveFailed = true
      this.saveMessage = this.textProvider(WritingTextKey.ClearFailed)
      this.emit(null, null)
      return
    }
    // clear 之后用户立刻打的新字允许继续进入 Dispatcher；ensureSnapshotSaved('') 只保存空基线。
    const saved: boolean = await this.ensureSnapshotSaved('', false)
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
  // seal（拒新输入）→ finishActiveComposition → drain dispatcher → coordinator idle
  // → 保存稳定 snapshot → 保存成功后 detach IME → close Core session。
  // 幂等：closeTask 复用同一 Promise；失败后重置允许重试。
  async performGracefulClose(): Promise<boolean> {
    // 无编辑会话（零章节工作台）：没有 session 需要 close/save/detach，直接允许离开。
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
  // Issue #629 评论8 第3项 + 评论9 第2项：顺序
  //   seal → finishActiveComposition → drain → whenIdle → save → closeAsync → detach。
  private async doGracefulClose(): Promise<boolean> {
    // 1. dispatcher 进入 sealed：拒绝新的普通输入命令（已排队命令仍可 drain）
    this.dispatcher.seal()
    try {
      // 2. 同一 dispatcher 队列里 finish 当前 composition：最后的中文 preedit 提交进 committed 正文。
      const finishResult = await this.dispatcher.finishActiveComposition()
      // 3. drain dispatcher（所有已排队语义输入应用完）
      await this.dispatcher.flush()
      // 4. 等 coordinator 命令队列空闲（snapshot 已包含最后输入）
      await this.editorCoordinator.whenIdle()
      // 5. composition 唯一真源是 Core snapshot。finish 失败 / snapshot 丢失 / composition 仍在 → 拒绝关闭。
      const afterFinish = this.editorCoordinator.getSnapshot()
      if (finishResult.success === false || afterFinish === null ||
        (afterFinish.composition !== null && afterFinish.composition !== undefined)) {
        this.dispatcher.unseal()
        return false
      }
      // 6. Issue #829：等写作统计真正写完再离开。统计没写进去不该拦住用户离开，失败只记日志。
      const statsBarrier: number = this.statsQueue.currentSeq()
      const statsFlushed: boolean = await this.flushEditorStatsQueue(statsBarrier)
      if (!statsFlushed) {
        console.warn(`[WritingWorkflowCoordinator] 写作统计未全部落盘（丢弃 ${this.statsQueue.dropped()} 条）`)
      }
      // 7. 保存稳定 snapshot（此时不再读可能已变化的 this.content）
      const savedText: string = afterFinish.text
      if (savedText !== this.lastSavedContent) {
        const settled: boolean = await this.ensureSnapshotSaved(savedText, false)
        if (!settled) {
          // 保存失败：不 detach、不 close、不 pop；解锁输入让用户继续编辑（下次返回重试）。
          this.dispatcher.unseal()
          return false
        }
      }
      // 7. 关闭编辑会话。Issue #629 评论9 第2项：先 closeAsync，成功后才 detach IME。
      // Issue #629 评论11 第3项：close 必须 success===true && data===true 才算成功。
      const closeResult = await this.editorCoordinator.closeAsync()
      if (!closeResult.success || closeResult.data !== true) {
        this.dispatcher.unseal()
        return false
      }
      // 8. Core close 成功后才 detach IME（不可逆清理放到最后）。
      await this.imeConnection.detach()
      return true
    } catch (err) {
      // 异常路径按失败处理：不 pop（正文仍在活跃会话），解锁输入允许重试，不 detach IME。
      this.dispatcher.unseal()
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
