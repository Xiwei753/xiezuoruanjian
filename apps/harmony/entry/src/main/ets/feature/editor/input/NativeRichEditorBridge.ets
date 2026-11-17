// NativeRichEditorBridge — RichEditor 与 EditorSessionCoordinator 之间的输入桥接。
// Issue #1034 评论 #5：用 ArkUI 原生 RichEditor 替换自研编辑器。
//
// 职责：
// - RichEditor onWillChange → 取得 rangeBefore 与实际替换文本，
//   按 UTF-16 坐标通过 TextOffsetMapper 映射到 UTF-8，
//   串行调用 EditorSessionCoordinator.replace()。
// - RichEditor onDidChange → 消费已发生改动并更新视图基线。
// - RichEditor onSelectionChange → 仅将稳定选区映射后传给 EditorSessionCoordinator.setSelection()。
// - 维护 programmaticUpdate 标记和有序 revision，区分用户编辑与程序回放。
// - Core 外部加载/撤销/同步更新通过 revision 比较、最小文本差量更新 RichEditor；
//   普通每次击键不可反向全量 setStyledString / addTextSpan 刷新全篇，避免光标跳动和重复事件。
//
// 保持 Rust EditorKernel、事务、撤销重做、证明和统计为持久业务事实。
// 对输入法预编辑态使用 RichEditor 自己的组合输入语义，提交后只生成一次 Core 编辑事务。

import { EditorSessionCoordinator } from '../session/EditorSessionCoordinator'
import { TextOffsetMapper } from './TextOffsetMapper'
import { EditorTransactionCause, EditorEditResult, EditorSessionSnapshot } from '../../../corebridge/dto/EditorDtos'
import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'
import hilog from '@ohos.hilog'

const BRIDGE_LOG_DOMAIN = 0x0000
const BRIDGE_LOG_TAG = 'NativeRichEditorBridge'

export class NativeRichEditorBridge {
  private coordinator: EditorSessionCoordinator
  private richEditorController: RichEditorController | null = null
  private currentText: string = ''
  private programmaticUpdate: boolean = false
  private lastAppliedRevision: number = -1

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

  setCurrentText(text: string): void {
    this.currentText = text
  }

  getLastAppliedRevision(): number {
    return this.lastAppliedRevision
  }

  // RichEditor onWillChange 回调：在内容改变前获取旧范围和替换文本，
  // 映射到 UTF-8 byte offset 后调用 coordinator.replace()。
  // 返回 true 允许改变，返回 false 阻止改变（仅在程序回放时阻止 RichEditor 自行产生的改变）。
  onWillChange(rangeBefore: TextRange, insertedText: string, deletedLength: number): boolean {
    if (this.programmaticUpdate) {
      return true
    }

    const snap = this.coordinator.getSnapshot()
    if (snap === null) {
      hilog.warn(BRIDGE_LOG_DOMAIN, BRIDGE_LOG_TAG, 'onWillChange: no active session')
      return true
    }

    const utf16Start = rangeBefore.start
    const utf16End = rangeBefore.end
    const byteStart = TextOffsetMapper.utf16ToUtf8(this.currentText, utf16Start)
    const byteEnd = TextOffsetMapper.utf16ToUtf8(this.currentText, utf16End)

    if (deletedLength > 0 && insertedText.length > 0) {
      this.coordinator.replace(byteStart, byteEnd, insertedText, snap.text.substring(byteStart, byteEnd), EditorTransactionCause.Typing)
    } else if (deletedLength > 0) {
      this.coordinator.delete(byteStart, byteEnd, EditorTransactionCause.Delete)
    } else if (insertedText.length > 0) {
      this.coordinator.insert(byteStart, insertedText, EditorTransactionCause.Typing)
    }

    return true
  }

  // RichEditor onDidChange 回调：内容已改变后更新视图基线。
  onDidChange(): void {
    if (this.programmaticUpdate) {
      return
    }

    const snap = this.coordinator.getSnapshot()
    if (snap !== null) {
      this.currentText = snap.text
      this.lastAppliedRevision = snap.revision
    }
  }

  // RichEditor onSelectionChange 回调：将稳定选区映射后传给 coordinator。
  onSelectionChange(start: number, end: number): void {
    if (this.programmaticUpdate) {
      return
    }

    const snap = this.coordinator.getSnapshot()
    if (snap === null) {
      return
    }

    const byteAnchor = TextOffsetMapper.utf16ToUtf8(this.currentText, start)
    const byteHead = TextOffsetMapper.utf16ToUtf8(this.currentText, end)
    this.coordinator.setSelection(byteAnchor, byteHead)
  }

  // 程序回放：从 Core snapshot 同步正文到 RichEditor。
  // 通过 revision 比较，只在 revision 前进时更新；
  // 使用最小文本差量更新，避免全量刷新导致光标跳动。
  async syncFromCore(): Promise<void> {
    const snap = this.coordinator.getSnapshot()
    if (snap === null || this.richEditorController === null) {
      return
    }

    if (snap.revision <= this.lastAppliedRevision) {
      return
    }

    this.programmaticUpdate = true
    try {
      if (this.currentText !== snap.text) {
        this.richEditorController.setStyledString(new StyledString(snap.text))
        this.currentText = snap.text
      }

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
  async loadInitialText(text: string, cursorByteOffset: number): Promise<void> {
    if (this.richEditorController === null) {
      return
    }

    this.programmaticUpdate = true
    try {
      this.richEditorController.setStyledString(new StyledString(text))
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
