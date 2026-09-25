// HarmonyImeConnection — HarmonyOS IME Kit 自绘编辑框连接。
// 把 IME Kit 事件翻译成语义命令，经 EditorSemanticDispatcher 统一排队执行。
// 不把 HarmonyOS 原始 IME 对象传进 Rust Core；只翻译成 insert/delete/selection/composition 语义命令。
//
// Issue #629 评论6 Part A：统一串行边界。
// - 不再自己拥有 imeQueue（SerialCommandQueue）；所有 IME 事件经 EditorSemanticDispatcher 派发。
// - Dispatcher 持有唯一串行队列，物理键盘/IME/鼠标/触摸都从这里排队。
//
// Issue #629 评论7 第2项：HarmonyImeConnection 变纯协议翻译器。
// - 删除 composing/lastPreedit 本地状态（不再在队列外判断 composition begin/update/finish/cancel）。
// - 只派发三种语义命令：imePreviewText / imeCommitText / imeCancelPreview。
// - composition 状态判断（begin/update/finish/cancel）由 EditorSemanticDispatcher 在唯一队列出队时
//   查 inputAdapter.isComposing()（Core 驱动状态）决定，不在本类队列外判断。
// - 串行队列保证事件顺序，composition 状态判断属于同一条串行边界。
//
// Issue #629 评论7 第6项：软键盘删除统一 Core grapheme。
// - onDeleteText 派发 graphemeBackspace/graphemeDelete（不再派发 {kind:'delete',direction}）。
// - 删除边界由 Core previousGraphemeBoundary/nextGraphemeBoundary 决定，不手算 UTF-16 surrogate pair。
//
// Issue #744 评论 5794965395 第 5 节：按 API26 IMEKit d.ts 重写事件监听。
// - API26 InputMethodController 真实事件：insertText / deleteLeft(length) / deleteRight(length) /
//   setPreviewText(text, range) / finishTextPreview / selectByRange(Range)。
// - 删除不存在的旧事件：deleteText / DeleteDirection / sendEnterKey / setSelection /
//   unsetPreviewText / inputMethod.Selection。回车通过 insertText('\n') 处理。
// - IME 删除带 length，派发 imeDeleteLeft/imeDeleteRight（带长度），在 dispatcher 唯一队列里
//   按 Core grapheme boundary 连续求边界后一次完成，不退回 UTF-16 ±1。
// - notifySelection/notifyCursor 改 no-op：API26 InputMethodController 没有 setSelection 方法，
//   IME 通过 selectByRange 事件主动查询选区，编辑器不主动推送。

import { inputMethod } from '@kit.IMEKit'
import hilog from '@ohos.hilog'
import { EditorSemanticDispatcher } from './EditorSemanticDispatcher'

const IME_LOG_DOMAIN = 0x0000
const IME_LOG_TAG = 'HarmonyImeConnection'

export class HarmonyImeConnection {
  private controller: inputMethod.InputMethodController = inputMethod.getController()
  // Issue #767 评论 5828898817：唯一串行 transition 链。
  // 不再分别维护 attachTask + attached + generation，让 attach/detach 各自抢状态。
  // 所有 IME 生命周期操作串到同一条 transitionTask 后面，保证同一时间只有一条 attach/detach 链。
  // desiredDispatcher：期望连接的 dispatcher（attach 写入，detach 写 null）。
  // attachedDispatcher：实际已连接的 dispatcher（reconcile 成功后写入，detach 后写 null）。
  // isAttached() 直接由 attachedDispatcher !== null 判断，不再维护另一份 bool。
  private transitionTask: Promise<void> = Promise.resolve()
  private desiredDispatcher: EditorSemanticDispatcher | null = null
  private attachedDispatcher: EditorSemanticDispatcher | null = null

