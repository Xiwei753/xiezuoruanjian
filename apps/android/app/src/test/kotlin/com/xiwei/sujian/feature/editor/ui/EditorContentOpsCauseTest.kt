package com.xiwei.sujian.feature.editor.ui

import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.app.WriterAppServiceHolder
import com.xiwei.sujian.feature.editor.platform.EditorEditSource
import com.xiwei.sujian.feature.editor.session.EditorAppliedEvent
import com.xiwei.sujian.feature.editor.session.EditorContentDelta
import com.xiwei.sujian.feature.editor.session.EditorDocumentUpdate
import com.xiwei.sujian.feature.editor.session.EditorOperationKind
import com.xiwei.sujian.feature.editor.session.EditorSessionCoordinator
import com.xiwei.sujian.feature.editor.session.PreparedSessionHandle
import com.xiwei.sujian.feature.editor.session.PreparedSessionMode
import com.xiwei.sujian.feature.editor.session.TargetSnapshot
import com.xiwei.sujian.feature.editor.session.TextEditorProfile
import com.xiwei.sujian.feature.editor.session.applyLocalEdit
import com.xiwei.sujian.feature.editor.session.commitPreparedSession
import com.xiwei.sujian.feature.editor.session.writingEventSourceFrom
import com.xiwei.sujian.feature.project.data.ChapterRepository
import com.xiwei.sujian.feature.project.data.ProjectRepository
import com.xiwei.sujian.feature.project.data.RecentEditsRepository
import com.xiwei.sujian.feature.settings.data.SettingsRepository
import com.xiwei.sujian.feature.stats.data.WritingStatsRepository
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import uniffi.writer_core.EditorTransactionCauseDto

