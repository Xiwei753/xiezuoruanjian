package com.xiwei.sujian.feature.editor.presentation

import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.app.WriterAppServiceHolder
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.feature.editor.session.ChapterContentSavePort
import com.xiwei.sujian.feature.editor.session.EditorContentDelta
import com.xiwei.sujian.feature.editor.session.EditorDocumentUpdate
import com.xiwei.sujian.feature.editor.session.EditorSessionCoordinator
import com.xiwei.sujian.feature.editor.session.PreparedSessionHandle
import com.xiwei.sujian.feature.editor.session.PreparedSessionMode
import com.xiwei.sujian.feature.editor.session.TargetSnapshot
import com.xiwei.sujian.feature.editor.session.TextEditorProfile
import com.xiwei.sujian.feature.editor.session.activateAttachedForTest
import com.xiwei.sujian.feature.editor.session.applyLocalEdit
import com.xiwei.sujian.feature.editor.session.commitPreparedSession
import com.xiwei.sujian.feature.project.data.ChapterRepository
import com.xiwei.sujian.feature.project.data.ProjectRepository
import com.xiwei.sujian.feature.project.data.RecentEditsRepository
import com.xiwei.sujian.feature.project.data.model.ChapterSaveReceipt
import com.xiwei.sujian.feature.settings.data.SettingsRepository
import com.xiwei.sujian.feature.stats.data.WritingStatsRepository
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.Description
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

