// NativeRichEditorBridge — RichEditor 与 EditorSessionCoordinator 之间的输入桥接。
// Issue #1034 评论 #5：用 ArkUI 原生 RichEditor 替换自研编辑器。
// Issue #1034 评论 #6 修复：正确使用 RichEditorController API、串行事务队列、UTF-8/UTF-16 坐标转换。
// Issue #1034 评论 #7 修复：getSpans offsetInSpan 截取、deleteSpans() 无参数清空、
//   computeDiff 按 code point、whenIdle 注入保存链路、事务失败恢复。
//
// 职责：
// - RichEditor onWillChange → 只做校验（返回 true 允许改变），不调用 coordinator。
// - RichEditor onDidChange(rangeBefore, rangeAfter) → 从前后文本差量计算编辑事实，
//   串行提交给 EditorSessionCoordinator。
// - RichEditor onSelectionChange → 将稳定选区映射后传给 EditorSessionCoordinator.setSelection()。
// - 维护 programmaticUpdate 标记和有序 revision，区分用户编辑与程序回放。
// - Core 外部加载/撤销/同步更新通过 revision 比较、最小文本差量更新 RichEditor；
//   文本 revision 对齐和 selection/cursor 更新独立处理。

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

export class NativeRichEditorBridge {
  private coordinator: EditorSessionCoordinator
  private richEditorController: RichEditorController | null = null
  private currentText: string = ''
  private programmaticUpdate: boolean = false
  private lastAppliedRevision: number = -1
  private editQueue: Promise<void> = Promise.resolve()
  private pendingPaste: boolean = false

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
  whenIdle(): Promise<void> {
    return this.editQueue
  }

  setPendingPaste(): void {
    this.pendingPaste = true
  }

  // onWillChange 只做校验：允许所有改变。
  // 编辑事实派发在 onDidChange 中完成，避免 onWillChange 中调用 coordinator 但不等结果。
  onWillChange(_value: RichEditorChangeValue): boolean {
    return true
  }