  // 编辑框获得焦点时调用：绑定语义调度器，attach 到 IME，注册事件回调。
  // Issue #767 评论 5828898817：attach 只做两件事：
  // 1. 写 desiredDispatcher = dispatcher；
  // 2. 把"把实际状态收敛到 desiredDispatcher"的 reconcile 操作串到 transitionTask 后面。
  // 不再分别维护 attachTask + attached + generation，避免并发窗口。
  attach(dispatcher: EditorSemanticDispatcher): Promise<void> {
    this.desiredDispatcher = dispatcher
    const task = this.transitionTask.then(() => this.reconcile())
    // transitionTask 始终保持 resolved 状态（吞掉错误），链不会因单次 reconcile 失败而断掉。
    this.transitionTask = task.then(
      () => undefined,
      () => undefined
    )
    return task
  }

  // 失焦/页面退出时调用：取消监听并 detach。
  // Issue #767 评论 5828898817：detach 只做两件事：
  // 1. 写 desiredDispatcher = null；
  // 2. 同样串到 transitionTask 后面；
  // 3. 返回这个 transition Promise。
  // 不再把 attachTask 设 null 来模拟取消——串行链保证旧 attach 完成后才会处理 detach。
  detach(): Promise<void> {
    this.desiredDispatcher = null
    const task = this.transitionTask.then(() => this.reconcile())
    this.transitionTask = task.then(
      () => undefined,
      () => undefined
    )
    return task
  }

  // Issue #767 评论 5828898817：唯一串行 reconcile——把实际状态收敛到 desiredDispatcher。
  // - 当前已连接旧 dispatcher 而 desired 不同：先 unregister + await controller.detach；
  // - desired 为 null：结束；
  // - desired 非 null 且当前未连接：await controller.attach；
  // - attach 返回后再次检查 desired 是否还是本 dispatcher；
  // - 已变更就立刻 detach，不注册 listener；
  // - 仍一致才 registerListeners，并把 attachedDispatcher = desiredDispatcher。
  // 这样 attach、detach、快速 blur→focus、dispatcher 切换全部只有一条串行控制链。
  private async reconcile(): Promise<void> {
    const desired = this.desiredDispatcher

    // 当前已连接旧 dispatcher 而 desired 不同：先 unregister + await controller.detach。
    if (this.attachedDispatcher !== null && this.attachedDispatcher !== desired) {
      hilog.info(IME_LOG_DOMAIN, IME_LOG_TAG, 'reconcile: detach old dispatcher')
      this.unregisterListeners()
      try {
        await this.controller.detach()
      } catch (_e) {
        // detach 失败不阻塞清理
      }
      this.attachedDispatcher = null
    }

    // desired 为 null：结束。
    if (desired === null) {
      return
    }

    // desired 非 null 且当前未连接：await controller.attach。
    if (this.attachedDispatcher === null) {
      const textConfig: inputMethod.TextConfig = {
        inputAttribute: {
          textInputType: inputMethod.TextInputType.MULTILINE,
          enterKeyType: inputMethod.EnterKeyType.UNSPECIFIED,
        },
      }
      hilog.info(IME_LOG_DOMAIN, IME_LOG_TAG, 'reconcile: attach start')
      try {
        await this.controller.attach(true, textConfig)
      } catch (e) {
        hilog.error(IME_LOG_DOMAIN, IME_LOG_TAG, `reconcile: attach failed: ${JSON.stringify(e)}`)
        throw new Error(`IME attach failed: ${JSON.stringify(e)}`)
      }

      // attach 返回后再次检查 desired 是否还是本 dispatcher。
      if (this.desiredDispatcher !== desired) {
        // 已变更就立刻 detach，不注册 listener。
        hilog.info(IME_LOG_DOMAIN, IME_LOG_TAG, 'reconcile: desired changed during attach, detaching')
        try {
          await this.controller.detach()
        } catch (_e) {
          // detach 失败不阻塞清理
        }
        return
      }

      // 仍一致才 registerListeners，并把 attachedDispatcher = desiredDispatcher。
      this.registerListeners()
      this.attachedDispatcher = desired
      hilog.info(IME_LOG_DOMAIN, IME_LOG_TAG, 'reconcile: attach success')
    }
  }

