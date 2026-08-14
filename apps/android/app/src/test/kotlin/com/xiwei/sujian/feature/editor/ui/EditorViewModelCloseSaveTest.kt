package com.xiwei.sujian.feature.editor.ui

import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.app.WriterAppServiceHolder
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.core.interop.common.ResultEnvelope
import com.xiwei.sujian.feature.editor.session.ChapterContentSavePort
import com.xiwei.sujian.feature.editor.session.EditorContentDelta
import com.xiwei.sujian.feature.editor.session.EditorDocumentUpdate
import com.xiwei.sujian.feature.editor.session.EditorSessionCoordinator
import com.xiwei.sujian.feature.editor.session.PreparedSessionHandle
import com.xiwei.sujian.feature.editor.session.TargetSnapshot
import com.xiwei.sujian.feature.editor.session.TextEditorProfile
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
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TestWatcher
import org.junit.runner.Description
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

/**
 * #624 评论12 第1/2/3项：离开正文（workspace 返回）的保存、业务关闭与
 * 生命周期收口测试。
 *
 * - 离开正文的保存由 workspace 返回事务（guardedBack → ActiveDocumentGate
 *   flush → requestSave）完成：正文必须从真实 Rust snapshot/lease 取，
 *   不得回退 `_uiState.content`（评论9 后本地输入不再更新它）；
 *   snapshot 缺失/错版时保存必须中止（返回失败），绝不伪造空正文；
 * - [EditorViewModel.finishWorkspaceClose]：导航成功离开后清空 currentSession，
 *   避免「Rust session 已关闭，ViewModel 仍宣称 A 是当前章节」；
 * - onCleared 不再同步阻塞保存（评论12 第3项）— 正文安全由统一
 *   autosave / ActiveDocumentGate / workspace 离开事务负责。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class EditorViewModelCloseSaveTest {
    class MainDispatcherRule(
        val dispatcher: kotlinx.coroutines.test.TestDispatcher = UnconfinedTestDispatcher(),
    ) : TestWatcher() {
        override fun starting(description: Description) {
            Dispatchers.setMain(dispatcher)
        }

        override fun finished(description: Description) {
            Dispatchers.resetMain()
        }
    }

    @get:Rule
    val mainDispatcherRule = MainDispatcherRule()

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
            statsRepo = WritingStatsRepository(bridge.statsBridge, statsWriterScope()),
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

    /** 真实输入路径：applyLocalEdit(contentChanged=true) 置 session store localDirty。 */
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

    // ── requestSave：workspace 离开事务（guardedBack → ActiveDocumentGate flush）的保存入口 ──

    /**
     * 用户输入后（snapshot 已含"真实正文"，_uiState.content 仍是打开的旧正文），
     * 离开正文必须保存 snapshot 正文，绝不能保存 _uiState.content 旧正文。
     */
    @Test
    fun requestSave_savesRealSnapshotNotColdPathContent() =
        runTest(UnconfinedTestDispatcher()) {
            commitActiveSession(text = REAL_SNAPSHOT_CONTENT, revision = 2L)
            setCurrentSession()
            // 评论9 后本地输入不更新 _uiState.content — 它停留在打开章节时的旧正文。
            vm._uiState.value = vm._uiState.value.copy(content = STALE_COLD_PATH_CONTENT)
            markDirty()
            vm.startSaveActor()

            val ok = vm.requestSave().await()

            assertTrue("拿到真实 snapshot 必须保存成功", ok)
            assertEquals("必须保存 snapshot 正文而不是 _uiState.content", REAL_SNAPSHOT_CONTENT, savePort.savedContent)
            assertEquals(1, savePort.calls)
        }

    /**
     * snapshot 缺失（querySnapshotForSession 返回 null）时保存必须中止 —
     * 不伪造空正文保存（否则旧章节被清空）。
     */
    @Test
    fun requestSave_abortsWhenSnapshotUnavailable() =
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
            vm.startSaveActor()

            val ok = vm.requestSave().await()

            assertFalse("snapshot 不可得时必须中止保存", ok)
            assertEquals("snapshot 不可得时不得发出任何保存", 0, savePort.calls)
            assertEquals(
                "snapshot 不可得时不得误报 Saved",
                SaveStatus.Idle,
                vm.uiState.value.saveStatus,
            )
        }

    /**
     * snapshot revision 与 store 记录不一致（错版）时保存必须中止。
     */
    @Test
    fun requestSave_abortsWhenSnapshotRevisionMismatch() =
        runTest(UnconfinedTestDispatcher()) {
            commitActiveSession(text = "记录正文", revision = 2L)
            // 内核已前进到 revision 3，记录仍为 2 → 错版 snapshot。
            coordinator.installSnapshot(1UL, "内核新正文", revision = 3L)
            setCurrentSession()
            vm.startSaveActor()

            val ok = vm.requestSave().await()

            assertFalse("错版 snapshot 时必须中止保存", ok)
            assertEquals("错版 snapshot 时不得发出任何保存", 0, savePort.calls)
        }

    /** 保存失败（磁盘错误）时离开事务失败 — 正文保留在 session 中。 */
    @Test
    fun requestSave_abortsOnSaveFailure() =
        runTest(UnconfinedTestDispatcher()) {
            commitActiveSession(text = "真实正文", revision = 2L)
            setCurrentSession()
            markDirty()
            savePort.nextResult = BridgeResult.Error(ResultEnvelope.errorOf("IO_ERROR", "disk full"))
            vm.startSaveActor()

            val ok = vm.requestSave().await()

            assertFalse("保存失败必须中止离开事务", ok)
            assertEquals(SaveStatus.SaveFailed, vm.uiState.value.saveStatus)
        }

    /** 真实 snapshot 正文为空且未编辑时：无需保存，可安全离开。 */
    @Test
    fun requestSave_safeWhenEmptyAndNotDirty() =
        runTest(UnconfinedTestDispatcher()) {
            commitActiveSession(text = "", revision = 1L)
            setCurrentSession()
            vm.startSaveActor()

            val ok = vm.requestSave().await()

            assertTrue("空正文且未编辑 — 无需保存即可离开", ok)
            assertEquals(0, savePort.calls)
        }

    // ── finishWorkspaceClose：workspace 离开正文后的业务关闭收口 ──

    /**
     * 导航成功离开正文后必须清空 currentSession — 否则「Rust session 已关闭，
     * ViewModel 仍宣称 A 是当前章节」：再点 A 会命中"相同章节 no-op"跳过重新
     * 加载；点 B 会把已关闭的 A 当 oldSession 去拿活动 lease（拿不到 → SaveFailed）。
     */
    @Test
    fun finishWorkspaceClose_clearsCurrentSessionAndUiState() {
        commitActiveSession(text = "正文", revision = 1L)
        setCurrentSession()
        vm._uiState.value = vm._uiState.value.copy(content = "正文", saveStatus = SaveStatus.Unsaved)
        vm.autoSaveJob = vm.editorScope.launch { delay(100_000) }

        vm.finishWorkspaceClose(TARGET_ID)

        assertNull("离开正文后 currentSession 必须清空", vm.currentSession)
        assertEquals(
            "UI 冷状态必须复位（保留设置字段）",
            EditorUiState(settings = vm.uiState.value.settings),
            vm.uiState.value,
        )
        assertTrue("离开后 autosave job 必须取消", vm.autoSaveJob?.isCancelled == true)
    }

    /** 不匹配的 target 不得清掉当前会话（防串章）。 */
    @Test
    fun finishWorkspaceClose_noopForDifferentTarget() {
        commitActiveSession(text = "正文", revision = 1L)
        setCurrentSession()

        vm.finishWorkspaceClose("chapter-body:p:v:other")

        assertNotNull("不匹配的 target 不得清掉当前会话", vm.currentSession)
    }

    /** 无当前会话时是 no-op。 */
    @Test
    fun finishWorkspaceClose_noopWithoutCurrentSession() {
        commitActiveSession(text = "正文", revision = 1L)

        vm.finishWorkspaceClose(TARGET_ID)

        assertNull(vm.currentSession)
    }

    /** onCleared 是 protected 生命周期回调 — 测试经反射调用（不改变产品可见性）。 */
    private fun invokeOnCleared(vm: EditorViewModel) {
        val method = EditorViewModel::class.java.getDeclaredMethod("onCleared")
        method.isAccessible = true
        method.invoke(vm)
    }

    // ── onCleared：生命周期收尾不再同步阻塞保存 ──

    /**
     * #624 评论12 第3项：onCleared 不是持久化边界 — 不再 runBlocking 阻塞主线程
     * 做文件保存；正文安全由统一 autosave / ActiveDocumentGate / workspace 离开
     * 事务负责，生命周期收尾只取消 Job、关闭注册。
     */
    @Test
    fun onCleared_doesNotBlockOnSave() {
        commitActiveSession(text = REAL_SNAPSHOT_CONTENT, revision = 2L)
        setCurrentSession()
        vm._uiState.value = vm._uiState.value.copy(content = STALE_COLD_PATH_CONTENT)
        markDirty()

        invokeOnCleared(vm)

        assertEquals(
            "onCleared 不得同步阻塞保存（评论12 第3项）",
            0,
            savePort.calls,
        )
    }
}

/** #624 评论11 第3项：测试用进程级 stats writer scope（与 SujianAppDependencies 同构）。 */
private fun statsWriterScope(): CoroutineScope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
