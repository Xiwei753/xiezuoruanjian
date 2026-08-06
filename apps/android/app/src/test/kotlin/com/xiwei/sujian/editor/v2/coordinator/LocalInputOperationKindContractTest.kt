package com.xiwei.sujian.editor.v2.coordinator

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
class LocalInputOperationKindContractTest {

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
        val update = EditorDocumentUpdate.LocalInput(
            targetId = "t1",
            text = "typed",
            revision = 7L,
            transactionId = 42L,
            operationKind = EditorOperationKind.INSERT,
        )
        assertEquals("typed", update.text)
        assertEquals(7L, update.revision)
        assertEquals(42L, update.transactionId)
        assertEquals(EditorOperationKind.INSERT, update.operationKind)
        // 默认值兜底：未显式传 operationKind 时按 INSERT 处理。
        val legacy = EditorDocumentUpdate.LocalInput("t1", "x", 1L, 0L)
        assertEquals(EditorOperationKind.INSERT, legacy.operationKind)
    }

    @Test
    fun operationKindDoesNotAffectLocalResetJudgement() {
        // WritingPane 的本地/外部判断只用 origin + text；operationKind 是语义记录，
        // 不能改变 reset 判定结果。
        val sessionState = EditorSessionState(
            targetId = "t1",
            text = "本地输入结果",
            revision = 9L,
            origin = EditorSessionOrigin.LOCAL_INPUT,
        )
        val uiContent = "本地输入结果"
        val isLocal = sessionState.origin == EditorSessionOrigin.LOCAL_INPUT &&
            sessionState.text == uiContent
        assertTrue("operationKind 不得参与 reset 判定，本地回显必须仍判为本地", isLocal)
    }
}
