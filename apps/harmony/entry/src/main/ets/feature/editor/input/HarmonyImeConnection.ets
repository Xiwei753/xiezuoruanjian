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
  private dispatcher: EditorSemanticDispatcher | null = null
  private attached: boolean = false
  // Issue #767 评论 5828448634 第 1 节：单一状态源——attach 进行中的 Promise。
  // 同一 dispatcher 在 attach 进行中时直接复用同一个 task，避免并发 attach。
  // attach 失败时清掉 attachTask / dispatcher / attached，允许下一次点击重试。
  private attachTask: Promise<void> | null = null
  // Issue #767 评论 5828681434 第 1 节：attach 生命周期 generation/token。
  // 每次 detach 递增 generation，让所有进行中的旧 attach 失效。
  // 旧 controller.attach() await 返回后检查 generation：已失效则立即 controller.detach()，
  // 不注册 listeners，不设 attached=true。避免页面离开后 IME 重新 attach 成功。
  private attachGeneration: number = 0

  // 编辑框获得焦点时调用：绑定语义调度器，attach 到 IME，注册事件回调。
  // 接受 dispatcher 而非 inputAdapter + selectionController：所有输入经 dispatcher 统一排队。
  // Issue #767 评论 5828448634 第 1 节：单一 attach owner，避免并发 attach 竞态。
  // - 已 attached 且 dispatcher 还是当前 dispatcher：直接返回（不 detach 重连）；
  // - attach 进行中（attachTask != null）且同一 dispatcher：复用同一个 task；
  // - attached 但 dispatcher 已变化：才 detach 后重新 attach；
  // - attach 失败时清掉 attachTask / dispatcher / attached，允许下一次点击重试。
  // Issue #767 评论 5828681434 第 1 节：attachTask != null 但 dispatcher 不同时，
  // 不能直接启动第二个 doAttach()——整个 InputMethodController 同一时间只能有一条
  // attach/detach 链。先递增 generation 让当前 attach 失效，等当前 task 完全结束后
  // （包括失效后的 controller.detach()）再切 dispatcher 重新 attach。
  attach(dispatcher: EditorSemanticDispatcher): Promise<void> {
    // 已 attached 且 dispatcher 未变化：直接返回，避免反复 detach/attach。
    if (this.attached && this.dispatcher === dispatcher) {
      hilog.info(IME_LOG_DOMAIN, IME_LOG_TAG, 'attach: already attached with same dispatcher, skip')
      return Promise.resolve()
    }
    // attach 进行中且同一 dispatcher：复用同一个 task，避免并发 attach。
    if (this.attachTask !== null && this.dispatcher === dispatcher) {
      hilog.info(IME_LOG_DOMAIN, IME_LOG_TAG, 'attach: attach in progress with same dispatcher, reuse task')
      return this.attachTask
    }
    // attach 进行中但 dispatcher 已变化：递增 generation 让当前 attach 失效，
    // 等当前 task 完全结束后再重新 attach（确保 controller 只有一条 attach/detach 链）。
    if (this.attachTask !== null && this.dispatcher !== dispatcher) {
      hilog.info(IME_LOG_DOMAIN, IME_LOG_TAG, 'attach: dispatcher changed during attach, wait then re-attach')
      this.attachGeneration++
      const prevTask = this.attachTask
      const newTask = prevTask.then(
        () => this.doAttach(dispatcher),
        () => this.doAttach(dispatcher)
      )
      this.attachTask = newTask
      newTask.then(
        () => { if (this.attachTask === newTask) this.attachTask = null },
        () => { if (this.attachTask === newTask) this.attachTask = null }
      )
      return newTask
    }
    // attached 但 dispatcher 已变化：先 detach 再重新 attach。
    if (this.attached && this.dispatcher !== dispatcher) {
      hilog.info(IME_LOG_DOMAIN, IME_LOG_TAG, 'attach: dispatcher changed, detach then re-attach')
      const newTask = this.detach().then(() => {
        return this.doAttach(dispatcher)
      })
      this.attachTask = newTask
      newTask.then(
        () => { if (this.attachTask === newTask) this.attachTask = null },
        () => { if (this.attachTask === newTask) this.attachTask = null }
      )
      return newTask
    }
    // 首次 attach 或上次失败后重试。
    const task = this.doAttach(dispatcher)
    this.attachTask = task
    task.then(
      () => { if (this.attachTask === task) this.attachTask = null },
      () => { if (this.attachTask === task) this.attachTask = null }
    )
    return task
  }

  // 真正执行 attach 的内部方法。调用方负责管理 attachTask 状态。
  // Issue #767 评论 5828681434 第 1 节：generation 检查防止失效 attach 继续。
  // - doAttach 开始时记录当前 attachGeneration；
  // - await controller.attach() 返回后检查 generation 是否仍是自己的；
  // - 已失效（detach 在 attach 进行中调用过）则立即 controller.detach()，不注册 listeners，不设 attached；
  // - 只有 generation 仍有效时才能 registerListeners() 并提交 attached 状态。
  private async doAttach(dispatcher: EditorSemanticDispatcher): Promise<void> {
    this.dispatcher = dispatcher
    const myGeneration = this.attachGeneration
    // API26 InputMethodController.attach 需要 TextConfig；素笺是多行文本编辑框，
    // 用 MULTILINE + UNSPECIFIED 回车键类型。IME 通过 selectByRange 事件主动查询选区。
    const textConfig: inputMethod.TextConfig = {
      inputAttribute: {
        textInputType: inputMethod.TextInputType.MULTILINE,
        enterKeyType: inputMethod.EnterKeyType.UNSPECIFIED,
      },
    }
    hilog.info(IME_LOG_DOMAIN, IME_LOG_TAG, 'attach: start')
    try {
      await this.controller.attach(true, textConfig)
    } catch (e) {
      // Issue #767 评论 5828448634 第 1 节：attach 失败时清掉所有状态，允许下次重试。
      // Issue #767 评论 5828681434 第 1 节：只在 generation 仍有效时才清状态，
      // 避免覆盖 detach 或新 doAttach 已设置的状态。
      hilog.error(IME_LOG_DOMAIN, IME_LOG_TAG, `attach: failure, reason: ${JSON.stringify(e)}`)
      if (myGeneration === this.attachGeneration) {
        this.dispatcher = null
        this.attached = false
      }
      throw new Error(`IME attach failed: ${JSON.stringify(e)}`)
    }
    // Issue #767 评论 5828681434 第 1 节：generation 检查。
    // 如果 detach 在 attach 进行中调用过（递增了 attachGeneration），
    // 此 attach 已失效——立即 controller.detach()，不注册 listeners，不设 attached。
    if (myGeneration !== this.attachGeneration) {
      hilog.info(IME_LOG_DOMAIN, IME_LOG_TAG, 'attach: generation mismatch, detaching stale attach')
      try {
        await this.controller.detach()
      } catch (_e) {
        // detach 失败不阻塞清理
      }
      return
    }
    this.registerListeners()
    this.attached = true
    hilog.info(IME_LOG_DOMAIN, IME_LOG_TAG, 'attach: success')
  }

  // 失焦/页面退出时调用：取消监听并 detach。
  // Issue #767 评论 5828448634 第 1 节：detach 时也清掉 attachTask。
  // Issue #767 评论 5828681434 第 1 节：先递增 attachGeneration，让所有进行中的旧 attach 失效。
  // 旧 attach 的 doAttach 在 await controller.attach() 返回后检测到 generation 不匹配，
  // 会立即 controller.detach() 并 return，不注册 listeners，不设 attached=true。
  // 这避免了"页面已 blur/离开，IME 却在后面重新 attach 成功"的竞态。
  async detach(): Promise<void> {
    this.attachGeneration++
    this.attachTask = null
    if (!this.attached) {
      return
    }
    this.unregisterListeners()
    try {
      await this.controller.detach()
    } catch (_e) {
      // detach 失败不阻塞清理
    }
    this.attached = false
    this.dispatcher = null
  }

  isAttached(): boolean {
    return this.attached
  }

  // 通知 IME 当前选区（UTF-16 code unit offset，ArkUI 事件坐标）。
  // Issue #744 评论 5794965395 第 5 节：API26 InputMethodController 没有 setSelection 方法
  // （只有 changeSelection(text, start, end)，需要传完整 text，不适合这里主动推送）。
  // API26 IME 通过 selectByRange 事件主动查询选区，编辑器不主动推送，这里保留 no-op。
  async notifySelection(_utf16Start: number, _utf16End: number): Promise<void> {
    if (!this.attached) {
      return
    }
    // no-op：API26 IME 通过 selectByRange 事件主动查询选区。
  }

  // 通知 IME 当前光标位置（UTF-16 code unit offset）。
  // Issue #744 评论 5794965395 第 5 节：同 notifySelection，API26 改 no-op。
  async notifyCursor(_utf16Offset: number): Promise<void> {
    if (!this.attached) {
      return
    }
    // no-op：API26 IME 通过 selectByRange 事件主动查询选区。
  }

  // Issue #629 R11 评论5329310563 第2项：从 dispatcher 读当前 display selection 并通知 IME。
  // 在 dispatch Promise resolve（命令已应用、snapshot 已更新）后调用，
  // 避免拿原 hitPos 猜未来状态通知系统。已持有 dispatcher 字段，无循环依赖。
  async syncSelectionFromEditor(): Promise<void> {
    if (!this.dispatcher) {
      return
    }
    const sel = this.dispatcher.getCurrentDisplaySelectionUtf16()
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
    if (!this.dispatcher) {
      return
    }
    hilog.info(IME_LOG_DOMAIN, IME_LOG_TAG, `insertText: length=${text.length}`)
    this.dispatcher.dispatchFireAndForget({ kind: 'imeCommitText', text })
  }

  // IME 向左删除（退格）：派发 imeDeleteLeft（带长度）。
  // Issue #744 评论 5794965395 第 5 节：API26 事件是 deleteLeft(length)，
  // 删除边界由 Core grapheme boundary 决定，不退回 UTF-16 ±1。
  // length 是 IME 要求删除的字符数；dispatcher 在唯一队列里逐个 grapheme 边界连续删除。
  private onDeleteLeft(length: number): void {
    if (!this.dispatcher) {
      return
    }
    hilog.info(IME_LOG_DOMAIN, IME_LOG_TAG, `deleteLeft: length=${length}`)
    this.dispatcher.dispatchFireAndForget({ kind: 'imeDeleteLeft', length: length })
  }

  // IME 向右删除（向前删除）：派发 imeDeleteRight（带长度）。
  // Issue #744 评论 5794965395 第 5 节：API26 事件是 deleteRight(length)，
  // 删除边界由 Core grapheme boundary 决定，不退回 UTF-16 ±1。
  private onDeleteRight(length: number): void {
    if (!this.dispatcher) {
      return
    }
    hilog.info(IME_LOG_DOMAIN, IME_LOG_TAG, `deleteRight: length=${length}`)
    this.dispatcher.dispatchFireAndForget({ kind: 'imeDeleteRight', length: length })
  }

  // IME 选区变化：翻译成 imeSetSelection 语义命令，经 dispatcher 派发。
  // Issue #744 评论 5794965395 第 5 节：API26 用 selectByRange 事件，Range = { start, end }。
  // Issue #629 评论8 第4项：IME 来源的 selection 不再复用普通 setSelection/dragSelect
  // （那会走 committed selection，把 Core/UI composition 状态拆开）；
  // composition 内外判断由 EditorSemanticDispatcher 在唯一队列出队时做，
  // 本连接只负责传 start/end UTF-16 显示坐标。
  private onSelectByRange(range: inputMethod.Range): void {
    if (!this.dispatcher) {
      return
    }
    this.dispatcher.dispatchFireAndForget({ kind: 'imeSetSelection', utf16Start: range.start, utf16End: range.end })
  }

  // IME composition preedit：纯翻译器，派发 imePreviewText。
  // Issue #744 评论 5794965395 第 5 节：API26 setPreviewText 回调签名是 (text: string, range: Range) => void。
  // begin 还是 update 由 EditorSemanticDispatcher 出队时查 isComposing() 决定：
  //   - 未开始 composition → onCompositionBegin 成功后紧接 onCompositionUpdate
  //   - 已开始 composition → 只 onCompositionUpdate
  // 不在本类队列外判断 composing 状态。dispatcher 串行队列保证 begin→update→update→finish/cancel 顺序。
  // range 参数目前不参与语义命令（preedit cursor 由 Core snapshot 驱动），保留以匹配 API26 签名。
  private onSetPreviewText(text: string, _range: inputMethod.Range): void {
    if (!this.dispatcher) {
      return
    }
    hilog.info(IME_LOG_DOMAIN, IME_LOG_TAG, `setPreviewText: length=${text.length}`)
    this.dispatcher.dispatchFireAndForget({ kind: 'imePreviewText', text })
  }

  // IME 取消 composition preedit：纯翻译器，派发 imeCancelPreview。
  // Issue #744 评论 5794965395 第 5 节：API26 用 finishTextPreview 事件（不是 unsetPreviewText）。
  // 是否真的 cancel 由 EditorSemanticDispatcher 出队时查 isComposing() 决定（无 composition 时 no-op）。
  private onFinishTextPreview(): void {
    if (!this.dispatcher) {
      return
    }
    hilog.info(IME_LOG_DOMAIN, IME_LOG_TAG, 'finishTextPreview')
    this.dispatcher.dispatchFireAndForget({ kind: 'imeCancelPreview' })
  }
}
