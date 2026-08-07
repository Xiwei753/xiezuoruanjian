package com.xiwei.sujian.ui

import com.xiwei.sujian.data.AppServiceBridge
import com.xiwei.sujian.data.BridgeResult
import com.xiwei.sujian.data.ChapterContentSavePort
import com.xiwei.sujian.data.SettingsRepository
import com.xiwei.sujian.data.WorkspaceRepository
import com.xiwei.sujian.data.WriterAppServiceHolder
import com.xiwei.sujian.editor.v2.coordinator.EditorDocumentUpdate
import com.xiwei.sujian.editor.v2.coordinator.EditorSessionCoordinator
import com.xiwei.sujian.editor.v2.coordinator.PreparedSessionHandle
import com.xiwei.sujian.editor.v2.coordinator.TargetSnapshot
import com.xiwei.sujian.editor.v2.coordinator.TextEditorProfile
import com.xiwei.sujian.editor.v2.coordinator.applyLocalEdit
import com.xiwei.sujian.editor.v2.coordinator.commitPreparedSession
import com.xiwei.sujian.model.ChapterSaveReceipt
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
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
 * #597（评论五）：真正执行完整保存流程的可控假保存对象测试。
 *
 * 流程：
 * 1. 开始保存正文 A（performSave 经 [ChapterContentSavePort] 调用假保存器，
 *    假保存器挂起直到测试放行）；
 * 2. 保存返回前继续输入正文 B（onContentChanged + applyLocalEdit 推进 revision）；
 * 3. 让 A 返回保存成功；
 * 4. 检查当前正文仍是 B；
 * 5. 检查页面仍显示未保存（revision 不匹配 → 不标记 Saved）；
 * 6. 检查 B 没有被 A 的晚到结果覆盖（chapterHash 仍是 B 的）。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class EditorSaveFlowTest {
    private companion object {
        const val TARGET_ID = "chapter-body:p:v:a"
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
                contentHash = "hash-A",
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

    private lateinit var bridge: AppServiceBridge
    private lateinit var coordinator: EditorSessionCoordinator
    private lateinit var vm: EditorViewModel
    private lateinit var savePort: ControllableSavePort

    @Before
    fun setUp() {
        bridge = AppServiceBridge(WriterAppServiceHolder("/tmp/sujian_test_workspace_597_save_flow"))
        coordinator = EditorSessionCoordinator(bridge)
        val app = RuntimeEnvironment.getApplication()
        val repo = WorkspaceRepository(app, bridge)
        vm = EditorViewModel(app)
        vm.initialize(repo, SettingsRepository(app, bridge), sessionCoordinator = coordinator)
        savePort = ControllableSavePort()
        vm.chapterSavePort = savePort
    }

    /** 通过 coordinator 提交一个带可控 sessionId 的活动会话，并同步 ViewModel 状态。 */
    private fun commitSession(
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
        vm.currentSession = EditorSession("s1", "p", "v", "a")
        vm.onContentChanged(text)
    }

    @Test
    fun saveInFlight_continuedTyping_notOverwrittenByLateReceipt() =
        runTest(UnconfinedTestDispatcher()) {
            commitSession(text = "正文A", revision = 1L)

            // 1. 开始保存正文 A — 假保存器挂起（保存 IO 未返回）。
            val saveJob =
                async(Dispatchers.Default) {
                    vm.performSave(
                        content = "正文A",
                        session = requireNotNull(vm.currentSession),
                        isAutoSave = false,
                        revisionAtEnqueue = 1L,
                    )
                }
            // 等保存器真正进入挂起（至少一次调用）。
            runCurrentUntil { savePort.calls >= 1 }
            assertTrue("performSave 必须已调用假保存器", savePort.calls >= 1)
            assertEquals("保存中的正文必须是 A", "正文A", savePort.savedContent)
            assertEquals(SaveStatus.Saving, vm.uiState.value.saveStatus)

            // 2. 保存返回前继续输入正文 B — UI 与会话层 revision 同步前进。
            vm.onContentChanged("正文B")
            val inputLease = coordinator.currentInputLease()
            assertTrue("会话提交后必须存在有效输入 lease", inputLease != null)
            coordinator.applyLocalEdit(
                EditorDocumentUpdate.LocalInput(
                    targetId = TARGET_ID,
                    text = "正文B",
                    revision = 2L,
                    transactionId = 11L,
                    lease = inputLease!!,
                ),
            )
            assertEquals("当前正文必须是 B", "正文B", vm.uiState.value.content)
            assertEquals(2L, coordinator.sessionState.revision)

            // 3. 让 A 返回保存成功（晚到回执）。
            savePort.gate.complete(Unit)
            assertTrue("performSave 必须以成功返回", saveJob.await())

            // 4. 当前正文仍是 B。
            assertEquals("A 的晚到结果不得覆盖 B", "正文B", vm.uiState.value.content)
            // 5. 页面仍显示未保存 — revision 已前进，不得标记 Saved。
            assertEquals("保存期间继续输入后不得显示已保存", SaveStatus.Unsaved, vm.uiState.value.saveStatus)
            // 6. B 没有被 A 的晚到结果覆盖 — chapterHash 不得变成 A 的 hash。
            assertEquals("", vm.uiState.value.chapterHash)
            assertTrue("B 必须保持 dirty（未落盘）", vm.contentDirty)
        }

    @Test
    fun saveInFlight_noFurtherTyping_marksSavedWithMatchingRevision() =
        runTest(UnconfinedTestDispatcher()) {
            commitSession(text = "正文A", revision = 1L)

            val saveJob =
                async(Dispatchers.Default) {
                    vm.performSave(
                        content = "正文A",
                        session = requireNotNull(vm.currentSession),
                        isAutoSave = false,
                        revisionAtEnqueue = 1L,
                    )
                }
            runCurrentUntil { savePort.calls >= 1 }

            // 保存期间没有新输入 — revision 未前进。
            savePort.gate.complete(Unit)
            assertTrue(saveJob.await())

            // revision 匹配 → 标记 Saved 并记录 hash。
            assertEquals(SaveStatus.Saved, vm.uiState.value.saveStatus)
            assertEquals("hash-A", vm.uiState.value.chapterHash)
            assertFalse("保存成功后 dirty 必须清", vm.contentDirty)
        }

    private suspend fun runCurrentUntil(condition: () -> Boolean) {
        var spins = 0
        while (!condition() && spins < 100) {
            kotlinx.coroutines.yield()
            spins++
        }
    }
}
