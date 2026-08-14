package com.xiwei.sujian.feature.editor.ui

import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.app.WriterAppServiceHolder
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.feature.editor.platform.EditorEditSource
import com.xiwei.sujian.feature.editor.session.ChapterContentSavePort
import com.xiwei.sujian.feature.editor.session.EditorAppliedEvent
import com.xiwei.sujian.feature.editor.session.EditorContentDelta
import com.xiwei.sujian.feature.editor.session.EditorDocumentUpdate
import com.xiwei.sujian.feature.editor.session.EditorOperationKind
import com.xiwei.sujian.feature.editor.session.EditorSessionCoordinator
import com.xiwei.sujian.feature.editor.session.PreparedSessionHandle
import com.xiwei.sujian.feature.editor.session.TargetSnapshot
import com.xiwei.sujian.feature.editor.session.TextEditorProfile
import com.xiwei.sujian.feature.editor.session.applyLocalEdit
import com.xiwei.sujian.feature.editor.session.commitPreparedSession
import com.xiwei.sujian.feature.editor.session.documentCommittedVersionFor
import com.xiwei.sujian.feature.project.data.ChapterRepository
import com.xiwei.sujian.feature.project.data.ProjectRepository
import com.xiwei.sujian.feature.project.data.RecentEditsRepository
import com.xiwei.sujian.feature.project.data.model.ChapterSaveReceipt
import com.xiwei.sujian.feature.settings.data.SettingsRepository
import com.xiwei.sujian.feature.stats.data.WritingStatsRepository
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.async
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