  isAttached(): boolean {
    return this.attachedDispatcher !== null
  }

  // 通知 IME 当前选区（UTF-16 code unit offset，ArkUI 事件坐标）。
  // Issue #744 评论 5794965395 第 5 节：API26 InputMethodController 没有 setSelection 方法
  // （只有 changeSelection(text, start, end)，需要传完整 text，不适合这里主动推送）。
  // API26 IME 通过 selectByRange 事件主动查询选区，编辑器不主动推送，这里保留 no-op。
  async notifySelection(_utf16Start: number, _utf16End: number): Promise<void> {
    if (!this.attachedDispatcher) {
      return
    }
    // no-op：API26 IME 通过 selectByRange 事件主动查询选区。
  }

  // 通知 IME 当前光标位置（UTF-16 code unit offset）。
  // Issue #744 评论 5794965395 第 5 节：同 notifySelection，API26 改 no-op。
  async notifyCursor(_utf16Offset: number): Promise<void> {
    if (!this.attachedDispatcher) {
      return
    }
    // no-op：API26 IME 通过 selectByRange 事件主动查询选区。
  }

  // Issue #629 R11 评论5329310563 第2项：从 dispatcher 读当前 display selection 并通知 IME。
  // 在 dispatch Promise resolve（命令已应用、snapshot 已更新）后调用，
  // 避免拿原 hitPos 猜未来状态通知系统。已持有 attachedDispatcher 字段，无循环依赖。
  async syncSelectionFromEditor(): Promise<void> {
    if (!this.attachedDispatcher) {
      return
    }
    const sel = this.attachedDispatcher.getCurrentDisplaySelectionUtf16()
    if (sel !== null) {
      await this.notifySelection(sel.start, sel.end)
    }
  }

  // Issue #744 评论 5794965395 第 5 节：按 API26 InputMethodController d.ts 注册事件。
  // - insertText(text) / deleteLeft(length) / deleteRight(length) /
  //   setPreviewText(text, range) / finishTextPreview() / selectByRange(Range)。
  // - 删除不存在的旧事件 deleteText/sendEnterKey/setSelection/unsetPreviewText。
  private registerListeners(): void {
    this.controller.on('insertText', (text: string): void => { this.onInsertText(text) })
    this.controller.on('deleteLeft', (length: number): void => { this.onDeleteLeft(length) })
    this.controller.on('deleteRight', (length: number): void => { this.onDeleteRight(length) })
    this.controller.on('selectByRange', (range: inputMethod.Range): void => { this.onSelectByRange(range) })
    this.controller.on('setPreviewText', (text: string, range: inputMethod.Range): void => { this.onSetPreviewText(text, range) })
    this.controller.on('finishTextPreview', (): void => { this.onFinishTextPreview() })
  }

  private unregisterListeners(): void {
    this.controller.off('insertText')
    this.controller.off('deleteLeft')
    this.controller.off('deleteRight')
    this.controller.off('selectByRange')
    this.controller.off('setPreviewText')
    this.controller.off('finishTextPreview')
  }

  // IME 插入文本：纯翻译器，派发 imeCommitText。
  // composition commit 还是普通插入由 EditorSemanticDispatcher 出队时查 isComposing() 决定，
  // 不在本类队列外判断。dispatcher 串行队列保证 IME 事件按到达顺序执行。
  // Issue #744 评论 5794965395 第 5 节：回车也走 insertText('\n')，不再有单独的 sendEnterKey 事件。
  private onInsertText(text: string): void {
    if (!this.attachedDispatcher) {
      return
    }
    hilog.info(IME_LOG_DOMAIN, IME_LOG_TAG, `insertText: length=${text.length}`)
    this.attachedDispatcher.dispatchFireAndForget({ kind: 'imeCommitText', text })
  }