  // onDidChange：内容已改变后，从 rangeBefore/rangeAfter 和本地镜像计算编辑事实，
  // 串行提交给 coordinator。
  onDidChange(rangeBefore: TextRange, rangeAfter: TextRange): void {
    if (this.programmaticUpdate || this.richEditorController === null) {
      return
    }

    // 从 RichEditor 获取改动后插入的文本内容
    // Issue #1034 评论 #8：getSpans 返回 Array<RichEditorImageSpanResult | RichEditorTextSpanResult>，
    // 需按 union 类型筛选纯文本 Span；排序用 spanPosition.spanRange[0]（不是不存在的 spanPosition.start）。
    const insertedSpans = this.richEditorController.getSpans({
      start: rangeAfter.start,
      end: rangeAfter.end
    })
    // 筛选纯文本 Span，按 spanPosition.spanRange[0] 排序
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

    // 从本地镜像推算被删除的旧文本
    const oldText = this.currentText.substring(rangeBefore.start, rangeBefore.end)

    // 推算改动后的完整文本
    const newText = this.currentText.substring(0, rangeBefore.start)
      + insertedText
      + this.currentText.substring(rangeBefore.end)

    // 立即更新本地镜像（不等 coordinator 异步操作完成）
    const previousText = this.currentText
    this.currentText = newText

    // 确定编辑原因
    const cause: EditorTransactionCause = this.pendingPaste ? EditorTransactionCause.Paste : EditorTransactionCause.Typing
    this.pendingPaste = false

    // 串行提交编辑事实给 coordinator
    this.editQueue = this.editQueue.then(async () => {
      const snap = this.coordinator.getSnapshot()
      if (snap === null) {
        hilog.warn(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, 'onDidChange: no active session')
        return
      }

      // 从当笔前镜像换算 UTF-8 byte offset
      const byteStart = TextOffsetMapper.utf16ToUtf8(previousText, rangeBefore.start)
      const byteEnd = TextOffsetMapper.utf16ToUtf8(previousText, rangeBefore.end)

      let result: ResultEnvelope<EditorEditResult> | null = null
      let transactionFailed = false
      if (oldText.length > 0 && insertedText.length > 0) {
        result = await this.coordinator.replace(byteStart, byteEnd, insertedText, oldText, cause)
      } else if (oldText.length > 0) {
        result = await this.coordinator.delete(byteStart, byteEnd, EditorTransactionCause.Delete)
      } else if (insertedText.length > 0) {
        result = await this.coordinator.insert(byteStart, insertedText, cause)
      }

      // Issue #1034 评论 #8：不仅检查 success，还要检查 outcome。
      // staleRevision/invalidOffset/invalidRange 是业务拒绝结果，需从 Core snapshot 对齐。
      if (result !== null) {
        if (!result.success || result.data === undefined ||
            (result.data.outcome !== EditorEditOutcome.Applied &&
             result.data.outcome !== EditorEditOutcome.AppliedWithAdjustedSelection &&
             result.data.outcome !== EditorEditOutcome.NoChange)) {
          transactionFailed = true
        }
      }

      // 事务失败时从 Core snapshot 恢复，不让本地镜像与 Core 分叉
      if (transactionFailed) {
        hilog.warn(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, 'onDidChange: transaction failed, recovering from Core')
        await this.recoverFromCore()
        return
      }

      // 如果 Core 调整了选区位置，更新 RichEditor 的选区
      if (result !== null && result.success && result.data !== undefined &&
          result.data.outcome === EditorEditOutcome.AppliedWithAdjustedSelection) {
        const newSnap = this.coordinator.getSnapshot()
        if (newSnap !== null && this.richEditorController !== null) {
          const utf16Cursor = TextOffsetMapper.utf8ToUtf16(newSnap.text, newSnap.cursor)
          const utf16Anchor = TextOffsetMapper.utf8ToUtf16(newSnap.text, newSnap.selectionAnchor)
          this.richEditorController.setSelection(utf16Anchor, utf16Cursor)
        }
      }

      const newSnap = this.coordinator.getSnapshot()
      if (newSnap !== null) {
        this.lastAppliedRevision = newSnap.revision
      }
    }).catch(async e => {
      hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, `onDidChange error: ${JSON.stringify(e)}`)
      // Issue #1034 评论 #8：异常也触发恢复路径，不让 whenIdle 误判成功。
      await this.recoverFromCore()
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

  // 计算最小文本差量
  // Issue #1034 评论 #7：按完整 Unicode code point 求前后缀，避免 surrogate pair 内部断裂。
  private computeDiff(oldText: string, newText: string): TextDiff {
    // 前缀：按 code point 比较
    let prefixLen = 0
    while (prefixLen < oldText.length && prefixLen < newText.length) {
      const oldCp = oldText.codePointAt(prefixLen)
      const newCp = newText.codePointAt(prefixLen)
      if (oldCp !== newCp) {
        break
      }
      // code point > 0xFFFF 表示是 surrogate pair，占 2 个 UTF-16 code unit
      prefixLen += (oldCp > 0xFFFF) ? 2 : 1
    }

    // 后缀：从末尾按 code point 回退
    let oldSuffixStart = oldText.length
    let newSuffixStart = newText.length
    while (oldSuffixStart > prefixLen && newSuffixStart > prefixLen) {
      // 判断 oldText 中最后一个 code point 的长度
      let oldCpLen = 1
      if (oldSuffixStart >= 2) {
        const lastUnit = oldText.charCodeAt(oldSuffixStart - 1)
        const prevUnit = oldText.charCodeAt(oldSuffixStart - 2)
        if (lastUnit >= 0xDC00 && lastUnit <= 0xDFFF && prevUnit >= 0xD800 && prevUnit <= 0xDBFF) {
          oldCpLen = 2
        }
      }
      // 判断 newText 中最后一个 code point 的长度
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
    } catch (e) {
      hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, `recoverFromCore error: ${JSON.stringify(e)}`)
    } finally {
      this.programmaticUpdate = false
    }
  }

  // 程序回放：从 Core snapshot 同步正文到 RichEditor。
  // 文本 revision 对齐和 selection/cursor 更新独立处理。
  async syncFromCore(): Promise<void> {
    await this.editQueue

    const snap = this.coordinator.getSnapshot()
    if (snap === null || this.richEditorController === null) {
      return
    }

    this.programmaticUpdate = true
    try {
      // 文本内容对齐：只在文本不同时更新
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

      // 选区/光标更新：独立于文本 revision，每次都同步
      const utf16Cursor = TextOffsetMapper.utf8ToUtf16(snap.text, snap.cursor)
      const utf16Anchor = TextOffsetMapper.utf8ToUtf16(snap.text, snap.selectionAnchor)
      this.richEditorController.setSelection(utf16Anchor, utf16Cursor)

      this.lastAppliedRevision = snap.revision
    } catch (e) {
      hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, `syncFromCore error: ${JSON.stringify(e)}`)
    } finally {
      this.programmaticUpdate = false
    }
  }

  // 初始加载：把 Core 返回的初始正文写入 RichEditor。
  // Issue #1034 评论 #7：使用 deleteSpans() 无参数清空全部内容。
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
      }
    } catch (e) {
      hilog.error(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, `loadInitialText error: ${JSON.stringify(e)}`)
    } finally {
      this.programmaticUpdate = false
    }
  }
}