/**
 * #624 评论12 第2项：保存/切章决策只消费权威 lease（`localDirty + text`），
 * 删除 ViewModel 第二份 contentDirty — dirty 唯一真值在 session store
 * （applyLocalEdit 写入，issueDocumentOperationLease 从记录填入 lease）。
 *
 * 关键回归：用户把正文删到 "" 后（store localDirty=true，snapshot 正文已确定为空），
 * 在 autosave 跑完之前立刻 requestSave/切章，旧实现走
 * `contentExplicitlyCleared == false → else true`，磁盘旧正文不会被清掉。
 * 新实现 `!lease.localDirty → NoOp；dirty+空正文 → Clear；dirty+非空 → Save`。
 *
 * 测试环境无 native 库：真实 bridge 的 clearChapterContent 返回 NotLoaded →
 * clearChapterContentInternal/clearChapterContentForSwitch 报 SaveFailed。
 * 「SaveFailed」正是 Clear 分支被真实尝试的可观测证据（旧实现不会尝试 Clear，
 * saveStatus 保持 Unsaved）。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class EditorSaveClearDecisionTest {
    private companion object {
        const val TARGET_ID = "chapter-body:p:v:a"
    }

    class MainDispatcherRule(
        val dispatcher: kotlinx.coroutines.test.TestDispatcher = UnconfinedTestDispatcher(),
    ) : org.junit.rules.TestWatcher() {
        override fun starting(description: Description) {
            Dispatchers.setMain(dispatcher)
        }

        override fun finished(description: Description) {
            Dispatchers.resetMain()
        }
    }

    @get:Rule
    val mainDispatcherRule = MainDispatcherRule()

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

    private class RecordingSavePort : ChapterContentSavePort {
        var savedContent: String? = null
        var calls = 0

        override suspend fun saveChapterContent(
            projectId: String,
            volumeId: String,
            chapterId: String,
            content: String,
        ): BridgeResult<ChapterSaveReceipt> {
            calls++
            savedContent = content
            return BridgeResult.Success(
                ChapterSaveReceipt(
                    chapterRelativePath = "chapters/a.md",
                    contentLen = content.toByteArray(Charsets.UTF_8).size.toLong(),
                    contentHash = "hash-$content",
                    metaHash = "meta-A",
                    updatedAt = "2026-08-07T00:00:00Z",
                    wordCount = 0,
                ),
            )
        }
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
                    "/tmp/sujian_test_workspace_624_save_clear_decision",
                    "/tmp/sujian_test_workspace_624_save_clear_decision",
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
            statsRepo = WritingStatsRepository(bridge.statsBridge, CoroutineScope(SupervisorJob() + Dispatchers.IO)),
        )
        savePort = RecordingSavePort()
        vm.chapterSavePort = savePort
    }

    /** 提交活动会话：store 记录 revision 与注入 snapshot 的 revision 一致。 */
    private fun commitActiveSession(
        text: String,
        revision: Long = 1L,
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
        coordinator.activateAttachedForTest(TARGET_ID)
        coordinator.installSnapshot(sessionId, text, revision)
        vm.currentSession = EditorSession("s1", "p", "v", "a")
    }

    /**
     * #624 评论12 第2项：dirty 唯一真值在 session store — 通过真实输入路径
     * （applyLocalEdit contentChanged=true）置位，不再读写 ViewModel contentDirty。
     */
    private fun markDirty() {
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                targetId = TARGET_ID,
                revision = coordinator.sessionState.revision,
                transactionId = 1L,
                lease = coordinator.currentInputLease()!!,
                contentChanged = true,
                contentDelta = EditorContentDelta(insertedChars = 1),
            ),
        )
    }

    /**
     * 等真实 IO save actor 把 saveStatus 置成 SaveFailed。
     * 轮询必须同时：yield() 推进测试调度器（跑 Main 上的 autosave 协程）+ 真实
     * sleep 让 IO 线程获得 CPU（actor/FFI 调用是真实线程，不受虚拟时间控制）。
     */
    private suspend fun awaitSaveFailed() {
        var spins = 0
        while (vm.uiState.value.saveStatus != SaveStatus.SaveFailed && spins < 2000) {
            kotlinx.coroutines.yield()
            Thread.sleep(2)
            spins++
        }
    }

    // ── requestSave：手动保存统一决策 ──

    /**
     * 用户把正文删空（dirty=true、snapshot 正文 ""）后立刻手动保存：
     * 必须真实尝试 Clear — 旧实现 `contentExplicitlyCleared=false → return false`
     * 只报失败不动磁盘，正文永远不会被清掉。
     * 无 native 环境下 Clear 尝试以 NotLoaded 失败 → SaveFailed（尝试的可观测证据）。
     */
    @Test
    fun requestSave_dirtyEmptyContent_attemptsClear_notFakeSuccess() =
        runTest {
            commitActiveSession(text = "", revision = 1L)
            markDirty()
            vm._uiState.value = vm._uiState.value.copy(saveStatus = SaveStatus.Unsaved)
            vm.startSaveActor()

            val ok = vm.requestSave().await()

            assertFalse("dirty+空正文的保存必须失败（Clear 未落盘）", ok)
            assertEquals(
                "#624 评论11 第2项：dirty+空正文必须真实尝试 Clear — " +
                    "不得停留在 Unsaved 假失败（旧实现不尝试 Clear）",
                SaveStatus.SaveFailed,
                vm.uiState.value.saveStatus,
            )
        }

    /**
     * 未编辑（!lease.localDirty）时保存无事可做 — 直接成功，不发任何命令
     * （旧实现发 Flush 且无回执 → 假失败）。
     */
    @Test
    fun requestSave_notDirty_noOp_returnsTrue() =
        runTest {
            commitActiveSession(text = "", revision = 1L)
            vm._uiState.value = vm._uiState.value.copy(saveStatus = SaveStatus.Idle)
            vm.startSaveActor()

            val ok = vm.requestSave().await()

            assertTrue("未 dirty 时保存无事可做 — 必须直接成功", ok)
            assertEquals("未 dirty 时不得触碰 saveStatus", SaveStatus.Idle, vm.uiState.value.saveStatus)
            assertEquals("未 dirty 时不得发出保存", 0, savePort.calls)
        }

    /** dirty + 非空正文 → Save（正文经 save port 落盘）。 */
    @Test
    fun requestSave_dirtyNonEmptyContent_savesThroughPort() =
        runTest {
            commitActiveSession(text = "真实正文", revision = 1L)
            markDirty()
            vm._uiState.value = vm._uiState.value.copy(saveStatus = SaveStatus.Unsaved)
            vm.startSaveActor()

            val ok = vm.requestSave().await()

            assertTrue("dirty+非空正文必须保存成功", ok)
            assertEquals("真实正文", savePort.savedContent)
            assertEquals(SaveStatus.Saved, vm.uiState.value.saveStatus)
        }

    /** snapshot 不可得（lease null）→ 返回失败，绝不回退冷路径正文、绝不 Clear。 */
    @Test
    fun requestSave_snapshotUnavailable_returnsFalseNoClear() =
        runTest {
            // 只提交 session，不安装 snapshot → lease 不可签发。
            coordinator.registerTargetMeta(TARGET_ID, TextEditorProfile.DocumentBody, persistent = true)
            val handle =
                PreparedSessionHandle(
                    targetId = TARGET_ID,
                    sessionId = 9UL,
                    snapshot = TargetSnapshot("x", 1, 1L, 0, 1),
                    mode = PreparedSessionMode.Created,
                    previousRecord = null,
                )
            assertTrue(coordinator.commitPreparedSession(handle))
            coordinator.activateAttachedForTest(TARGET_ID)
            vm.currentSession = EditorSession("s1", "p", "v", "a")
            markDirty()
            vm._uiState.value = vm._uiState.value.copy(saveStatus = SaveStatus.Unsaved, content = "冷路径旧正文")
            vm.startSaveActor()

            val ok = vm.requestSave().await()

            assertFalse("snapshot 不可得时保存必须失败", ok)
            assertEquals("snapshot 不可得时不得发出保存", 0, savePort.calls)
            assertEquals("snapshot 不可得时不得误报 Saved", SaveStatus.Unsaved, vm.uiState.value.saveStatus)
        }

    // ── scheduleAutoSave：自动保存统一决策 ──

    /**
     * autosave 到点后同样按 lease.localDirty + lease.text 决策：dirty+空 → Clear 尝试
     * （无 native → SaveFailed）。
     */
    @Test
    fun autoSave_dirtyEmptyContent_attemptsClear() =
        runTest {
            commitActiveSession(text = "", revision = 1L)
            markDirty()
            vm._uiState.value =
                vm._uiState.value.copy(
                    saveStatus = SaveStatus.Unsaved,
                    settings = EditorSettingsState(autoSaveDelayMs = 0, autoSaveEnabled = true),
                )
            vm.startSaveActor()

            vm.scheduleAutoSave()
            awaitSaveFailed()

            assertEquals(
                "#624 评论11 第2项：autosave 对 dirty+空正文必须尝试 Clear",
                SaveStatus.SaveFailed,
                vm.uiState.value.saveStatus,
            )
        }

    // ── clearChapterContent：显式清空直接发命令 ──

    /** 显式清空不再写布尔侧信道 — 直接发 Clear 命令（无 native → SaveFailed 可观测）。 */
    @Test
    fun clearChapterContent_sendsClearCommandDirectly() =
        runTest {
            commitActiveSession(text = "旧正文", revision = 1L)
            vm._uiState.value = vm._uiState.value.copy(saveStatus = SaveStatus.Idle)
            vm.startSaveActor()

            vm.clearChapterContent()
            awaitSaveFailed()

            assertEquals("显式清空必须真实尝试 Clear", SaveStatus.SaveFailed, vm.uiState.value.saveStatus)
        }

    /**
     * #624 评论10 第1项：autosave 到点后 snapshot 不可得（lease null）—
     * 不回退 "" 伪造空正文、不回退 _uiState.content、不发 Save/Clear 命令，
     * 保持 Unsaved（绝不误触发 Clear）。
     */
    @Test
    fun autoSave_snapshotUnavailable_keepsUnsavedNoCommand() =
        runTest {
            // 只提交 session，不安装 snapshot → lease 不可签发。
            coordinator.registerTargetMeta(TARGET_ID, TextEditorProfile.DocumentBody, persistent = true)
            val handle =
                PreparedSessionHandle(
                    targetId = TARGET_ID,
                    sessionId = 9UL,
                    snapshot = TargetSnapshot("x", 1, 1L, 0, 1),
                    mode = PreparedSessionMode.Created,
                    previousRecord = null,
                )
            assertTrue(coordinator.commitPreparedSession(handle))
            coordinator.activateAttachedForTest(TARGET_ID)
            vm.currentSession = EditorSession("s1", "p", "v", "a")
            markDirty()
            vm._uiState.value =
                vm._uiState.value.copy(
                    saveStatus = SaveStatus.Unsaved,
                    content = "冷路径旧正文",
                    settings = EditorSettingsState(autoSaveDelayMs = 0, autoSaveEnabled = true),
                )
            vm.startSaveActor()

            vm.scheduleAutoSave()
            // 等 debounce(0) + actor 处理窗口结束 — 若错误触发命令会落到 SaveFailed。
            var spins = 0
            while (spins < 500) {
                kotlinx.coroutines.yield()
                Thread.sleep(2)
                spins++
            }

            assertEquals(
                "snapshot 不可得时 autosave 必须保持 Unsaved，不得误触发 Clear",
                SaveStatus.Unsaved,
                vm.uiState.value.saveStatus,
            )
            assertEquals("snapshot 不可得时不得发出任何保存命令", 0, savePort.calls)
        }
}