/**
 * #597（评论五）/ #624 评论12 第2项：真正执行完整保存流程的可控假保存对象测试。
 *
 * 流程：
 * 1. 开始保存正文 A（performSave 经 [ChapterContentSavePort] 调用假保存器，
 *    假保存器挂起直到测试放行；lease 由权威 snapshot 签发）；
 * 2. 保存返回前继续输入正文 B（applyLocalEdit + onEditorApplied 推进 revision）；
 * 3. 让 A 返回保存成功；
 * 4. 检查当前正文仍是 B；
 * 5. 检查页面仍显示未保存（revision 不匹配 → 不标记 Saved、不 markSaved）；
 * 6. 检查 B 没有被 A 的晚到结果覆盖（chapterHash 仍是 B 的）。
 *
 * #624 评论12：dirty 唯一真值在 session store（applyLocalEdit 写入），
 * 不再读写 ViewModel contentDirty；保存完成按 lease + 落盘 hash 统一提交。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class EditorSaveFlowTest {
    private companion object {
        const val TARGET_ID = "chapter-body:p:v:a"

        /** #624 评论1：纯空行正文 — 是用户正文，不是空文档。 */
        const val TWO_NEWLINES = "\n\n"

        /** #624 评论1：只含空格/制表符的段落正文。 */
        const val INDENT_ONLY_BODY = "   \t\n"

        /** 测试正文 A（保存中/保存完成后的一致性检查）。 */
        const val BODY_A = "正文A"

        /** 测试保存回执 hash。 */
        const val HASH_A = "hash-A"
    }

    /** 可控假保存器 — 每次调用挂起在 gate 上，测试放行后返回预设回执。 */
    private class ControllableSavePort : ChapterContentSavePort {
        val gate = CompletableDeferred<Unit>()
        var savedContent: String? = null
        var calls = 0
        var receipt =
            ChapterSaveReceipt(
                chapterRelativePath = "chapters/a.md",
                contentLen = 0L,
                contentHash = HASH_A,
                metaHash = "meta-A",
                updatedAt = "2026-08-07T00:00:00Z",
                wordCount = 0,
            )

        override suspend fun saveChapterContent(
            projectId: String,
            volumeId: String,
            chapterId: String,
            content: String,
        ): BridgeResult<ChapterSaveReceipt> {
            calls++
            savedContent = content
            // 保存 IO 挂起 — 放行前调用方已继续输入 B。
            gate.await()
            return BridgeResult.Success(receipt)
        }
    }

    /** 立即返回成功的假保存端口 — 用于不关心 IO 挂起时序的保存语义测试。 */
    private class ImmediateSavePort : ChapterContentSavePort {
        var savedContent: String? = null

        override suspend fun saveChapterContent(
            projectId: String,
            volumeId: String,
            chapterId: String,
            content: String,
        ): BridgeResult<ChapterSaveReceipt> {
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
    private lateinit var savePort: ControllableSavePort

    @Before
    fun setUp() {
        bridge =
            AppServiceBridge(
                WriterAppServiceHolder(
                    "/tmp/sujian_test_workspace_597_save_flow",
                    "/tmp/sujian_test_workspace_597_save_flow",
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
        savePort = ControllableSavePort()
        vm.chapterSavePort = savePort
    }

    /** 通过 coordinator 提交一个带可控 sessionId 的活动会话，并同步 ViewModel 状态。 */
    private suspend fun commitSession(
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
                    newlyCreated = true,
                    previousRecord = null,
                ),
            ),
        )
        coordinator.installSnapshot(sessionId, text, revision)
        vm.currentSession = EditorSession("s1", "p", "v", "a")
        // #624 评论9：章节加载是冷路径 — uiState.content 经 applyExternalContentToUi 一次性设置。
        // hash 传空串：模拟"从未保存过"的章节（保存成功后由回执写入 hash）。
        vm.applyExternalContentToUi(TARGET_ID, text, "")
    }

    private fun storeLocalDirty(): Boolean = coordinator.store.record(TARGET_ID)?.documentState?.localDirty ?: false

    @Test
    fun saveInFlight_continuedTyping_notOverwrittenByLateReceipt() =
        runTest(UnconfinedTestDispatcher()) {
            commitSession(text = BODY_A, revision = 1L)

            // 1. 开始保存正文 A — lease 从权威 snapshot 签发（revision=1），
            // 假保存器挂起（保存 IO 未返回）。
            val lease = coordinator.issueDocumentOperationLease()!!
            assertEquals(1L, lease.rustRevision)
            val saveJob =
                async(Dispatchers.Default) {
                    vm.performSave(
                        content = BODY_A,
                        session = requireNotNull(vm.currentSession),
                        lease = lease,
                        isAutoSave = false,
                    )
                }
            // 等保存器真正进入挂起（至少一次调用）。
            runCurrentUntil { savePort.calls >= 1 }
            assertTrue("performSave 必须已调用假保存器", savePort.calls >= 1)
            assertEquals("保存中的正文必须是 A", BODY_A, savePort.savedContent)
            assertEquals(SaveStatus.Saving, vm.uiState.value.saveStatus)

            // 2. 保存返回前继续输入正文 B — UI 与会话层 revision 同步前进。
            val inputLease = coordinator.currentInputLease()
            assertTrue("会话提交后必须存在有效输入 lease", inputLease != null)
            coordinator.applyLocalEdit(
                EditorDocumentUpdate.LocalInput(
                    targetId = TARGET_ID,
                    revision = 2L,
                    transactionId = 11L,
                    operationKind = EditorOperationKind.INSERT,
                    contentChanged = true,
                    contentDelta = EditorContentDelta(insertedChars = "正文B".length),
                    lease = inputLease!!,
                ),
            )
            // #624 评论9：热路径不传整章 String — ViewModel 收轻量事件（revision/delta）。
            vm.onEditorApplied(
                EditorAppliedEvent(
                    revision = 2L,
                    transactionId = 11L,
                    operationKind = EditorOperationKind.INSERT,
                    source = EditorEditSource.NORMAL,
                    cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
                    contentChanged = true,
                    contentDelta = EditorContentDelta(insertedChars = "正文B".length),
                    selectionAnchorUtf8 = 3,
                    selectionHeadUtf8 = 3,
                ),
            )
            assertEquals("输入后必须标记未保存", SaveStatus.Unsaved, vm.uiState.value.saveStatus)
            assertTrue("输入后 session store 必须置 dirty", storeLocalDirty())
            assertEquals(2L, coordinator.sessionState.revision)

            // 3. 让 A 返回保存成功（晚到回执）。
            savePort.gate.complete(Unit)
            assertTrue("performSave 必须以成功返回", saveJob.await())

            // 4. 当前正文仍是 B（未 materialize 到 uiState — 保存走 lease/快照）。
            assertEquals("A 的晚到结果不得把 UI 标成 Saved", SaveStatus.Unsaved, vm.uiState.value.saveStatus)
            // 5. 页面仍显示未保存 — revision 已前进，不得标记 Saved。
            assertEquals("保存期间继续输入后不得显示已保存", SaveStatus.Unsaved, vm.uiState.value.saveStatus)
            // 6. B 没有被 A 的晚到结果覆盖 — chapterHash 不得变成 A 的 hash。
            assertEquals("", vm.uiState.value.chapterHash)
            assertTrue("B 必须保持 dirty（未落盘）", storeLocalDirty())
            assertEquals(
                "revision 不匹配时不得 markSaved — committedVersion 不得推进到 A",
                "",
                coordinator.documentCommittedVersionFor(TARGET_ID).contentHash,
            )
        }

    @Test
    fun saveInFlight_noFurtherTyping_marksSavedWithMatchingRevision() =
        runTest(UnconfinedTestDispatcher()) {
            commitSession(text = BODY_A, revision = 1L)
            val lease = coordinator.issueDocumentOperationLease()!!

            val saveJob =
                async(Dispatchers.Default) {
                    vm.performSave(
                        content = BODY_A,
                        session = requireNotNull(vm.currentSession),
                        lease = lease,
                        isAutoSave = false,
                    )
                }
            runCurrentUntil { savePort.calls >= 1 }

            // 保存期间没有新输入 — revision 未前进。
            savePort.gate.complete(Unit)
            assertTrue(saveJob.await())

            // revision 匹配 → 标记 Saved 并记录 hash + markSaved。
            assertEquals(SaveStatus.Saved, vm.uiState.value.saveStatus)
            assertEquals(HASH_A, vm.uiState.value.chapterHash)
            assertFalse("保存成功后 store localDirty 必须清", storeLocalDirty())
            assertEquals(
                "保存成功后 committedVersion 必须推进到落盘 hash",
                HASH_A,
                coordinator.documentCommittedVersionFor(TARGET_ID).contentHash,
            )
        }

    /**
     * #624 评论1："\n"、连续空行、纯空白段落都是用户正文，原样保存 —
     * 不能被 trim 后当成“空正文”拒绝保存（旧逻辑会把它们当作空内容跳过保存，
     * 造成正文从未落盘却显示“保存失败”的假象）。
     */
    @Test
    fun whitespaceOnlyBody_newlinesSavedAsRealContent() =
        runTest(UnconfinedTestDispatcher()) {
            commitSession(text = TWO_NEWLINES, revision = 1L)
            // 立即返回的保存端口 — 不挂起，直接记录保存内容。
            val immediatePort = ImmediateSavePort()
            vm.chapterSavePort = immediatePort

            val ok =
                vm.performSave(
                    TWO_NEWLINES,
                    requireNotNull(vm.currentSession),
                    coordinator.issueDocumentOperationLease()!!,
                    isAutoSave = false,
                )

            assertTrue("空白正文必须真正落盘", ok)
            assertEquals("保存内容必须原样保留 \n\n", TWO_NEWLINES, immediatePort.savedContent)
            assertEquals(SaveStatus.Saved, vm.uiState.value.saveStatus)
        }

    /** #624 评论1：只含缩进/空白的段落也是正文 — 不 trim、不拒绝。 */
    @Test
    fun bodyWithOnlyIndentSpaces_savedAsIs() =
        runTest(UnconfinedTestDispatcher()) {
            commitSession(text = INDENT_ONLY_BODY, revision = 1L)
            val immediatePort = ImmediateSavePort()
            vm.chapterSavePort = immediatePort

            val ok =
                vm.performSave(
                    INDENT_ONLY_BODY,
                    requireNotNull(vm.currentSession),
                    coordinator.issueDocumentOperationLease()!!,
                    isAutoSave = false,
                )

            assertTrue(ok)
            assertEquals("带空格/制表符的段落必须原样保存", INDENT_ONLY_BODY, immediatePort.savedContent)
            assertEquals(SaveStatus.Saved, vm.uiState.value.saveStatus)
        }

    private suspend fun runCurrentUntil(condition: () -> Boolean) {
        var spins = 0
        while (!condition() && spins < 100) {
            kotlinx.coroutines.yield()
            spins++
        }
    }

    /** 记录派发次数的 dispatcher — 断言 Repository 的 main-safe 职责真实生效。 */
    private class RecordingDispatcher : kotlinx.coroutines.CoroutineDispatcher() {
        var dispatchCount = 0

        override fun dispatch(
            context: kotlin.coroutines.CoroutineContext,
            block: Runnable,
        ) {
            dispatchCount++
            block.run()
        }
    }

    /**
     * #624 评论13 第4项：EditorViewModel.calculateWordCount 是 suspend —
     * 整章字数统计必须经注入的 IO dispatcher 派发，不得同步跨 UniFFI 跑在 Main。
     */
    @Test
    fun calculateWordCount_dispatchesThroughRepositoryIoDispatcher() =
        runTest(UnconfinedTestDispatcher()) {
            val dispatcher = RecordingDispatcher()
            val app = RuntimeEnvironment.getApplication()
            val vm = EditorViewModel(app)
            vm.initialize(
                ProjectRepository(app, bridge),
                SettingsRepository(app, bridge),
                sessionCoordinator = coordinator,
                chapterRepo = ChapterRepository(app, bridge, ioDispatcher = dispatcher),
                recentEditsRepo = RecentEditsRepository(app, bridge),
                statsRepo = WritingStatsRepository(bridge.statsBridge, statsWriterScope()),
            )

            val count = vm.calculateWordCount("正文一二三")

            assertTrue(
                "calculateWordCount 必须经注入的 IO dispatcher 派发（main-safe）",
                dispatcher.dispatchCount >= 1,
            )
            assertEquals("正文一二三".length, count)
        }
}

/** #624 评论11 第3项：测试用进程级 stats writer scope（与 SujianAppDependencies 同构）。 */
private fun statsWriterScope(): CoroutineScope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
