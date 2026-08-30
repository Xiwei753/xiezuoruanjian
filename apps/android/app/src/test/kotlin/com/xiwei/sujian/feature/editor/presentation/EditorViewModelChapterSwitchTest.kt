package com.xiwei.sujian.feature.editor.presentation

import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.app.WriterAppServiceHolder
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.feature.editor.session.EditorAppliedEvent
import com.xiwei.sujian.feature.editor.session.EditorContentDelta
import com.xiwei.sujian.feature.editor.session.EditorDocumentUpdate
import com.xiwei.sujian.feature.editor.session.EditorEditSource
import com.xiwei.sujian.feature.editor.session.EditorOperationKind
import com.xiwei.sujian.feature.editor.session.EditorSessionCoordinator
import com.xiwei.sujian.feature.editor.session.PreparedSessionHandle
import com.xiwei.sujian.feature.editor.session.PreparedSessionMode
import com.xiwei.sujian.feature.editor.session.TargetSnapshot
import com.xiwei.sujian.feature.editor.session.TextEditorProfile
import com.xiwei.sujian.feature.editor.session.activateAttachedForTest
import com.xiwei.sujian.feature.editor.session.applyLocalEdit
import com.xiwei.sujian.feature.editor.session.commitPreparedSession
import com.xiwei.sujian.feature.editor.session.documentCommittedVersionFor
import com.xiwei.sujian.feature.project.data.ChapterRepository
import com.xiwei.sujian.feature.project.data.ProjectRepository
import com.xiwei.sujian.feature.project.data.RecentEditsRepository
import com.xiwei.sujian.feature.project.data.model.ChapterSaveReceipt
import com.xiwei.sujian.feature.settings.data.SettingsRepository
import com.xiwei.sujian.feature.stats.data.WritingStatsRepository
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.async
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.TestDispatcher
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TestWatcher
import org.junit.runner.Description
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

