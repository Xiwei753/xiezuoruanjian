package com.xiwei.sujian.feature.editor.ui

import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.app.WriterAppServiceHolder
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.core.interop.common.ResultEnvelope
import com.xiwei.sujian.feature.editor.session.ChapterContentSavePort
import com.xiwei.sujian.feature.editor.session.EditorSessionCoordinator
import com.xiwei.sujian.feature.editor.session.PreparedSessionHandle
import com.xiwei.sujian.feature.editor.session.TargetSnapshot
import com.xiwei.sujian.feature.editor.session.TextEditorProfile
import com.xiwei.sujian.feature.editor.session.commitPreparedSession
import com.xiwei.sujian.feature.project.data.ChapterRepository
import com.xiwei.sujian.feature.project.data.ProjectRepository
import com.xiwei.sujian.feature.project.data.RecentEditsRepository
import com.xiwei.sujian.feature.project.data.model.ChapterSaveReceipt
import com.xiwei.sujian.feature.settings.data.SettingsRepository
import com.xiwei.sujian.feature.stats.data.WritingStatsRepository
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

/**
 * #624 评论10 第2项：章节关闭（离开正文）与 ViewModel 兜底保存必须从真实
 * Rust snapshot/lease 取 text+revision，不得回退 `_uiState.content`（评论9 后
 * 本地输入不再更新它 — 保存它会用刚打开章节时的旧正文覆盖用户输入）。
 *
 * - [EditorViewModel.saveTargetBeforeClose]：离开正文前保存真实正文；
 *   snapshot 缺失/错版时中止（返回 false），绝不伪造空正文保存；
 * - [EditorViewModel.onCleared]：ViewModel 销毁兜底保存同样只消费 lease 正文。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class EditorViewModelCloseSaveTest {
    private companion object {
        const val TARGET_ID = "chapter-body:p:v:a"

        /** 评论9 后本地输入不更新 _uiState.content — 冷路径正文停留在打开章节时的旧正文。 */
        const val STALE_COLD_PATH_CONTENT = "刚打开章节时的旧正文"
        const val REAL_SNAPSHOT_CONTENT = "用户刚输入的真实正文"
    }

    /** 记录式假保存器 — 立即返回预设回执，捕获正文与调用次数。 */
    private class RecordingSavePort : ChapterContentSavePort {
        var savedContent: String? = null
        var calls = 0
        var nextResult: BridgeResult<ChapterSaveReceipt> =
            BridgeResult.Success(
                ChapterSaveReceipt(
                    chapterRelativePath = "chapters/a.md",
                    contentLen = 0L,
                    contentHash = "hash-A",
                    metaHash = "meta-A",
                    updatedAt = "2026-08-07T00:00:00Z",
                    wordCount = 0,
                ),
            )

        override suspend fun saveChapterContent(
            projectId: String,
            volumeId: String,
            chapterId: String,
            content: String,
        ): BridgeResult<ChapterSaveReceipt> {
            calls++
            savedContent = content
            return nextResult
        }
    }

    /** 可控 snapshot 注入 fake — querySnapshotForSession 由测试安装。 */
    private class FakeSnapshotCoordinator(bridge: AppServiceBridge) : EditorSessionCoordinator(bridge) {
        private val snapshots = mutableMapOf<ULong, TargetSnapshot>()

        fun installSnapshot(
            sessionId: ULong,
            text: String,
            revision: Long,
        ) {
            val cursor = text.toByteArray(Charsets.UTF_8).size
            snapshots[sessionId] = TargetSnapshot(text, cursor, revision, 0, cursor)
        }

        internal override fun querySnapshotForSession(sessionId: ULong): TargetSnapshot? = snapshots[sessionId]
    }

    private lateinit var bridge: AppServiceBridge
    private lateinit var coordinator: FakeSnapshotCoordinator
    private lateinit var vm: EditorViewModel
    private lateinit var savePort: RecordingSavePort

    @Before
    fun setUp() {
        bridge =
            AppServiceBridge(
                WriterAppServiceHolder(
                    "/tmp/sujian_test_workspace_624_close_save",
                    "/tmp/sujian_test_workspace_624_close_save",
                ),
            )
        coordinator = FakeSnapshotCoordinator(bridge)
        val app = RuntimeEnvironment.getApplication()
        val repo = ProjectRepository(app, bridge)
        vm = EditorViewModel(app)
        vm.initialize(
            repo,
            SettingsRepository(app, bridge),
            sessionCoordinator = coordinator,
            chapterRepo = ChapterRepository(app, bridge),
            recentEditsRepo = RecentEditsRepository(app, bridge),
            statsRepo = WritingStatsRepository(bridge.statsBridge),
        )
        savePort = RecordingSavePort()
        vm.chapterSavePort = savePort
    }

    /** 提交活动会话：store 记录 revision 与注入 snapshot 的 revision 一致。 */
    private fun commitActiveSession(
        text: String,
        revision: Long = 2L,
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
                    newlyCreated = true,
                    previousRecord = null,
                ),
            ),
        )
        coordinator.installSnapshot(sessionId, text, revision)
    }

    private fun setCurrentSession() {
        vm.currentSession = EditorSession("s1", "p", "v", "a")
    }

    // ── saveTargetBeforeClose：章节关闭前保存真实 snapshot 正文 ──

    /**
     * 用户输入后（snapshot 已含"真实正文"，_uiState.content 仍是打开的旧正文），
     * 离开正文必须保存 snapshot 正文，绝不能保存 _uiState.content 旧正文。
     */
    @Test
    fun saveTargetBeforeClose_savesRealSnapshotNotColdPathContent() =
        runTest(UnconfinedTestDispatcher()) {
            commitActiveSession(text = REAL_SNAPSHOT_CONTENT, revision = 2L)
            setCurrentSession()
            // 评论9 后本地输入不更新 _uiState.content — 它停留在打开章节时的旧正文。
            vm._uiState.value = vm._uiState.value.copy(content = STALE_COLD_PATH_CONTENT)

            val safe = vm.saveTargetBeforeClose(TARGET_ID, "p", "v", "a")

            assertTrue("拿到真实 snapshot 必须允许关闭", safe)
            assertEquals("必须保存 snapshot 正文而不是 _uiState.content", REAL_SNAPSHOT_CONTENT, savePort.savedContent)
            assertEquals(1, savePort.calls)
        }

    /**
     * snapshot 缺失（querySnapshotForSession 返回 null）时保存/关闭必须中止 —
     * 不伪造空正文保存（否则旧章节被清空）。
     */
    @Test
    fun saveTargetBeforeClose_abortsWhenSnapshotUnavailable() =
        runTest(UnconfinedTestDispatcher()) {
            // 只注册元数据 + 提交 session，但不安装 snapshot → 快照不可得。
            coordinator.registerTargetMeta(TARGET_ID, TextEditorProfile.DocumentBody, persistent = true)
            val handle =
                PreparedSessionHandle(
                    targetId = TARGET_ID,
                    sessionId = 9UL,
                    snapshot = TargetSnapshot("x", 1, 1L, 0, 1),
                    newlyCreated = true,
                    previousRecord = null,
                )
            assertTrue(coordinator.commitPreparedSession(handle))
            setCurrentSession()
            vm._uiState.value = vm._uiState.value.copy(content = "旧正文")

            val safe = vm.saveTargetBeforeClose(TARGET_ID, "p", "v", "a")

            assertFalse("snapshot 不可得时必须中止关闭", safe)
            assertEquals("snapshot 不可得时不得发出任何保存", 0, savePort.calls)
            assertEquals(
                "snapshot 不可得时必须上报 SaveFailed（不得误报已保存）",
                SaveStatus.SaveFailed,
                vm.uiState.value.saveStatus,
            )
        }

    /**
     * snapshot revision 与 store 记录不一致（错版）时保存/关闭必须中止。
     */
    @Test
    fun saveTargetBeforeClose_abortsWhenSnapshotRevisionMismatch() =
        runTest(UnconfinedTestDispatcher()) {
            commitActiveSession(text = "记录正文", revision = 2L)
            // 内核已前进到 revision 3，记录仍为 2 → 错版 snapshot。
            coordinator.installSnapshot(1UL, "内核新正文", revision = 3L)
            setCurrentSession()

            val safe = vm.saveTargetBeforeClose(TARGET_ID, "p", "v", "a")

            assertFalse("错版 snapshot 时必须中止关闭", safe)
            assertEquals("错版 snapshot 时不得发出任何保存", 0, savePort.calls)
        }

    /** 保存失败（磁盘错误）时中止关闭，正文保留在 session 中。 */
    @Test
    fun saveTargetBeforeClose_abortsOnSaveFailure() =
        runTest(UnconfinedTestDispatcher()) {
            commitActiveSession(text = "真实正文", revision = 2L)
            setCurrentSession()
            savePort.nextResult = BridgeResult.Error(ResultEnvelope.errorOf("IO_ERROR", "disk full"))

            val safe = vm.saveTargetBeforeClose(TARGET_ID, "p", "v", "a")

            assertFalse("保存失败必须中止关闭", safe)
            assertEquals(SaveStatus.SaveFailed, vm.uiState.value.saveStatus)
        }

    /** 真实 snapshot 正文为空且未确认清空时：无需保存，可安全关闭。 */
    @Test
    fun saveTargetBeforeClose_safeWhenSnapshotEmptyAndNotCleared() =
        runTest(UnconfinedTestDispatcher()) {
            commitActiveSession(text = "", revision = 1L)
            setCurrentSession()

            val safe = vm.saveTargetBeforeClose(TARGET_ID, "p", "v", "a")

            assertTrue("空正文且未确认清空 — 无需保存即可关闭", safe)
            assertEquals(0, savePort.calls)
        }

    // ── onCleared：ViewModel 兜底保存同样只消费 lease 正文 ──

    /**
     * onCleared 兜底保存必须保存 snapshot 正文；_uiState.content 是冷路径旧正文，
     * 用它保存会把用户刚输入的内容覆盖回磁盘（数据丢失）。
     */
    @Test
    fun onCleared_savesRealSnapshotNotColdPathContent() {
        commitActiveSession(text = REAL_SNAPSHOT_CONTENT, revision = 2L)
        setCurrentSession()
        vm._uiState.value = vm._uiState.value.copy(content = STALE_COLD_PATH_CONTENT)

        vm.saveFallbackOnClear(EditorSession("s1", "p", "v", "a"))

        assertEquals("onCleared 必须保存 snapshot 正文而不是 _uiState.content", REAL_SNAPSHOT_CONTENT, savePort.savedContent)
        assertEquals(1, savePort.calls)
    }

    /** snapshot 不可得时 onCleared 不得用 _uiState.content 兜底保存（不覆盖磁盘）。 */
    @Test
    fun onCleared_skipsSaveWhenSnapshotUnavailable() {
        coordinator.registerTargetMeta(TARGET_ID, TextEditorProfile.DocumentBody, persistent = true)
        val handle =
            PreparedSessionHandle(
                targetId = TARGET_ID,
                sessionId = 9UL,
                snapshot = TargetSnapshot("x", 1, 1L, 0, 1),
                newlyCreated = true,
                previousRecord = null,
            )
        assertTrue(coordinator.commitPreparedSession(handle))
        setCurrentSession()
        vm._uiState.value = vm._uiState.value.copy(content = "冷路径旧正文")

        vm.saveFallbackOnClear(EditorSession("s1", "p", "v", "a"))

        assertNull("snapshot 不可得时不得保存任何正文", savePort.savedContent)
        assertEquals("snapshot 不可得时不得发出任何保存", 0, savePort.calls)
    }
}