  // IME 向左删除（退格）：派发 imeDeleteLeft（带长度）。
  // Issue #744 评论 5794965395 第 5 节：API26 事件是 deleteLeft(length)，
  // 删除边界由 Core grapheme boundary 决定，不退回 UTF-16 ±1。
  // length 是 IME 要求删除的字符数；dispatcher 在唯一队列里逐个 grapheme 边界连续删除。
  private onDeleteLeft(length: number): void {
    if (!this.attachedDispatcher) {
      return
    }
    hilog.info(IME_LOG_DOMAIN, IME_LOG_TAG, `deleteLeft: length=${length}`)
    this.attachedDispatcher.dispatchFireAndForget({ kind: 'imeDeleteLeft', length: length })
  }

  // IME 向右删除（向前删除）：派发 imeDeleteRight（带长度）。
  // Issue #744 评论 5794965395 第 5 节：API26 事件是 deleteRight(length)，
  // 删除边界由 Core grapheme boundary 决定，不退回 UTF-16 ±1。
  private onDeleteRight(length: number): void {
    if (!this.attachedDispatcher) {
      return
    }
    hilog.info(IME_LOG_DOMAIN, IME_LOG_TAG, `deleteRight: length=${length}`)
    this.attachedDispatcher.dispatchFireAndForget({ kind: 'imeDeleteRight', length: length })
  }

  // IME 选区变化：翻译成 imeSetSelection 语义命令，经 dispatcher 派发。
  // Issue #744 评论 5794965395 第 5 节：API26 用 selectByRange 事件，Range = { start, end }。
  // Issue #629 评论8 第4项：IME 来源的 selection 不再复用普通 setSelection/dragSelect
  // （那会走 committed selection，把 Core/UI composition 状态拆开）；
  // composition 内外判断由 EditorSemanticDispatcher 在唯一队列出队时做，
  // 本连接只负责传 start/end UTF-16 显示坐标。
  private onSelectByRange(range: inputMethod.Range): void {
    if (!this.attachedDispatcher) {
      return
    }
    this.attachedDispatcher.dispatchFireAndForget({ kind: 'imeSetSelection', utf16Start: range.start, utf16End: range.end })
  }

  // IME composition preedit：纯翻译器，派发 imePreviewText。
  // Issue #744 评论 5794965395 第 5 节：API26 setPreviewText 回调签名是 (text: string, range: Range) => void。
  // begin 还是 update 由 EditorSemanticDispatcher 出队时查 isComposing() 决定：
  //   - 未开始 composition → onCompositionBegin 成功后紧接 onCompositionUpdate
  //   - 已开始 composition → 只 onCompositionUpdate
  // 不在本类队列外判断 composing 状态。dispatcher 串行队列保证 begin→update→update→finish/cancel 顺序。
  // range 参数目前不参与语义命令（preedit cursor 由 Core snapshot 驱动），保留以匹配 API26 签名。
  private onSetPreviewText(text: string, _range: inputMethod.Range): void {
    if (!this.attachedDispatcher) {
      return
    }
    hilog.info(IME_LOG_DOMAIN, IME_LOG_TAG, `setPreviewText: length=${text.length}`)
    this.attachedDispatcher.dispatchFireAndForget({ kind: 'imePreviewText', text })
  }

  // IME 取消 composition preedit：纯翻译器，派发 imeCancelPreview。
  // Issue #744 评论 5794965395 第 5 节：API26 用 finishTextPreview 事件（不是 unsetPreviewText）。
  // 是否真的 cancel 由 EditorSemanticDispatcher 出队时查 isComposing() 决定（无 composition 时 no-op）。
  private onFinishTextPreview(): void {
    if (!this.attachedDispatcher) {
      return
    }
    hilog.info(IME_LOG_DOMAIN, IME_LOG_TAG, 'finishTextPreview')
    this.attachedDispatcher.dispatchFireAndForget({ kind: 'imeCancelPreview' })
  }
}
