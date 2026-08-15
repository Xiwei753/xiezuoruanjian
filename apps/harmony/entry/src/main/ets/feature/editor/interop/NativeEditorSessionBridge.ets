// NativeEditorSessionBridge — IEditorSessionBridge 的 native 实现。
// 只做参数类型转换、JSON 解包和调用 native module，不写编辑规则。
// 职责对齐 Android TextEditSessionBridge。

import { IEditorSessionBridge, EditorSnapshot } from './IEditorSessionBridge'
import { ResultEnvelope, ChangedEntity } from '../../../corebridge/dto/ResultEnvelope'
import { NativeCoreModule } from '../../../corebridge/native/NativeCoreModule'

export class NativeEditorSessionBridge implements IEditorSessionBridge {
  private core: NativeCoreModule = new NativeCoreModule()

  async create(initialText: string): Promise<ResultEnvelope<EditorSnapshot>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeEditorSessionCreate(initialText) as string
      return this.core.parseEnvelope<EditorSnapshot>(jsonStr)
    } catch (e) {
      return this.nativeError<EditorSnapshot>(e)
    }
  }

  async close(sessionId: string): Promise<ResultEnvelope<boolean>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeEditorSessionClose(sessionId) as string
      return this.core.parseEnvelope<boolean>(jsonStr)
    } catch (e) {
      return this.nativeError<boolean>(e)
    }
  }

  async snapshot(sessionId: string): Promise<ResultEnvelope<EditorSnapshot>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeEditorSessionSnapshot(sessionId) as string
      return this.core.parseEnvelope<EditorSnapshot>(jsonStr)
    } catch (e) {
      return this.nativeError<EditorSnapshot>(e)
    }
  }

  async insert(sessionId: string, byteOffset: number, text: string): Promise<ResultEnvelope<EditorSnapshot>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeEditorSessionInsert(sessionId, byteOffset, text) as string
      return this.core.parseEnvelope<EditorSnapshot>(jsonStr)
    } catch (e) {
      return this.nativeError<EditorSnapshot>(e)
    }
  }

  async delete(sessionId: string, byteOffsetStart: number, byteOffsetEnd: number): Promise<ResultEnvelope<EditorSnapshot>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeEditorSessionDelete(sessionId, byteOffsetStart, byteOffsetEnd) as string
      return this.core.parseEnvelope<EditorSnapshot>(jsonStr)
    } catch (e) {
      return this.nativeError<EditorSnapshot>(e)
    }
  }

  async replace(sessionId: string, byteOffsetStart: number, byteOffsetEnd: number, text: string): Promise<ResultEnvelope<EditorSnapshot>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeEditorSessionReplace(sessionId, byteOffsetStart, byteOffsetEnd, text) as string
      return this.core.parseEnvelope<EditorSnapshot>(jsonStr)
    } catch (e) {
      return this.nativeError<EditorSnapshot>(e)
    }
  }

  async setSelection(sessionId: string, anchorByteOffset: number, headByteOffset: number): Promise<ResultEnvelope<EditorSnapshot>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeEditorSessionSetSelection(sessionId, anchorByteOffset, headByteOffset) as string
      return this.core.parseEnvelope<EditorSnapshot>(jsonStr)
    } catch (e) {
      return this.nativeError<EditorSnapshot>(e)
    }
  }

  async undo(sessionId: string): Promise<ResultEnvelope<EditorSnapshot>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeEditorSessionUndo(sessionId) as string
      return this.core.parseEnvelope<EditorSnapshot>(jsonStr)
    } catch (e) {
      return this.nativeError<EditorSnapshot>(e)
    }
  }

  async redo(sessionId: string): Promise<ResultEnvelope<EditorSnapshot>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeEditorSessionRedo(sessionId) as string
      return this.core.parseEnvelope<EditorSnapshot>(jsonStr)
    } catch (e) {
      return this.nativeError<EditorSnapshot>(e)
    }
  }

  async commitText(sessionId: string, byteOffset: number, text: string): Promise<ResultEnvelope<EditorSnapshot>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeEditorSessionCommitText(sessionId, byteOffset, text) as string
      return this.core.parseEnvelope<EditorSnapshot>(jsonStr)
    } catch (e) {
      return this.nativeError<EditorSnapshot>(e)
    }
  }

  async deleteSurrounding(sessionId: string, beforeByteLen: number, afterByteLen: number): Promise<ResultEnvelope<EditorSnapshot>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeEditorSessionDeleteSurrounding(sessionId, beforeByteLen, afterByteLen) as string
      return this.core.parseEnvelope<EditorSnapshot>(jsonStr)
    } catch (e) {
      return this.nativeError<EditorSnapshot>(e)
    }
  }

  async beginComposition(sessionId: string, byteOffset: number): Promise<ResultEnvelope<EditorSnapshot>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeEditorSessionBeginComposition(sessionId, byteOffset) as string
      return this.core.parseEnvelope<EditorSnapshot>(jsonStr)
    } catch (e) {
      return this.nativeError<EditorSnapshot>(e)
    }
  }

  async updateComposition(sessionId: string, preedit: string): Promise<ResultEnvelope<EditorSnapshot>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeEditorSessionUpdateComposition(sessionId, preedit) as string
      return this.core.parseEnvelope<EditorSnapshot>(jsonStr)
    } catch (e) {
      return this.nativeError<EditorSnapshot>(e)
    }
  }

  async finishComposition(sessionId: string, committed: string): Promise<ResultEnvelope<EditorSnapshot>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeEditorSessionFinishComposition(sessionId, committed) as string
      return this.core.parseEnvelope<EditorSnapshot>(jsonStr)
    } catch (e) {
      return this.nativeError<EditorSnapshot>(e)
    }
  }

  async cancelComposition(sessionId: string): Promise<ResultEnvelope<EditorSnapshot>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeEditorSessionCancelComposition(sessionId) as string
      return this.core.parseEnvelope<EditorSnapshot>(jsonStr)
    } catch (e) {
      return this.nativeError<EditorSnapshot>(e)
    }
  }

  async getText(sessionId: string): Promise<ResultEnvelope<string>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeEditorSessionGetText(sessionId) as string
      return this.core.parseEnvelope<string>(jsonStr)
    } catch (e) {
      return this.nativeError<string>(e)
    }
  }

  async getRevision(sessionId: string): Promise<ResultEnvelope<number>> {
    try {
      const jsonStr: string = this.core.getNativeModule().nativeEditorSessionGetRevision(sessionId) as string
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
