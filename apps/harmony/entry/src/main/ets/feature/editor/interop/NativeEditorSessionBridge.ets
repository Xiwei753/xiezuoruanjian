// NativeEditorSessionBridge — IEditorSessionBridge 的 native 实现。
// 只做参数类型转换、JSON 解包和调用 native module，不写编辑规则。
// 职责对齐 Android TextEditSessionBridge。
// Issue #629 评论 3：create→parseEnvelope<number>, snapshot→parseEnvelope<EditorSessionSnapshot>,
// 编辑命令→parseEnvelope<EditorEditResult>。

import { IEditorSessionBridge } from './IEditorSessionBridge'
import { ResultEnvelope, ChangedEntity } from '../../../corebridge/dto/ResultEnvelope'
import { EditorTransactionCause, EditorSessionSnapshot, EditorEditResult } from '../../../corebridge/dto/EditorDtos'
import { NativeCoreModule } from '../../../corebridge/native/NativeCoreModule'

export class NativeEditorSessionBridge implements IEditorSessionBridge {
  private core: NativeCoreModule = new NativeCoreModule()

  async create(targetId: string, initialText: string, initialCursorByteOffset: number, isPersistent: boolean): Promise<ResultEnvelope<number>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeEditorSessionCreate(targetId, initialText, initialCursorByteOffset, isPersistent ? 1 : 0) as string
      return this.core.parseEnvelope<number>(jsonStr)
    } catch (e) {
      return this.nativeError<number>(e)
    }
  }

  async close(sessionId: number): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeEditorSessionClose(sessionId) as string
      return this.core.parseEnvelope<boolean>(jsonStr)
    } catch (e) {
      return this.nativeError<boolean>(e)
    }
  }

  async snapshot(sessionId: number): Promise<ResultEnvelope<EditorSessionSnapshot>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeEditorSessionSnapshot(sessionId) as string
      return this.core.parseEnvelope<EditorSessionSnapshot>(jsonStr)
    } catch (e) {
      return this.nativeError<EditorSessionSnapshot>(e)
    }
  }

  async insert(sessionId: number, byteOffset: number, text: string, cause: EditorTransactionCause, expectedRevision: number): Promise<ResultEnvelope<EditorEditResult>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeEditorSessionInsert(sessionId, byteOffset, text, cause, expectedRevision) as string
      return this.core.parseEnvelope<EditorEditResult>(jsonStr)
    } catch (e) {
      return this.nativeError<EditorEditResult>(e)
    }
  }

  async delete(sessionId: number, byteOffsetStart: number, byteOffsetEnd: number, cause: EditorTransactionCause, expectedRevision: number): Promise<ResultEnvelope<EditorEditResult>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeEditorSessionDelete(sessionId, byteOffsetStart, byteOffsetEnd, cause, expectedRevision) as string
      return this.core.parseEnvelope<EditorEditResult>(jsonStr)
    } catch (e) {
      return this.nativeError<EditorEditResult>(e)
    }
  }

  async replace(sessionId: number, byteOffsetStart: number, byteOffsetEnd: number, text: string, originalText: string, cause: EditorTransactionCause, expectedRevision: number): Promise<ResultEnvelope<EditorEditResult>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeEditorSessionReplace(sessionId, byteOffsetStart, byteOffsetEnd, text, originalText, cause, expectedRevision) as string
      return this.core.parseEnvelope<EditorEditResult>(jsonStr)
    } catch (e) {
      return this.nativeError<EditorEditResult>(e)
    }
  }

  async setSelection(sessionId: number, anchorByteOffset: number, headByteOffset: number, expectedRevision: number): Promise<ResultEnvelope<EditorEditResult>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeEditorSessionSetSelection(sessionId, anchorByteOffset, headByteOffset, expectedRevision) as string
      return this.core.parseEnvelope<EditorEditResult>(jsonStr)
    } catch (e) {
      return this.nativeError<EditorEditResult>(e)
    }
  }

  async undo(sessionId: number, expectedRevision: number): Promise<ResultEnvelope<EditorEditResult>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeEditorSessionUndo(sessionId, expectedRevision) as string
      return this.core.parseEnvelope<EditorEditResult>(jsonStr)
    } catch (e) {
      return this.nativeError<EditorEditResult>(e)
    }
  }

  async redo(sessionId: number, expectedRevision: number): Promise<ResultEnvelope<EditorEditResult>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeEditorSessionRedo(sessionId, expectedRevision) as string
      return this.core.parseEnvelope<EditorEditResult>(jsonStr)
    } catch (e) {
      return this.nativeError<EditorEditResult>(e)
    }
  }

  async commitText(sessionId: number, byteStart: number, byteEndExclusive: number, replacementText: string, resultingSelectionAnchor: number, resultingSelectionHead: number, compositionSessionId: number, compositionBaseRevision: number, compositionGeneration: number, cause: EditorTransactionCause, expectedRevision: number): Promise<ResultEnvelope<EditorEditResult>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeEditorSessionCommitText(sessionId, byteStart, byteEndExclusive, replacementText, resultingSelectionAnchor, resultingSelectionHead, compositionSessionId, compositionBaseRevision, compositionGeneration, cause, expectedRevision) as string
      return this.core.parseEnvelope<EditorEditResult>(jsonStr)
    } catch (e) {
      return this.nativeError<EditorEditResult>(e)
    }
  }

  async deleteSurrounding(sessionId: number, beforeByteStart: number, beforeByteEndExclusive: number, afterByteStart: number, afterByteEndExclusive: number, cause: EditorTransactionCause, expectedRevision: number): Promise<ResultEnvelope<EditorEditResult>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeEditorSessionDeleteSurrounding(sessionId, beforeByteStart, beforeByteEndExclusive, afterByteStart, afterByteEndExclusive, cause, expectedRevision) as string
      return this.core.parseEnvelope<EditorEditResult>(jsonStr)
    } catch (e) {
      return this.nativeError<EditorEditResult>(e)
    }
  }

  async beginComposition(sessionId: number, replaceStart: number, replaceEndExclusive: number, expectedRevision: number): Promise<ResultEnvelope<EditorEditResult>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeEditorSessionBeginComposition(sessionId, replaceStart, replaceEndExclusive, expectedRevision) as string
      return this.core.parseEnvelope<EditorEditResult>(jsonStr)
    } catch (e) {
      return this.nativeError<EditorEditResult>(e)
    }
  }

  async updateComposition(sessionId: number, compositionSessionId: number, compositionGeneration: number, newPreeditText: string, newPreeditCursorUtf16: number, expectedRevision: number): Promise<ResultEnvelope<EditorEditResult>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeEditorSessionUpdateComposition(sessionId, compositionSessionId, compositionGeneration, newPreeditText, newPreeditCursorUtf16, expectedRevision) as string
      return this.core.parseEnvelope<EditorEditResult>(jsonStr)
    } catch (e) {
      return this.nativeError<EditorEditResult>(e)
    }
  }

  async finishComposition(sessionId: number, compositionSessionId: number, compositionGeneration: number, expectedRevision: number): Promise<ResultEnvelope<EditorEditResult>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeEditorSessionFinishComposition(sessionId, compositionSessionId, compositionGeneration, expectedRevision) as string
      return this.core.parseEnvelope<EditorEditResult>(jsonStr)
    } catch (e) {
      return this.nativeError<EditorEditResult>(e)
    }
  }

  async cancelComposition(sessionId: number, compositionSessionId: number, compositionGeneration: number, expectedRevision: number): Promise<ResultEnvelope<EditorEditResult>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeEditorSessionCancelComposition(sessionId, compositionSessionId, compositionGeneration, expectedRevision) as string
      return this.core.parseEnvelope<EditorEditResult>(jsonStr)
    } catch (e) {
      return this.nativeError<EditorEditResult>(e)
    }
  }

  async getText(sessionId: number): Promise<ResultEnvelope<string>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeEditorSessionGetText(sessionId) as string
      return this.core.parseEnvelope<string>(jsonStr)
    } catch (e) {
      return this.nativeError<string>(e)
    }
  }

  async getRevision(sessionId: number): Promise<ResultEnvelope<number>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeEditorSessionGetRevision(sessionId) as string
      return this.core.parseEnvelope<number>(jsonStr)
    } catch (e) {
      return this.nativeError<number>(e)
    }
  }

  // #606: grapheme cluster 边界查询。平台端 Left/Right/Backspace/Delete 通过此方法
  // 调用 Core 的 unicode_segmentation，不再自己用 UTF-16 ±1 猜字符边界。
  async previousGraphemeBoundary(sessionId: number, byteOffset: number): Promise<ResultEnvelope<number>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeEditorSessionPreviousGraphemeBoundary(sessionId, byteOffset) as string
      return this.core.parseEnvelope<number>(jsonStr)
    } catch (e) {
      return this.nativeError<number>(e)
    }
  }

  async nextGraphemeBoundary(sessionId: number, byteOffset: number): Promise<ResultEnvelope<number>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeEditorSessionNextGraphemeBoundary(sessionId, byteOffset) as string
      return this.core.parseEnvelope<number>(jsonStr)
    } catch (e) {
      return this.nativeError<number>(e)
    }
  }

  private nativeError<T>(e: unknown): ResultEnvelope<T> {
    return {
      success: false,
      errorCode: 'NATIVE_ERROR',
      messageKey: 'error.native_error',
      messageArgs: { 'detail': String(e) } as Record<string, string>,
      rawError: String(e),
      warnings: [] as string[],
      changedPaths: [] as string[],
      changedEntities: [] as ChangedEntity[]
    }
  }
}
