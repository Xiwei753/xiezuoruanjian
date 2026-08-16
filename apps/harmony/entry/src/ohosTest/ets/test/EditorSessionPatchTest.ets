// EditorSessionPatchTest.ets — EditorSessionState.withEditResult 行为测试。
// 环境阻塞：无 hvigor/ohpm/SDK，需在 CI 或本地 HarmonyOS 环境运行。
// 运行：在 DevEco Studio 中以 ohosTest 模块运行，或 hvigorw assembleHap。
//
// 覆盖：
//   1. applied outcome 正确更新 text/revision/cursor/selectionAnchor
//   2. staleRevision outcome 不更新（返回原 state）
//   3. composition generation 从 compositionSession.generation 取
//   4. appliedWithAdjustedSelection 正常应用
//   5. noChange 保留 text 但更新 selection

import { describe, it, expect } from '@ohos/hypium'
import { EditorSessionState } from '../../../main/ets/feature/editor/session/EditorSessionState'
import {
  EditorSessionSnapshot,
  EditorEditResult,
  EditorEditOutcome,
  DisplayPatch,
  CompositionSession,
  EditorContentDelta,
  EditorVisualIntent
} from '../../../main/ets/corebridge/dto/EditorDtos'

export default function editorSessionPatchTest() {
  describe('EditorSessionState.withEditResult', () => {
    // 工具：构造完整 EditorEditResult
    const makeResult = (overrides: Partial<EditorEditResult>): EditorEditResult => {
      const base: EditorEditResult = {
        outcome: EditorEditOutcome.Applied,
        transactionId: 1,
        baseRevision: 1,
        newRevision: 2,
        displayPatches: [],
        oldSelectionStart: 0,
        oldSelectionEnd: 0,
        newSelectionStart: 0,
        newSelectionEnd: 0,
        visualIntent: {} as EditorVisualIntent,
        compositionSession: null,
        contentDelta: {
          insertedChars: 0,
          deletedChars: 0,
          insertedNonWhitespaceChars: 0,
          deletedNonWhitespaceChars: 0
        } as EditorContentDelta
      }
      return Object.assign({}, base, overrides) as EditorEditResult
    }

    const makeSnapshot = (overrides: Partial<EditorSessionSnapshot>): EditorSessionSnapshot => {
      const base: EditorSessionSnapshot = {
        text: '',
        revision: 1,
        cursor: 0,
        selectionAnchor: 0,
        generation: 0,
        chapterId: 'c1'
      }
      return Object.assign({}, base, overrides) as EditorSessionSnapshot
    }

    it('applied outcome 正确更新 text/revision/cursor/selectionAnchor', () => {
      const snap: EditorSessionSnapshot = makeSnapshot({
        text: 'hello', revision: 1, cursor: 5, selectionAnchor: 5, generation: 0
      })
      const state: EditorSessionState = EditorSessionState.fromCreate(42, snap)
      const patch: DisplayPatch = {
        baseRevision: 1,
        newRevision: 2,
        replaceByteStart: 5,
        replaceByteEndExclusive: 5,
        insertedText: '!',
        resultingSelectionStart: 6,
        resultingSelectionEnd: 6
      }
      const result: EditorEditResult = makeResult({
        outcome: EditorEditOutcome.Applied,
        baseRevision: 1,
        newRevision: 2,
        displayPatches: [patch],
        newSelectionStart: 6,
        newSelectionEnd: 6,
        compositionSession: null
      })
      const next: EditorSessionState = state.withEditResult(result)
      expect(next.snapshot.text).assertEqual('hello!')
      expect(next.snapshot.revision).assertEqual(2)
      expect(next.snapshot.cursor).assertEqual(6)
      expect(next.snapshot.selectionAnchor).assertEqual(6)
      expect(next.snapshot.generation).assertEqual(0)
      expect(next.sessionId).assertEqual(42)
    })

    it('staleRevision outcome 不更新（返回原 state）', () => {
      const snap: EditorSessionSnapshot = makeSnapshot({
        text: 'hello', revision: 5, cursor: 3, selectionAnchor: 3, generation: 7
      })
      const state: EditorSessionState = EditorSessionState.fromCreate(42, snap)
      const result: EditorEditResult = makeResult({
        outcome: EditorEditOutcome.StaleRevision,
        newRevision: 99
      })
      const next: EditorSessionState = state.withEditResult(result)
      expect(next.snapshot.text).assertEqual('hello')
      expect(next.snapshot.revision).assertEqual(5)
      expect(next.snapshot.generation).assertEqual(7)
    })

    it('composition generation 从 compositionSession.generation 取', () => {
      const snap: EditorSessionSnapshot = makeSnapshot({
        text: 'a', revision: 1, cursor: 1, selectionAnchor: 1, generation: 0
      })
      const state: EditorSessionState = EditorSessionState.fromCreate(42, snap)
      const comp: CompositionSession = { sessionId: 7, baseRevision: 1, generation: 3 }
      const result: EditorEditResult = makeResult({
        outcome: EditorEditOutcome.Applied,
        compositionSession: comp
      })
      const next: EditorSessionState = state.withEditResult(result)
      expect(next.snapshot.generation).assertEqual(3)
    })

    it('appliedWithAdjustedSelection 正常应用 patch 和更新 selection', () => {
      const snap: EditorSessionSnapshot = makeSnapshot({
        text: 'hello', revision: 1, cursor: 0, selectionAnchor: 0, generation: 0
      })
      const state: EditorSessionState = EditorSessionState.fromCreate(42, snap)
      const patch: DisplayPatch = {
        baseRevision: 1,
        newRevision: 2,
        replaceByteStart: 5,
        replaceByteEndExclusive: 5,
        insertedText: ' world',
        resultingSelectionStart: 6,
        resultingSelectionEnd: 11
      }
      const result: EditorEditResult = makeResult({
        outcome: EditorEditOutcome.AppliedWithAdjustedSelection,
        baseRevision: 1,
        newRevision: 2,
        displayPatches: [patch],
        newSelectionStart: 3,
        newSelectionEnd: 8,
        compositionSession: null
      })
      const next: EditorSessionState = state.withEditResult(result)
      expect(next.snapshot.text).assertEqual('hello world')
      expect(next.snapshot.revision).assertEqual(2)
      expect(next.snapshot.cursor).assertEqual(8)
      expect(next.snapshot.selectionAnchor).assertEqual(3)
    })

    it('noChange 保留 text 但更新 selection', () => {
      const snap: EditorSessionSnapshot = makeSnapshot({
        text: 'hello', revision: 3, cursor: 0, selectionAnchor: 0, generation: 0
      })
      const state: EditorSessionState = EditorSessionState.fromCreate(42, snap)
      const result: EditorEditResult = makeResult({
        outcome: EditorEditOutcome.NoChange,
        baseRevision: 3,
        newRevision: 3,
        displayPatches: [],
        newSelectionStart: 2,
        newSelectionEnd: 4,
        compositionSession: null
      })
      const next: EditorSessionState = state.withEditResult(result)
      expect(next.snapshot.text).assertEqual('hello')
      expect(next.snapshot.revision).assertEqual(3)
      expect(next.snapshot.cursor).assertEqual(4)
      expect(next.snapshot.selectionAnchor).assertEqual(2)
    })

    it('invalidOffset outcome 不更新', () => {
      const snap: EditorSessionSnapshot = makeSnapshot({
        text: 'hello', revision: 5, cursor: 3, selectionAnchor: 3, generation: 7
      })
      const state: EditorSessionState = EditorSessionState.fromCreate(42, snap)
      const result: EditorEditResult = makeResult({
        outcome: EditorEditOutcome.InvalidOffset,
        newRevision: 99
      })
      const next: EditorSessionState = state.withEditResult(result)
      expect(next.snapshot.revision).assertEqual(5)
    })

    it('invalidRange outcome 不更新', () => {
      const snap: EditorSessionSnapshot = makeSnapshot({
        text: 'hello', revision: 5, cursor: 3, selectionAnchor: 3, generation: 7
      })
      const state: EditorSessionState = EditorSessionState.fromCreate(42, snap)
      const result: EditorEditResult = makeResult({
        outcome: EditorEditOutcome.InvalidRange,
        newRevision: 99
      })
      const next: EditorSessionState = state.withEditResult(result)
      expect(next.snapshot.revision).assertEqual(5)
    })

    it('多 patch 顺序应用', () => {
      const snap: EditorSessionSnapshot = makeSnapshot({
        text: 'abc', revision: 1, cursor: 3, selectionAnchor: 3, generation: 0
      })
      const state: EditorSessionState = EditorSessionState.fromCreate(42, snap)
      const patch1: DisplayPatch = {
        baseRevision: 1, newRevision: 2,
        replaceByteStart: 3, replaceByteEndExclusive: 3,
        insertedText: 'def',
        resultingSelectionStart: 3, resultingSelectionEnd: 6
      }
      const patch2: DisplayPatch = {
        baseRevision: 2, newRevision: 3,
        replaceByteStart: 0, replaceByteEndExclusive: 0,
        insertedText: 'XYZ',
        resultingSelectionStart: 0, resultingSelectionEnd: 3
      }
      const result: EditorEditResult = makeResult({
        outcome: EditorEditOutcome.Applied,
        baseRevision: 1, newRevision: 3,
        displayPatches: [patch1, patch2],
        newSelectionStart: 0, newSelectionEnd: 3,
        compositionSession: null
      })
      const next: EditorSessionState = state.withEditResult(result)
      expect(next.snapshot.text).assertEqual('XYZabcdef')
      expect(next.snapshot.revision).assertEqual(3)
    })
  })
}
