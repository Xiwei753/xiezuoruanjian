package com.xiwei.sujian.editor.v2.coordinator

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

/**
 * #595 一/三：reset 原子提交与保存回执按 revision 条件提交契约测试。
 *
 * 问题一：resetPersistentSession 在 Core reset/create 成功后读取 snapshot 失败时，
 * 旧实现返回 ExternalResetResult.Success(TargetSnapshot(text, cursorUtf8, 0L, ...))
 * — 这是 Android 根据输入参数补出来的 revision=0 快照，不是 Rust 返回的真实
 * snapshot。调用方仍当成功，导致 Rust session（新正文）/ SessionStore（旧正文）/
 * ViewModel（新正文+hash）三份状态分裂。
 *
 * 修复：禁止兜底 snapshot — snapshot 读取失败返回 Failed；成功时通过
 * commitResetSnapshot 一次性把真实 snapshot 的 text/revision/selection 写入
 * store 记录与活动 SessionState。
 *
 * 问题三：performSave 保存成功后无条件设 saveStatus=Saved，未确认当前 revision
 * 仍等于保存时的 revision。用户在保存 IO 期间继续输入 B（revision 前进）时，
 * A 保存成功仍把 UI 改为 Saved，页面错误显示"已保存"，B 未落盘。
 *
 * 修复：保存回执按 revision 条件提交 — 只有当前活动 revision 仍等于保存时的
 * revision 才标记 Saved、清 dirty、markSaved。
 */
class ResetSnapshotCommitContractTest {

    private fun coordinatorSource(): String {
        val path = File("src/main/kotlin/com/xiwei/sujian/editor/v2/coordinator/EditorSessionCoordinator.kt")
        return path.readText()
    }

    private fun viewModelSource(): String {
        val path = File("src/main/kotlin/com/xiwei/sujian/ui/EditorViewModel.kt")
        return path.readText()
    }

    @Test
    fun commitResetSnapshot_methodExists() {
        val source = coordinatorSource()
        assertTrue(
            "commitResetSnapshot must exist — atomic snapshot commit replaces fallback revision=0 snapshot (#595 一)",
            source.contains("private fun commitResetSnapshot("),
        )
    }

    @Test
    fun resetPersistentSession_noFallbackSnapshotConstruction() {
        val source = coordinatorSource()
        val resetMethodStart = source.indexOf("fun resetPersistentSession(")
        assertTrue("resetPersistentSession method must exist in source", resetMethodStart >= 0)
        val resetMethodEnd = source.indexOf("private fun commitResetSnapshot", resetMethodStart)
        assertTrue("commitResetSnapshot must follow resetPersistentSession", resetMethodEnd > resetMethodStart)
        val resetBody = source.substring(resetMethodStart, resetMethodEnd)
        assertFalse(
            "resetPersistentSession must not construct fallback TargetSnapshot(text, cursorUtf8, 0L, ...) — " +
            "snapshot read failure must return Failed, not a fabricated revision=0 snapshot (#595 一)",
            resetBody.contains("TargetSnapshot(text, cursorUtf8, 0L"),
        )
    }

    @Test
    fun commitResetSnapshot_writesTextRevisionSelectionToStoreAndState() {
        val source = coordinatorSource()
        val methodStart = source.indexOf("private fun commitResetSnapshot(")
        assertTrue("commitResetSnapshot method must exist", methodStart >= 0)
        val methodEnd = source.indexOf("\n    }", methodStart) + 1
        val methodBody = source.substring(methodStart, methodEnd)
        assertTrue(
            "commitResetSnapshot must write snapshot.text to documentState (#595 一)",
            methodBody.contains("text = snapshot.text"),
        )
        assertTrue(
            "commitResetSnapshot must write snapshot.revision to documentState (#595 一)",
            methodBody.contains("revision = snapshot.revision"),
        )
        assertTrue(
            "commitResetSnapshot must write snapshot.selectionAnchorUtf8 (#595 一)",
            methodBody.contains("selectionAnchorUtf8 = snapshot.selectionAnchorUtf8"),
        )
        assertTrue(
            "commitResetSnapshot must write snapshot.selectionHeadUtf8 (#595 一)",
            methodBody.contains("selectionHeadUtf8 = snapshot.selectionHeadUtf8"),
        )
    }

    @Test
    fun commitResetSnapshot_returnsFailedOnNullSnapshot() {
        val source = coordinatorSource()
        val methodStart = source.indexOf("private fun commitResetSnapshot(")
        assertTrue("commitResetSnapshot method must exist", methodStart >= 0)
        val methodEnd = source.indexOf("\n    }", methodStart) + 1
        val methodBody = source.substring(methodStart, methodEnd)
        assertTrue(
            "commitResetSnapshot must return Failed when snapshot is null — no fallback (#595 一)",
            methodBody.contains("ExternalResetResult.Failed"),
        )
    }

