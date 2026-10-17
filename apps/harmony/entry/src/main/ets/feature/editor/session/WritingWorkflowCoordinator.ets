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
import { IWriterCoreBridge } from '../../../corebridge/IWriterCoreBridge'
import { EditorSessionSnapshot, EditorEditResult } from '../../../corebridge/dto/EditorDtos'
import { EditorSessionCoordinator, EditorStateUpdate } from './EditorSessionCoordinator'
import { EditorSemanticDispatcher } from '../input/EditorSemanticDispatcher'
import { EditorStatsQueue } from '../stats/EditorStatsQueue'
import { NavigationTransactionCoordinator } from '../../../app/navigation/NavigationTransactionCoordinator'
import { HarmonyImeConnection } from '../input/HarmonyImeConnection'
import { Routes, WritingRouteParams } from '../../../app/navigation/Routes'

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

export class WritingWorkflowCoordinator {
  private bridge: IWriterCoreBridge
  private editorCoordinator: EditorSessionCoordinator
  private dispatcher: EditorSemanticDispatcher
  private statsQueue: EditorStatsQueue
  private navigation: NavigationTransactionCoordinator
  private imeConnection: HarmonyImeConnection
  private textProvider: WritingTextProvider

  // 页面监听器（显示状态回流）。
  private listener: WritingWorkflowListener | null = null
  // 页面在会话打开后恢复平台侧编辑器状态（接续选区 / viewport）的回调。
  private sessionOpenedHook: ((snap: EditorSessionSnapshot) => Promise<void>) | null = null

  // 路由上下文：chapterId/projectId/volumeId/navPathStack。
  private chapterId: string = ''
  private projectId: string = ''
  private volumeId: string = ''
  private navPathStack: NavPathStack = new NavPathStack()

  // ── 显示状态（页面映射到 @State；正文投影自 Core snapshot，不是第二套编辑状态）──
  private content: string = ''
  private wordCount: number = 0
  private hasUnsavedChanges: boolean = false
  private lastSavedContent: string = ''
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

  constructor(
    bridge: IWriterCoreBridge,
    editorCoordinator: EditorSessionCoordinator,
    dispatcher: EditorSemanticDispatcher,
    statsQueue: EditorStatsQueue,
    navigation: NavigationTransactionCoordinator,
    imeConnection: HarmonyImeConnection,
    textProvider: WritingTextProvider
  ) {
    this.bridge = bridge
    this.editorCoordinator = editorCoordinator
    this.dispatcher = dispatcher
    this.statsQueue = statsQueue
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

  private emit(snapshot: EditorSessionSnapshot | null, editResult: EditorEditResult | null): void {
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
    const update: WritingWorkflowUpdate = { state: state, snapshot: snapshot, editResult: editResult }
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
      // Issue #829 评论 #5995146525：正文每次真实变化就在这里产出统计事件，
      // 不再等保存后比较整章。enqueueChange 只入内存队列，不跨 NAPI、不 await。
      this.statsQueue.enqueueChange(
        this.projectId, this.volumeId, this.chapterId,
        update.editResult.cause, delta.insertedChars, delta.deletedChars
      )
    }
    this.emit(snap, update.editResult)
  }

  // ── 加载 ──

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
  async performBackgroundSave(): Promise<void> {
    await this.dispatcher.flush()
    await this.editorCoordinator.whenIdle()
    if (this.hasUnsavedChanges) {
      await this.saveChapter(true)
    }
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
