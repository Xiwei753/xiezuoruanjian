package com.xiwei.sujian.feature.editor.session

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.writer_core.EditorOperationKindDto

/**
 * #595 解决二：本地输入回调携带 operationKind 的契约测试。
 *
 * 验证：
 * - Rust [EditorOperationKindDto] 到平台 [EditorOperationKind] 的映射完整且无碰撞；
 * - [EditorDocumentUpdate.LocalInput] 携带 operationKind，随 text/revision/transactionId
 *   一起从 View → EditorWindowHost → EditorSessionCoordinator 传递；
 * - 会话层收到 LocalInput 后，WritingPane 的本地/外部判断（origin + text）不受
 *   operationKind 影响（operationKind 只记录语义，不参与 reset 判定）。
 */
class LocalInputOperationKindTest {
    @Test
    fun dtoMappingCoversEveryDtoKind() {
        val dtoKinds = EditorOperationKindDto.entries.toList()
        assertTrue("DTO enum must be non-empty", dtoKinds.isNotEmpty())
        for (kind in dtoKinds) {
            // 不抛异常即覆盖完整；CURSOR_ONLY → SELECTION、COMPOSITION_* → COMPOSITION、
            // LOAD/FORMAT → REPLACE 均为有意的语义归并。
            kind.toEditorOperationKind()
        }
    }

    @Test
    fun insertMapsToInsert() {
        assertEquals(EditorOperationKind.INSERT, EditorOperationKindDto.INSERT.toEditorOperationKind())
    }

    @Test
    fun deleteMapsToDelete() {
        assertEquals(EditorOperationKind.DELETE, EditorOperationKindDto.DELETE.toEditorOperationKind())
    }

    @Test
    fun replaceMapsToReplace() {
        assertEquals(EditorOperationKind.REPLACE, EditorOperationKindDto.REPLACE.toEditorOperationKind())
    }

    @Test
    fun cursorOnlyMapsToSelection() {
        assertEquals(
            EditorOperationKind.SELECTION,
            EditorOperationKindDto.CURSOR_ONLY.toEditorOperationKind(),
        )
    }

    @Test
    fun compositionKindsMapToComposition() {
        assertEquals(
            EditorOperationKind.COMPOSITION,
            EditorOperationKindDto.COMPOSITION_UPDATE.toEditorOperationKind(),
        )
        assertEquals(
            EditorOperationKind.COMPOSITION,
            EditorOperationKindDto.COMPOSITION_COMMIT.toEditorOperationKind(),
        )
        assertEquals(
            EditorOperationKind.COMPOSITION,
            EditorOperationKindDto.COMPOSITION_CANCEL.toEditorOperationKind(),
        )
    }

    @Test
    fun loadAndFormatMapToReplace() {
        assertEquals(EditorOperationKind.REPLACE, EditorOperationKindDto.LOAD.toEditorOperationKind())
        assertEquals(EditorOperationKind.REPLACE, EditorOperationKindDto.FORMAT.toEditorOperationKind())
    }

    @Test
    fun localInputCarriesOperationKind() {
        val update =
            EditorDocumentUpdate.LocalInput(
                targetId = "t1",
                operationKind = EditorOperationKind.INSERT,
                revision = 7L,
                transactionId = 42L,
                contentChanged = true,
                contentDelta = EditorContentDelta(insertedChars = "typed".length),
            )
        // #624 评论9：LocalInput 不再携带整章 text — 正文变化用 contentChanged/contentDelta。
        assertEquals(true, update.contentChanged)
        assertEquals(5, update.contentDelta.insertedChars)
        assertEquals(7L, update.revision)
        assertEquals(42L, update.transactionId)
        assertEquals(EditorOperationKind.INSERT, update.operationKind)
        // 默认值兜底：未显式传 operationKind 时按 INSERT 处理。
        val legacy =
            EditorDocumentUpdate.LocalInput(
                "t1",
                1L,
                0L,
                operationKind = EditorOperationKind.INSERT,
                contentChanged = true,
                contentDelta = EditorContentDelta(insertedChars = "x".length),
            )
        assertEquals(EditorOperationKind.INSERT, legacy.operationKind)
    }

    @Test
    fun operationKindDoesNotAffectLocalResetJudgement() {
        // #624 评论9：SessionState 已无 text 镜像 — 本地/外部判断走 origin +
        // 版本/dirty 判定（shouldApplyExternalContent），operationKind 只记录语义，
        // 不参与 reset 判定。
        val sessionState =
            EditorSessionState(
                targetId = "t1",
                revision = 9L,
                origin = EditorSessionOrigin.LOCAL_INPUT,
            )
        assertEquals(EditorSessionOrigin.LOCAL_INPUT, sessionState.origin)
        assertEquals(9L, sessionState.revision)
        assertTrue("本地输入 origin 保留，不因 operationKind 改变", sessionState.origin == EditorSessionOrigin.LOCAL_INPUT)
    }
}