    @Test
    fun performSave_revisionConditionalCommit() {
        val source = viewModelSource()
        val saveMethodStart = source.indexOf("private suspend fun performSave(")
        assertTrue("performSave method must exist", saveMethodStart >= 0)
        val saveMethodEnd = source.indexOf("\n    fun updateChapterNote", saveMethodStart)
        assertTrue("performSave method end must be found", saveMethodEnd > saveMethodStart)
        val saveBody = source.substring(saveMethodStart, saveMethodEnd)
        assertTrue(
            "performSave must check activeRevision == currentRevision before setting Saved — " +
            "saving A while user typed B must not show 'Saved' (#595 三)",
            saveBody.contains("activeRevision == currentRevision"),
        )
        assertTrue(
            "performSave must set Unsaved when revision has advanced (#595 三)",
            saveBody.contains("SaveStatus.Unsaved"),
        )
    }

    @Test
    fun requestSave_validatesTargetIdConsistency() {
        val source = viewModelSource()
        val methodStart = source.indexOf("fun requestSave(): kotlinx.coroutines.Deferred<Boolean>")
        assertTrue("requestSave method must exist", methodStart >= 0)
        val methodEnd = source.indexOf("\n    fun clearChapterContent", methodStart)
        assertTrue("requestSave method end must be found", methodEnd > methodStart)
        val body = source.substring(methodStart, methodEnd)
        assertTrue(
            "requestSave must validate currentSession targetId matches lease targetId — " +
            "章节切换期间 currentSession=B 但 lease=A 时不得把 A 正文保存到 B (#595 二)",
            body.contains("lease.targetId != targetId"),
        )
        assertTrue(
            "requestSave must validate lease is still current via DocumentOperationLease (#595 二)",
            body.contains("isDocumentOperationLeaseCurrent(lease)"),
        )
        assertTrue(
            "requestSave must return false on targetId mismatch — no cross-chapter save (#595 二)",
            body.contains("deferred.complete(false)"),
        )
    }

    @Test
    fun resetPersistentSession_usesCandidateSessionNotInPlaceReset() {
        val source = coordinatorSource()
        val resetMethodStart = source.indexOf("fun resetPersistentSession(")
        assertTrue("resetPersistentSession method must exist", resetMethodStart >= 0)
        val resetMethodEnd = source.indexOf("private fun commitResetSnapshot", resetMethodStart)
        assertTrue("commitResetSnapshot must follow resetPersistentSession", resetMethodEnd > resetMethodStart)
        val resetBody = source.substring(resetMethodStart, resetMethodEnd)
        assertFalse(
            "resetPersistentSession must not call textEditSessionReset — candidate session atomic swap, not in-place reset (#595 一)",
            resetBody.contains("textEditSessionReset("),
        )
    }

    @Test
    fun resetPersistentSession_createsCandidateSessionForAtomicSwap() {
        val source = coordinatorSource()
        val resetMethodStart = source.indexOf("fun resetPersistentSession(")
        val resetMethodEnd = source.indexOf("private fun commitResetSnapshot", resetMethodStart)
        val resetBody = source.substring(resetMethodStart, resetMethodEnd)
        assertTrue(
            "resetPersistentSession must create candidate session for atomic swap — old session preserved on failure (#595 一)",
            resetBody.contains("candidateSessionId"),
        )
    }

    @Test
    fun commitResetSnapshot_acceptsOldSessionIdToClose() {
        val source = coordinatorSource()
        assertTrue(
            "commitResetSnapshot must accept oldSessionIdToClose parameter — close old session after successful candidate commit (#595 一)",
            source.contains("oldSessionIdToClose: ULong?"),
        )
    }

    @Test
    fun commitResetSnapshot_closesCandidateOnSnapshotFailure() {
        val source = coordinatorSource()
        val methodStart = source.indexOf("private fun commitResetSnapshot(")
        assertTrue("commitResetSnapshot method must exist", methodStart >= 0)
        val snapshotCheckEnd = source.indexOf("return ExternalResetResult.Failed", methodStart)
        val guardBody = source.substring(methodStart, snapshotCheckEnd)
        assertTrue(
            "commitResetSnapshot must close candidate session when snapshot read fails — no session leak (#595 一)",
            guardBody.contains("closeSession(sessionId)"),
        )
    }
}