/**
 * #595 一：章节切换事务契约测试。
 *
 * switchChapter 返回 [ChapterSwitchResult]：
 * - 保存旧章节 → 加载新章节 → 成功后一次性提交（Success）；
 * - 保存失败返回 SaveFailed，currentSession/标题保持旧章节（导航必须回滚）；
 * - 加载失败返回 LoadFailed 并回退 currentSession/标题（防止回滚后把旧正文写入新章节）。
 *
 * 测试环境无 native 库：所有 Bridge 调用返回 NotLoaded（wrapResult 捕获
 * UnsatisfiedLinkError），因此保存必然失败、加载必然失败 — 正好覆盖两条失败路径。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class EditorViewModelChapterSwitchTest {
    class MainDispatcherRule(
        val dispatcher: TestDispatcher = UnconfinedTestDispatcher(),
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

    // #624 评论10 第2项：fake coordinator 注入可控 snapshot
    private class FakeSessionCoordinator(bridge: AppServiceBridge) : EditorSessionCoordinator(bridge) {
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

    private lateinit var fakeCoordinator: FakeSessionCoordinator

    private fun createVm(): EditorViewModel {
        val app = RuntimeEnvironment.getApplication()
        val bridge =
            AppServiceBridge(WriterAppServiceHolder("/tmp/sujian_test_workspace_595", "/tmp/sujian_test_workspace_595"))
        fakeCoordinator = FakeSessionCoordinator(bridge)
        val repo = ProjectRepository(app, bridge)
        val vm = EditorViewModel(app)
        vm.initialize(
            repo,
            SettingsRepository(app, bridge),
            sessionCoordinator = fakeCoordinator,
            chapterRepo = ChapterRepository(app, bridge),
            recentEditsRepo = RecentEditsRepository(app, bridge),
            statsRepo = WritingStatsRepository(bridge.statsBridge, statsWriterScope()),
        )
        return vm
    }

    @Suppress("LongParameterList")
    private fun commitActiveSession(
        vm: EditorViewModel,
        projectId: String,
        volumeId: String,
        chapterId: String,
        text: String,
        revision: Long = 1L,
        sessionId: ULong = 1UL,
    ) {
        val targetId = vm.chapterTargetId(projectId, volumeId, chapterId)
        fakeCoordinator.registerTargetMeta(targetId, TextEditorProfile.DocumentBody, persistent = true)
        val cursor = text.toByteArray(Charsets.UTF_8).size
        assertTrue(
            fakeCoordinator.commitPreparedSession(
                PreparedSessionHandle(
                    targetId = targetId,
                    sessionId = sessionId,
                    snapshot = TargetSnapshot(text, cursor, revision, 0, cursor),
                    mode = PreparedSessionMode.Created,
                    previousRecord = null,
                ),
            ),
        )
        fakeCoordinator.activateAttachedForTest(targetId)
        fakeCoordinator.installSnapshot(sessionId, text, revision)
    }

    /**
     * #624 评论12 第2项：dirty 唯一真值在 session store — 通过真实输入路径
     * （applyLocalEdit contentChanged=true）置位，不再读写 ViewModel contentDirty。
     */
    private fun markLocalDirty(
        vm: EditorViewModel,
        projectId: String,
        volumeId: String,
        chapterId: String,
    ) {
        fakeCoordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                targetId = vm.chapterTargetId(projectId, volumeId, chapterId),
                revision = fakeCoordinator.sessionState.revision,
                transactionId = 1L,
                lease = fakeCoordinator.currentInputLease()!!,
                contentChanged = true,
                contentDelta = EditorContentDelta(insertedChars = 1),
            ),
        )
    }

    /**
     * 等 initChapter 加载落定（loading=false）。
     * 非 suspend 函数 — detekt SleepInsteadOfDelay 只检测 suspend 函数内的 Thread.sleep；
     * 真实 sleep 让真实 IO 线程获得 CPU，是确定性同步机制，不是协程等待。
     */
    private fun awaitLoadingSettled(vm: EditorViewModel) {
        var attempts = 0
        while (vm.uiState.value.loading && attempts < 200) {
            Thread.sleep(5)
            attempts++
        }
    }

    @Test
    fun saveFailure_returnsSaveFailedAndKeepsCurrentChapter() =
        runTest {
            val vm = createVm()
            // 先有正文内容（#624 评论9：content 只走冷路径，测试直接设 uiState）
            vm._uiState.value = vm._uiState.value.copy(content = "已有章节内容")
            assertEquals("已有章节内容", vm.uiState.value.content)

            // 进入章节 A（initChapter 同步置 loading=true）
            vm.enterChapterForTest("p", "v", "a", "A")

            // 切换到章节 B：oldSession=A → 保存 A 的正文 → 无 native → 保存失败
            val result = vm.switchChapter("p", "v", "b", "B")

            assertTrue(
                "保存失败必须返回 ChapterSwitchResult.SaveFailed（不能只把 loading 改回 false）",
                result is ChapterSwitchResult.SaveFailed,
            )
            assertFalse(
                "章节切换保存失败必须恢复 loading=false（否则编辑器永久卡在加载态）",
                vm.uiState.value.loading,
            )
            assertEquals(
                "保存失败必须上报 SaveFailed",
                SaveStatus.SaveFailed,
                vm.uiState.value.saveStatus,
            )
            assertEquals(
                "#595 一：保存失败时 ViewModel 当前章节必须保持旧章节（A）— " +
                    "导航目标与 currentSession 不得分裂",
                "A",
                vm.uiState.value.chapterTitle,
            )
            assertEquals(
                "保存失败时正文保持旧章节内容，不得被替换",
                "已有章节内容",
                vm.uiState.value.content,
            )
        }

    @Test
    fun switchChapterSetsLoadingTrueDuringTransition() =
        runTest {
            val vm = createVm()
            vm.enterChapterForTest("p", "v", "a", "A")
            commitActiveSession(vm, "p", "v", "a", "")

            // 内容为空 → 保存跳过 → 同步部分（loading=true、建新 session、启动加载）直接完成，
            // 然后挂起在真实 IO 加载上。加载完成前 loading 必须已置 true —
            // 编辑器在旧正文可见期间不会被重新绑定到新章节。
            val switchJob = launch { vm.switchChapter("p", "v", "b", "B") }
            // enterChapterForTest 置 loading=false；推进调度让 switchChapter 执行到置 loading=true。
            runCurrent()
            assertTrue(
                "切换章节时 loading 必须已置 true（编辑器隐藏，新章节 session 待内容就绪后创建）",
                vm.uiState.value.loading,
            )
            switchJob.join()
        }

    @Test
    fun loadFailure_returnsLoadFailedAndRestoresCurrentChapter() =
        runTest {
            val vm = createVm()
            vm.enterChapterForTest("p", "v", "a", "A")
            commitActiveSession(vm, "p", "v", "a", "")

            // 切换到 B：旧章节内容为空 → 跳过保存 → 新 session=B → 加载 B 失败
            val result = vm.switchChapter("p", "v", "b", "B")

            assertTrue(
                "加载失败必须返回 ChapterSwitchResult.LoadFailed",
                result is ChapterSwitchResult.LoadFailed,
            )
            val failed = result as ChapterSwitchResult.LoadFailed
            assertEquals("LoadFailed 必须携带请求章节 key", ChapterKey("p", "v", "b"), failed.requested)
            assertFalse("加载失败后 loading 必须恢复 false", vm.uiState.value.loading)
            assertEquals(
                "#595 一：加载失败必须回退标题到旧章节 — 不能让 UI 停留在「新标题 + 旧正文」分裂态",
                "A",
                vm.uiState.value.chapterTitle,
            )
        }

    @Test
    fun firstEntryLoadFailure_returnsLoadFailedWithoutRollbackTarget() =
        runTest {
            val vm = createVm()
            // 无旧章节（首次进入编辑器）→ 加载失败 → LoadFailed，标题保持空（无旧章节可回退）。
            val result = vm.switchChapter("p", "v", "b", "B")
            assertTrue(
                "首次进入加载失败必须返回 LoadFailed",
                result is ChapterSwitchResult.LoadFailed,
            )
            assertEquals(
                "首次进入失败时没有旧章节标题可回退",
                "",
                vm.uiState.value.chapterTitle,
            )
            assertFalse(vm.uiState.value.loading)
        }

    @Test
    fun sameChapterSwitchIsNoOp() =
        runTest {
            val vm = createVm()
            vm.enterChapterForTest("p", "v", "a", "A")
            val before = vm.uiState.value.loading
            val result = vm.switchChapter("p", "v", "a", "A")
            assertTrue(
                "相同章节切换必须直接返回 Success（无操作），不改变 loading",
                result is ChapterSwitchResult.Success,
            )
            assertEquals("相同章节切换不改变 loading", before, vm.uiState.value.loading)
            assertEquals("A", vm.uiState.value.chapterTitle)
        }

    @Test
    fun cancelledSwitch_restoresFullOldStateAndRethrowsCancellation() =
        runTest {
            val vm = createVm()
            vm.enterChapterForTest("p", "v", "a", "A")
            // 等 initChapter 的加载落定，保证事务起点是稳定状态。
            awaitLoadingSettled(vm)
            // 旧章节有非空正文 → 切换事务的"保存旧章节"阶段会调用保存端口。
            // initChapter 事务后 inputFrozen 保持 true（等待编辑器附着），
            // 测试环境无编辑器 — 显式确认附着以解除冻结，模拟真实附着。
            val lease = fakeCoordinator.currentInputLease()!!
            vm.confirmEditorAttached(vm.chapterTargetId("p", "v", "a"), lease)
            vm._uiState.value = vm._uiState.value.copy(content = "正文A")
            commitActiveSession(vm, "p", "v", "a", "正文A")
            markLocalDirty(vm, "p", "v", "a")

            // #597：可控保存端口 — 保存 A 时挂起，为取消制造确定性挂起点
            // （loadChapter 的 withContext(IO) 在无 native 时几乎立即返回，
            // 直接取消会落在已完成的 job 上，满载调度下偶发）。
            val saveGate = kotlinx.coroutines.CompletableDeferred<Unit>()
            var saveCalls = 0
            vm.chapterSavePort =
                object : com.xiwei.sujian.feature.editor.session.ChapterContentSavePort {
                    override suspend fun saveChapterContent(
                        projectId: String,
                        volumeId: String,
                        chapterId: String,
                        content: String,
                    ): BridgeResult<ChapterSaveReceipt> {
                        saveCalls++
                        saveGate.await()
                        return BridgeResult.Success(
                            ChapterSaveReceipt("c", 0L, "h", "m", "t", 0),
                        )
                    }
                }

            var cancellationSeen = false
            val job =
                launch {
                    try {
                        vm.requestOpenChapter("p", "v", "b", "B")
                    } catch (e: kotlinx.coroutines.CancellationException) {
                        // #595 一：取消必须向上重抛，不得被当作普通加载失败（LoadFailed）。
                        cancellationSeen = true
                        throw e
                    }
                }
            // 事务进入保存 A 的挂起点后取消。
            var spin = 0
            while (saveCalls < 1 && spin < 200) {
                runCurrent()
                spin++
            }
            assertTrue("切换事务必须调用保存端口（进入保存挂起点）", saveCalls >= 1)
            job.cancelAndJoin()
            // 放行保存端口，避免事务协程悬挂泄漏。
            saveGate.complete(Unit)
            runCurrent()

            assertTrue(
                "取消必须重新抛出 CancellationException — 不得吞掉并当加载失败处理",
                cancellationSeen,
            )
            // 取消后旧状态完整恢复：标题、正文、loading、saveStatus 全部回到切换前。
            assertEquals("取消后标题必须恢复旧章节", "A", vm.uiState.value.chapterTitle)
            assertEquals("取消后正文必须保留旧章节内容", "正文A", vm.uiState.value.content)
            assertFalse("取消后 loading 必须恢复 false", vm.uiState.value.loading)
            // 取消后 inputFrozen 必须释放：后续输入能正常进入状态（否则输入被冻结）。
            // #624 评论9：热路径走 onEditorApplied（不再传整章 String）。
            vm.onEditorApplied(
                EditorAppliedEvent(
                    revision = 1L,
                    transactionId = 1L,
                    operationKind = EditorOperationKind.INSERT,
                    source = EditorEditSource.NORMAL,
                    cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
                    contentChanged = true,
                    contentDelta = EditorContentDelta(insertedChars = 3),
                ),
            )
            assertEquals(
                "取消后输入必须解冻（inputFrozen 由 finally 复位 — 事件被接受并标 Unsaved）",
                SaveStatus.Unsaved,
                vm.uiState.value.saveStatus,
            )
        }

    @Test
    fun loadFailure_restoresCompleteOldUiStateSnapshot() =
        runTest {
            val vm = createVm()
            vm.enterChapterForTest("p", "v", "a", "A")
            commitActiveSession(vm, "p", "v", "a", "")
            // 等 initChapter 的加载落定（无 native 时必失败 → loading=false）。
            awaitLoadingSettled(vm)
            assertFalse("前置状态必须是已落定（loading=false）", vm.uiState.value.loading)
            val before = vm.uiState.value

            val result = vm.requestOpenChapter("p", "v", "b", "B")

            assertTrue(
                "加载失败必须返回 LoadFailed",
                result is ChapterSwitchResult.LoadFailed,
            )
            assertEquals(
                "#595 一：加载失败必须完整恢复旧 EditorUiState（content/hash/note/" +
                    "editorEnabled/saveStatus/loading/title 全部一致），不能只恢复标题",
                before,
                vm.uiState.value,
            )
        }

    @Test
    fun concurrentSwitchRequests_serializeWithoutDeadlock() =
        runTest {
            val vm = createVm()
            vm.enterChapterForTest("p", "v", "a", "A")
            commitActiveSession(vm, "p", "v", "a", "")

            // #595 一：并发点击多个章节 — 请求经 ChapterSwitchGate 串行执行，
            // 不得死锁；失败环境下最终状态必须回到旧章节。
            val first = async { vm.requestOpenChapter("p", "v", "b", "B") }
            val second = async { vm.requestOpenChapter("p", "v", "c", "C") }
            val results = listOf(first.await(), second.await())

            for (r in results) {
                assertTrue(
                    "并发切换必须正常完成（Success/SaveFailed/LoadFailed/Stale 之一），不得挂起：$r",
                    r is ChapterSwitchResult.Success ||
                        r is ChapterSwitchResult.SaveFailed ||
                        r is ChapterSwitchResult.LoadFailed ||
                        r is ChapterSwitchResult.Stale,
                )
            }
            assertFalse("并发切换后 loading 必须恢复 false", vm.uiState.value.loading)
            assertEquals("并发切换后标题回到旧章节", "A", vm.uiState.value.chapterTitle)
        }

    @Test
    fun requestOpenChapter_successPathIsRequiredByCallers() =
        runTest {
            val vm = createVm()
            vm.enterChapterForTest("p", "v", "a", "A")

            // 同一章节（已提交）→ Success；调用方据此导航，不触发回滚。
            val same = vm.requestOpenChapter("p", "v", "a", "A")
            assertTrue("已提交章节的请求必须 Success", same is ChapterSwitchResult.Success)
        }

    @Test
    fun switchWithWhitespaceOnlyBody_savesRawBodyInsteadOfSkipping() =
        runTest {
            val vm = createVm()
            // #624 评论1：纯空行/纯空白正文是用户正文，不是空文档 —
            // 章节切换保存旧章节时也必须原样保存，不得 trim 后当成空文档跳过。
            val whitespaceBody = "\n\n   \t\n"
            var savedContent: String? = null
            vm.chapterSavePort =
                object : com.xiwei.sujian.feature.editor.session.ChapterContentSavePort {
                    override suspend fun saveChapterContent(
                        projectId: String,
                        volumeId: String,
                        chapterId: String,
                        content: String,
                    ): BridgeResult<ChapterSaveReceipt> {
                        savedContent = content
                        return BridgeResult.Success(
                            ChapterSaveReceipt("c", 0L, "h", "m", "t", 0),
                        )
                    }
                }
            vm._uiState.value = vm._uiState.value.copy(content = whitespaceBody)
            vm.enterChapterForTest("p", "v", "a", "A")
            commitActiveSession(vm, "p", "v", "a", whitespaceBody)
            markLocalDirty(vm, "p", "v", "a")

            // 切换到 B：旧章节保存必须真实尝试（无 native → 新章节加载失败
            // 返回 LoadFailed），但保存端口必须收到原始空白正文。
            val result = vm.switchChapter("p", "v", "b", "B")

            assertTrue(
                "空白正文切换必须走到保存阶段（LoadFailed 说明保存已尝试、加载才失败）",
                result is ChapterSwitchResult.LoadFailed,
            )
            assertEquals(
                "纯空白正文必须原样保存，不得被 trim 后当成空文档跳过",
                whitespaceBody,
                savedContent,
            )
        }

    /**
     * #624 评论10 第2项：切章保存旧章节必须用真实 snapshot 正文，
     * 不得保存 `_uiState.content`（评论9 后本地输入不更新它 — 它停在打开章节时
     * 的旧正文；用它保存会把用户刚输入的内容覆盖回磁盘，数据丢失）。
     */
    @Test
    fun switchSaveOldChapter_savesSnapshotNotColdPathContent() =
        runTest {
            val vm = createVm()
            val snapshotText = "用户刚输入的真实正文"
            vm.enterChapterForTest("p", "v", "a", "A")
            commitActiveSession(vm, "p", "v", "a", snapshotText)
            markLocalDirty(vm, "p", "v", "a")
            // 评论9 后本地正常输入不更新 _uiState.content — 它停留在打开时的旧正文。
            vm._uiState.value = vm._uiState.value.copy(content = "打开章节时的旧正文")
            var savedContent: String? = null
            vm.chapterSavePort =
                object : com.xiwei.sujian.feature.editor.session.ChapterContentSavePort {
                    override suspend fun saveChapterContent(
                        projectId: String,
                        volumeId: String,
                        chapterId: String,
                        content: String,
                    ): BridgeResult<ChapterSaveReceipt> {
                        savedContent = content
                        return BridgeResult.Success(
                            ChapterSaveReceipt("c", 0L, "h", "m", "t", 0),
                        )
                    }
                }

            // 切换到 B：保存阶段必须真实执行（无 native → 新章节加载失败返回
            // LoadFailed），但保存端口收到的必须是 snapshot 正文，不是冷路径旧正文。
            val result = vm.switchChapter("p", "v", "b", "B")

            assertTrue(
                "切章必须走到保存阶段（LoadFailed 说明保存已尝试、加载才失败）",
                result is ChapterSwitchResult.LoadFailed,
            )
            assertEquals(
                "切章保存必须用真实 snapshot 正文，不得用 _uiState.content 旧正文覆盖",
                snapshotText,
                savedContent,
            )
        }

    /**
     * #624 评论12 第2项：完全没改过的非空章节（localDirty=false）每次切章不得
     * 重写磁盘 — 旧实现 `content.isNotEmpty() → Save` 无条件重写。
     */
    @Test
    fun switchChapter_untouchedNonEmptyChapter_doesNotRewriteDisk() =
        runTest {
            val vm = createVm()
            var saveCalls = 0
            vm.chapterSavePort =
                object : com.xiwei.sujian.feature.editor.session.ChapterContentSavePort {
                    override suspend fun saveChapterContent(
                        projectId: String,
                        volumeId: String,
                        chapterId: String,
                        content: String,
                    ): BridgeResult<ChapterSaveReceipt> {
                        saveCalls++
                        return BridgeResult.Success(
                            ChapterSaveReceipt("c", 0L, "h", "m", "t", 0),
                        )
                    }
                }
            vm._uiState.value = vm._uiState.value.copy(content = "正文A")
            vm.enterChapterForTest("p", "v", "a", "A")
            commitActiveSession(vm, "p", "v", "a", "正文A")

            val result = vm.switchChapter("p", "v", "b", "B")

            assertTrue(
                "未 dirty 章节切章必须跳过保存（LoadFailed 说明保存阶段未中止、加载才失败）",
                result is ChapterSwitchResult.LoadFailed,
            )
            assertEquals("未 dirty 的非空章节切章时不得重写磁盘", 0, saveCalls)
        }

    /**
     * #624 评论12 第2项：切章保存成功必须 markSaved 提交回 session 文档状态 —
     * 旧实现只记 saveReceipts，DocumentState.localDirty 保持 true，后面的同步
     * 事实会被 IgnoreDirtyConflict 拦截。
     */
    @Test
    fun switchChapter_saveSuccess_marksSavedIntoSessionStore() =
        runTest {
            val vm = createVm()
            val targetA = vm.chapterTargetId("p", "v", "a")
            vm.enterChapterForTest("p", "v", "a", "A")
            commitActiveSession(vm, "p", "v", "a", "正文A")
            markLocalDirty(vm, "p", "v", "a")
            vm.chapterSavePort =
                object : com.xiwei.sujian.feature.editor.session.ChapterContentSavePort {
                    override suspend fun saveChapterContent(
                        projectId: String,
                        volumeId: String,
                        chapterId: String,
                        content: String,
                    ): BridgeResult<ChapterSaveReceipt> {
                        return BridgeResult.Success(
                            ChapterSaveReceipt("c", 0L, "hash-A", "m", "t", 0),
                        )
                    }
                }

            val result = vm.switchChapter("p", "v", "b", "B")

            assertTrue(
                "保存成功后继续加载失败 — LoadFailed 证明切章已走完保存阶段",
                result is ChapterSwitchResult.LoadFailed,
            )
            assertEquals(
                "切章保存成功必须 markSaved — committedVersion 推进到落盘 hash",
                "hash-A",
                fakeCoordinator.documentCommittedVersionFor(targetA).contentHash,
            )
            assertEquals(
                "markSaved 后 store 记录 localDirty 必须为 false（同步事实不再被 IgnoreDirtyConflict 拦截）",
                false,
                fakeCoordinator.store.record(targetA)?.documentState?.localDirty,
            )
        }

    /**
     * #624 评论11 第2项/评论12：dirty+空正文切章必须尝试 Clear（旧实现
     * `else true` 直接放行，磁盘旧正文不会被清掉）。无 native → SaveFailed。
     */
    @Test
    fun switchChapter_dirtyEmptyOldChapter_attemptsClear() =
        runTest {
            val vm = createVm()
            vm.enterChapterForTest("p", "v", "a", "A")
            commitActiveSession(vm, "p", "v", "a", "")
            markLocalDirty(vm, "p", "v", "a")

            val result = vm.switchChapter("p", "v", "b", "B")

            assertTrue(
                "dirty+空正文切章必须真实尝试 Clear（无 native → SaveFailed）",
                result is ChapterSwitchResult.SaveFailed,
            )
            assertEquals(SaveStatus.SaveFailed, vm.uiState.value.saveStatus)
        }

    /**
     * #624 评论13 第2项：切章保存期间 revision 前进 — Repository 成功只代表
     * "这一版 lease 正文已落盘"，不能算成可以离开章节；必须重新签发最新 lease
     * 再保存，直到最新 revision 真正提交（Committed）。旧实现把 stale save 当
     * 成功直接切走，最新输入可能从未落盘。
     */
    @Test
    fun switchChapter_saveRevisionAdvancedDuringSave_reissuesLeaseAndSavesLatest() =
        runTest {
            val vm = createVm()
            val targetA = vm.chapterTargetId("p", "v", "a")
            vm.enterChapterForTest("p", "v", "a", "A")
            commitActiveSession(vm, "p", "v", "a", "旧正文")
            markLocalDirty(vm, "p", "v", "a")

            var saveCalls = 0
            val savedContents = mutableListOf<String>()
            vm.chapterSavePort =
                object : com.xiwei.sujian.feature.editor.session.ChapterContentSavePort {
                    override suspend fun saveChapterContent(
                        projectId: String,
                        volumeId: String,
                        chapterId: String,
                        content: String,
                    ): BridgeResult<ChapterSaveReceipt> {
                        saveCalls++
                        savedContents.add(content)
                        if (saveCalls == 1) {
                            // 模拟保存 IO 期间用户继续输入：revision 前进到 2。
                            val inputLease = fakeCoordinator.currentInputLease()
                            fakeCoordinator.applyLocalEdit(
                                EditorDocumentUpdate.LocalInput(
                                    targetId = targetA,
                                    revision = 2L,
                                    transactionId = 21L,
                                    operationKind = EditorOperationKind.INSERT,
                                    contentChanged = true,
                                    contentDelta = EditorContentDelta(insertedChars = 3),
                                    lease = inputLease!!,
                                ),
                            )
                            fakeCoordinator.installSnapshot(1UL, "旧正文新输入", 2L)
                            return BridgeResult.Success(
                                ChapterSaveReceipt("c", 0L, "hash-stale", "m", "t", 0),
                            )
                        }
                        return BridgeResult.Success(
                            ChapterSaveReceipt("c", 0L, "hash-latest", "m", "t", 0),
                        )
                    }
                }

            val result = vm.switchChapter("p", "v", "b", "B")

            assertTrue(
                "保存期间 revision 前进后必须重新保存最新版本" +
                    "（LoadFailed 说明保存已提交、新章节加载才失败）",
                result is ChapterSwitchResult.LoadFailed,
            )
            assertEquals("stale 版保存后必须重新签发 lease 再保存", 2, saveCalls)
            assertEquals("第一次保存的是旧 revision 的正文", "旧正文", savedContents[0])
            assertEquals("第二次保存的必须是最新 revision 的正文", "旧正文新输入", savedContents[1])
            assertEquals(
                "只有最新 revision 提交才 markSaved — committedVersion 必须是第二次保存的 hash",
                "hash-latest",
                fakeCoordinator.documentCommittedVersionFor(targetA).contentHash,
            )
            val receipt = vm.saveReceipts.receipt(targetA)
            assertTrue(
                "回执必须是最新 revision 的真实保存（DocumentOperationLease.toSaveToken）",
                receipt != null && receipt.rustRevision == 2L && receipt.textHash == "hash-latest",
            )
        }

    /**
     * #624 评论13 第2项：切章保存循环中 ctx.isLatest() 失效 — 必须返回 Stale
     * 并恢复旧状态，不得把 stale 保存算成成功继续提交新章节（latest-wins
     * 事务边界在保存期间同样生效）。
     */
    @Test
    fun switchChapter_staleDuringSaveLoop_returnsStale() =
        runTest {
            val vm = createVm()
            vm.enterChapterForTest("p", "v", "a", "A")
            commitActiveSession(vm, "p", "v", "a", "旧正文")
            markLocalDirty(vm, "p", "v", "a")

            val saveGate = kotlinx.coroutines.CompletableDeferred<Unit>()
            var saveCalls = 0
            vm.chapterSavePort =
                object : com.xiwei.sujian.feature.editor.session.ChapterContentSavePort {
                    override suspend fun saveChapterContent(
                        projectId: String,
                        volumeId: String,
                        chapterId: String,
                        content: String,
                    ): BridgeResult<ChapterSaveReceipt> {
                        saveCalls++
                        saveGate.await()
                        return BridgeResult.Success(
                            ChapterSaveReceipt("c", 0L, "hash-s", "m", "t", 0),
                        )
                    }
                }

            val first = async { vm.requestOpenChapter("p", "v", "b", "B") }
            // 等第一次切换进入保存挂起点。
            var spin = 0
            while (saveCalls < 1 && spin < 200) {
                runCurrent()
                spin++
            }
            assertTrue("第一次切换必须进入保存挂起点", saveCalls >= 1)
            // 第二个请求到达 — 第一个事务的 isLatest() 立即失效。
            val second = async { vm.requestOpenChapter("p", "v", "c", "C") }
            saveGate.complete(Unit)
            runCurrent()

            val firstResult = first.await()
            assertTrue(
                "保存期间 isLatest 失效必须返回 Stale（不得把 stale 保存当成功切走）",
                firstResult is ChapterSwitchResult.Stale,
            )
            val secondResult = second.await()
            assertTrue(
                "后续最新请求正常完成（Success/SaveFailed/LoadFailed/Stale 之一），不得挂起：$secondResult",
                secondResult is ChapterSwitchResult.Success ||
                    secondResult is ChapterSwitchResult.SaveFailed ||
                    secondResult is ChapterSwitchResult.LoadFailed ||
                    secondResult is ChapterSwitchResult.Stale,
            )
            assertFalse("Stale 后 loading 必须恢复 false", vm.uiState.value.loading)
            assertEquals("Stale 后章节保持旧章节", "A", vm.uiState.value.chapterTitle)
        }

    /**
     * #624 评论13 第3项：外部应用/同步合并不是一次 Save/Clear 操作 —
     * 不得记录回执。旧 buildSaveToken 会读"此刻的 currentInputLease"拼进
     * 目标 targetId（加载 B 时拼出 targetId=B + A 的 session/epoch 假身份），
     * 回执从身份上就是假的。DocumentSaveReceiptTracker 只记录真实
     * Save/Clear 操作使用的 DocumentOperationLease.toSaveToken。
     */
    @Test
    fun applyExternalContentToUi_doesNotRecordFabricatedSaveReceipt() =
        runTest {
            val vm = createVm()
            vm.enterChapterForTest("p", "v", "a", "A")
            val targetId = vm.chapterTargetId("p", "v", "a")

            vm.applyExternalContentToUi(targetId, "同步合并后的正文", "hash-sync")

            assertNull(
                "外部应用不得记录假回执 — 只有真实 Save/Clear 才进入回执跟踪器",
                vm.saveReceipts.receipt(targetId),
            )
        }

    @Test
    fun isCurrentChapter_reflectsCommittedSession() {
        val vm = createVm()
        vm.enterChapterForTest("p", "v", "a", "A")
        assertTrue(
            "initChapter 后当前章节必须匹配",
            vm.isCurrentChapter("p", "v", "a"),
        )
        assertFalse(
            "未提交的章节必须不匹配 — 防止旧 pane 用新正文 beginEdit 旧 target",
            vm.isCurrentChapter("p", "v", "b"),
        )
    }

    /**
     * #624 评论14 第2项：章节切换 prepare→commit→publish — B 在 commit 前不可见。
     *
     * switchLoadAndPrepare 不写 currentSession/_uiState/不 emit fact；只有 switchCommit
     * commit 成功后才发布 B。加载失败时 currentSession 必须保持旧章节 A —
     * B 从未被发布，WritingPane 的 isCurrentChapter 守卫不会提前 beginEdit(B)，
     * WritingPaneExternalContentFlow 不会消费提前发出的 REPOSITORY_LOAD fact。
     */
    @Test
    fun loadFailure_currentSessionStaysOnOldChapter_bNotPublishedBeforeCommit() =
        runTest {
            val vm = createVm()
            vm.enterChapterForTest("p", "v", "a", "A")
            vm._uiState.value = vm._uiState.value.copy(content = "正文A")
            commitActiveSession(vm, "p", "v", "a", "正文A")

            // 切换到 B：旧章节内容非空但 localDirty=false → 跳过保存 → 加载 B 失败。
            val result = vm.requestOpenChapter("p", "v", "b", "B")

            assertTrue(
                "无 native 时加载 B 必须失败",
                result is ChapterSwitchResult.LoadFailed,
            )
            assertTrue(
                "加载失败时 currentSession 必须保持旧章节 A" +
                    "（B 在 commit 前不发布 — isCurrentChapter(B) 必须为 false）",
                vm.isCurrentChapter("p", "v", "a"),
            )
            assertFalse(
                "B 在 commit 前不得发布成 currentSession",
                vm.isCurrentChapter("p", "v", "b"),
            )
            assertEquals(
                "加载失败时正文必须保持旧章节 A（B 的正文不提前写入 _uiState）",
                "正文A",
                vm.uiState.value.content,
            )
        }

    /**
     * #624 评论15 问题1：切章失败必须完整回滚 — 旧实现三条失败路径
     * （loadChapterForSwitch null / prepareTargetSession null /
     * commitPreparedSession false）只恢复 `_uiState`，不恢复 inputFrozen、
     * 不重建 save channel/startSaveActor。结果：旧章节虽然重新显示，但
     * inputFrozen 仍为 true，新的保存 channel 没有 actor 消费。用户看到旧
     * 正文恢复了，实际上输入被 ViewModel 丢弃，自动保存也停了。
     *
     * 修复：三条失败路径统一走 [restoreAfterSwitch]（恢复 currentSession、
     * 重建 channel、启动 save actor、恢复 autosave、解除 inputFrozen）。
     *
     * 测试环境无 native → loadChapterForSwitch 返回 null（路径1）— 正好覆盖
     * 加载失败回滚。断言 inputFrozen 恢复 false 且 saveActorJob 重启 active。
     */
    @Test
    fun loadFailure_restoresInputFrozenAndRestartsSaveActor() =
        runTest {
            val vm = createVm()
            vm.enterChapterForTest("p", "v", "a", "A")
            commitActiveSession(vm, "p", "v", "a", "")
            // enterChapterForTest 已置 loading=false；防御性等待落定。
            awaitLoadingSettled(vm)
            assertFalse("前置状态必须是已落定（loading=false）", vm.uiState.value.loading)

            // 切换到 B：旧章节 localDirty=false → 跳过保存 → 加载 B 失败（无 native）。
            val result = vm.requestOpenChapter("p", "v", "b", "B")

            assertTrue(
                "加载失败必须返回 LoadFailed",
                result is ChapterSwitchResult.LoadFailed,
            )
            assertFalse(
                "#624 评论15 问题1：加载失败后 inputFrozen 必须恢复 false" +
                    "（否则用户输入被 ViewModel 丢弃 — 旧正文可见但输入被冻结拦截）",
                vm.inputFrozen,
            )
            assertTrue(
                "#624 评论15 问题1：加载失败后 save actor 必须已重启（active）" +
                    "— 否则新 saveCommandChannel 无 actor 消费，自动保存停止",
                vm.saveActorJob?.isActive == true,
            )
        }

    /**
     * #624 评论15 问题1：首次进入（无旧章节）加载失败也必须解除 inputFrozen —
     * 旧实现路径1 只恢复 _uiState，inputFrozen 保持 true，编辑器永久冻结。
     */
    @Test
    fun firstEntryLoadFailure_unfreezesInputAndRestartsSaveActor() =
        runTest {
            val vm = createVm()
            // 无旧章节（首次进入）→ 加载失败 → LoadFailed。
            val result = vm.requestOpenChapter("p", "v", "b", "B")

            assertTrue(
                "首次进入加载失败必须返回 LoadFailed",
                result is ChapterSwitchResult.LoadFailed,
            )
            assertFalse(
                "#624 评论15 问题1：首次进入加载失败后 inputFrozen 必须恢复 false",
                vm.inputFrozen,
            )
            assertTrue(
                "#624 评论15 问题1：首次进入加载失败后 save actor 必须已重启（active）",
                vm.saveActorJob?.isActive == true,
            )
        }
}

/** #624 评论11 第3项：测试用进程级 stats writer scope（与 SujianAppDependencies 同构）。 */
private fun statsWriterScope(): CoroutineScope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