/**
 * #624 评论10 第5项：cause 字段 + contentChanged 状态机测试。
 *
 * - selection/cursor-only（contentChanged=false）不进入持久化状态机：
 *   不置 Unsaved、不置 dirty、不 scheduleAutoSave、不改 wordCount、不记统计；
 * - contentChanged=true 才置 Unsaved/dirty/scheduleAutoSave/wordCount/统计；
 * - 统计 source 按 Core cause 明确分类，不靠 source/operationKind 猜；
 * - EditorAppliedEvent 携带 cause 字段。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class EditorContentOpsCauseTest {
    private companion object {
        const val TYPING = "typing"
        const val PASTED = "pasted"
        const val DELETED = "deleted"
        const val UNDO = "undo"
        const val REDO = "redo"
        const val PROGRAMMATIC = "programmatic"
        const val TARGET_ID = "chapter-body:p:v:a"
    }

    private lateinit var bridge: AppServiceBridge
    private lateinit var coordinator: EditorSessionCoordinator
    private lateinit var vm: EditorViewModel

    @Before
    fun setUp() {
        bridge =
            AppServiceBridge(
                WriterAppServiceHolder(
                    "/tmp/sujian_test_workspace_624_cause",
                    "/tmp/sujian_test_workspace_624_cause",
                ),
            )
        coordinator = EditorSessionCoordinator(bridge)
        val app = RuntimeEnvironment.getApplication()
        val repo = ProjectRepository(app, bridge)
        vm = EditorViewModel(app)
        vm.initialize(
            repo,
            SettingsRepository(app, bridge),
            sessionCoordinator = coordinator,
            chapterRepo = ChapterRepository(app, bridge),
            recentEditsRepo = RecentEditsRepository(app, bridge),
            statsRepo = WritingStatsRepository(bridge.statsBridge, statsWriterScope()),
        )
    }

    /** 提交一个活动会话并同步 ViewModel 到已保存状态（saveStatus=Saved, 无 dirty）。 */
    private suspend fun commitSavedSession(
        text: String,
        revision: Long,
        sessionId: ULong = 1UL,
    ) {
        coordinator.registerTargetMeta(TARGET_ID, TextEditorProfile.DocumentBody, persistent = true)
        val cursor = text.toByteArray(Charsets.UTF_8).size
        assertTrue(
            coordinator.commitPreparedSession(
                PreparedSessionHandle(
                    targetId = TARGET_ID,
                    sessionId = sessionId,
                    snapshot = TargetSnapshot(text, cursor, revision, 0, cursor),
                    mode = PreparedSessionMode.Created,
                    previousRecord = null,
                ),
            ),
        )
        vm.currentSession = EditorSession("s1", "p", "v", "a")
        vm.applyExternalContentToUi(TARGET_ID, text, "hash-init")
    }

    /**
     * #624 评论12 第2项：dirty 唯一真值在 session store — 与窗口层一致，
     * 会话层经 applyLocalEdit 按 contentChanged 更新 localDirty。
     */
    private fun driveSessionEdit(
        revision: Long,
        contentChanged: Boolean,
        insertedChars: Int = 0,
    ) {
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                targetId = TARGET_ID,
                revision = revision,
                transactionId = 1L,
                operationKind = if (contentChanged) EditorOperationKind.INSERT else EditorOperationKind.SELECTION,
                lease = coordinator.currentInputLease()!!,
                contentChanged = contentChanged,
                contentDelta =
                    EditorContentDelta(
                        insertedChars = insertedChars,
                        insertedNonWhitespaceChars = if (contentChanged) insertedChars else 0,
                    ),
            ),
        )
    }

    private fun storeLocalDirty(): Boolean = coordinator.store.record(TARGET_ID)?.documentState?.localDirty ?: false

    // ── 5B: onEditorApplied 状态机门控 ──

    /**
     * selection/cursor-only 事件（contentChanged=false）不进入持久化状态机 —
     * saveStatus 不变（不是 Unsaved）、不 scheduleAutoSave、wordCount 不变、不记统计。
     */
    @Test
    fun onEditorApplied_cursorOnly_doesNotSetUnsaved() =
        runTest(UnconfinedTestDispatcher()) {
            commitSavedSession(text = "正文", revision = 1L)
            assertEquals("前置必须是已保存", SaveStatus.Saved, vm.uiState.value.saveStatus)
            assertFalse("前置必须不 dirty", storeLocalDirty())
            val wordCountBefore = vm.uiState.value.wordCount
            assertNull("前置 autoSaveJob 必须为 null", vm.autoSaveJob)
            val statsLastEventMsBefore = vm.statsLastEventMs

            // 会话层：cursor-only（contentChanged=false）不得置 store localDirty。
            driveSessionEdit(revision = 1L, contentChanged = false)
            // UI 层：纯选区移动 — contentChanged=false, cause=Typing（光标移动也用 Typing cause）。
            vm.onEditorApplied(
                EditorAppliedEvent(
                    revision = 1L,
                    transactionId = 1L,
                    operationKind = EditorOperationKind.SELECTION,
                    source = EditorEditSource.NORMAL,
                    cause = EditorTransactionCauseDto.TYPING,
                    contentChanged = false,
                    contentDelta = EditorContentDelta(),
                    selectionAnchorUtf8 = 0,
                    selectionHeadUtf8 = 2,
                ),
            )

            assertEquals(
                "cursor-only 不得置 Unsaved",
                SaveStatus.Saved,
                vm.uiState.value.saveStatus,
            )
            assertFalse("cursor-only 不得置 dirty", storeLocalDirty())
            assertEquals("cursor-only 不得改 wordCount", wordCountBefore, vm.uiState.value.wordCount)
            assertNull("cursor-only 不得 scheduleAutoSave", vm.autoSaveJob)
            assertEquals(
                "cursor-only 不得记统计",
                statsLastEventMsBefore,
                vm.statsLastEventMs,
            )
        }

    /**
     * contentChanged=true 事件进入持久化状态机 —
     * saveStatus=Unsaved、store localDirty=true、scheduleAutoSave 调用、wordCount 更新、统计记录。
     */
    @Test
    fun onEditorApplied_contentChanged_setsUnsaved() =
        runTest(UnconfinedTestDispatcher()) {
            commitSavedSession(text = "正文", revision = 1L)
            val wordCountBefore = vm.uiState.value.wordCount

            // 会话层：真实输入路径置 store localDirty。
            driveSessionEdit(revision = 2L, contentChanged = true, insertedChars = 3)
            vm.onEditorApplied(
                EditorAppliedEvent(
                    revision = 2L,
                    transactionId = 2L,
                    operationKind = EditorOperationKind.INSERT,
                    source = EditorEditSource.NORMAL,
                    cause = EditorTransactionCauseDto.TYPING,
                    contentChanged = true,
                    contentDelta = EditorContentDelta(insertedChars = 3, insertedNonWhitespaceChars = 2),
                    selectionAnchorUtf8 = 5,
                    selectionHeadUtf8 = 5,
                ),
            )

            assertEquals("contentChanged 必须置 Unsaved", SaveStatus.Unsaved, vm.uiState.value.saveStatus)
            assertTrue("contentChanged 必须置 store localDirty", storeLocalDirty())
            assertNotNull("contentChanged 必须 scheduleAutoSave", vm.autoSaveJob)
            assertTrue(
                "contentChanged 必须更新 wordCount",
                vm.uiState.value.wordCount > wordCountBefore,
            )
            assertTrue("contentChanged 必须记统计", vm.statsLastEventMs > 0L)
        }

    // ── 5D: writingEventSourceFrom 按 cause 分类 ──

    /** 统计 source 按 Core cause 明确分类，不靠 source/operationKind 猜。 */
    @Test
    fun writingEventSourceFrom_usesCauseNotGuess() {
        assertEquals(TYPING, writingEventSourceFrom(EditorTransactionCauseDto.TYPING))
        assertEquals(TYPING, writingEventSourceFrom(EditorTransactionCauseDto.TYPING_COMMIT))
        assertEquals(TYPING, writingEventSourceFrom(EditorTransactionCauseDto.IME_COMPOSITION))
        assertEquals(PASTED, writingEventSourceFrom(EditorTransactionCauseDto.PASTE))
        assertEquals(DELETED, writingEventSourceFrom(EditorTransactionCauseDto.DELETE))
        assertEquals(UNDO, writingEventSourceFrom(EditorTransactionCauseDto.UNDO))
        assertEquals(REDO, writingEventSourceFrom(EditorTransactionCauseDto.REDO))
        assertEquals(PROGRAMMATIC, writingEventSourceFrom(EditorTransactionCauseDto.PROGRAMMATIC))
        assertEquals(PROGRAMMATIC, writingEventSourceFrom(EditorTransactionCauseDto.LOAD))
        assertEquals(PROGRAMMATIC, writingEventSourceFrom(EditorTransactionCauseDto.FORMAT))
    }

    // ── 5A/5C: EditorAppliedEvent 携带 cause ──

    /** EditorAppliedEvent 构造时 cause 字段正确填充并可读。 */
    @Test
    fun editorAppliedEvent_carriesCause() {
        val event =
            EditorAppliedEvent(
                revision = 10L,
                transactionId = 99L,
                operationKind = EditorOperationKind.REPLACE,
                source = EditorEditSource.PROGRAMMATIC,
                cause = EditorTransactionCauseDto.PASTE,
                contentChanged = true,
                contentDelta = EditorContentDelta(insertedChars = 5),
            )
        assertEquals(EditorTransactionCauseDto.PASTE, event.cause)

        val undoEvent =
            EditorAppliedEvent(
                revision = 11L,
                transactionId = 100L,
                operationKind = EditorOperationKind.DELETE,
                source = EditorEditSource.UNDO,
                cause = EditorTransactionCauseDto.UNDO,
                contentChanged = true,
                contentDelta = EditorContentDelta(deletedChars = 2),
            )
        assertEquals(EditorTransactionCauseDto.UNDO, undoEvent.cause)
    }
}

/** #624 评论11 第3项：测试用进程级 stats writer scope（与 SujianAppDependencies 同构）。 */
private fun statsWriterScope(): CoroutineScope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
