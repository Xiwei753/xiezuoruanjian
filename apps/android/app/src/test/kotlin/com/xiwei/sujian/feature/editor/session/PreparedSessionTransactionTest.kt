package com.xiwei.sujian.feature.editor.session

import com.xiwei.sujian.feature.editor.window.EditingState
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #595 一：无副作用章节预准备行为测试。
 *
 * 规则（issue 解决一）：准备阶段只允许读取 B 的记录、验证或新建 B session、
 * 读取 snapshot、返回 [PreparedSessionHandle]；禁止 commit/cancel A、修改
 * activeTargetId、修改 WindowBindingState、修改全局 EditorSessionState、
 * 关闭任何既有 session。最终 requestId 校验通过后才由 [commitPreparedSession]
 * 一次性执行 A→B 切换；Abort 按 newlyCreated 区分：新建才关闭 session，
 * 借用的既有 session 恢复 previousRecord。
 *
 * 本测试通过真实驱动 [EditorSessionCoordinator] 的状态变化（registerTargetMeta /
 * applyLocalEdit / prepareTargetSessionForCommit / commitPreparedSession /
 * releasePreparedTarget）并断言可观察的 [EditorSessionState] 与 store 记录结果，
 * 验证预准备事务的无副作用与原子提交契约。测试环境无 native（session 创建返回
 * NotLoaded），因此 prepare 失败路径与手工构造 handle 的 commit/abort 路径在这里
 * 验证纯状态契约。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class PreparedSessionTransactionTest {
    private fun createCoordinator(): EditorSessionCoordinator {
        return EditorSessionCoordinator(
            com.xiwei.sujian.core.interop.app.AppServiceBridge(
                com.xiwei.sujian.core.interop.app.WriterAppServiceHolder(
                    "/tmp/sujian_test_workspace_595_prepared",
                    "/tmp/sujian_test_workspace_595_prepared",
                ),
            ),
        )
    }

    private fun lease(targetId: String): EditorInputLease = EditorInputLease(targetId, 0UL, 0L)

    @Test
    fun failedPrepare_doesNotTouchActiveTargetState() {
        // 旧实现：prepare(B) 会先 commit/cancel A 并把全局活动会话切到 B —
        // 请求过期/snapshot 失败后 A 的 session ID、Undo 历史、窗口绑定状态
        // 都无法恢复。新实现：prepare 失败（无 native 时 create 失败返回 null）
        // 必须完全不动 A 的全局状态。
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                "a",
                1L,
                1L,
                operationKind = EditorOperationKind.INSERT,
                contentChanged = true,
                contentDelta = EditorContentDelta(insertedChars = "textA".length),
                lease = lease("a"),
            ),
        )
        coordinator.registerTargetMeta("b", TextEditorProfile.DocumentBody, persistent = true)

        val before = coordinator.sessionState
        val handle = coordinator.prepareTargetSessionForCommit("b", "textB", 5)
        assertNull("无 native 时预准备失败（session 创建不可用）", handle)
        assertEquals(
            "预准备失败不得修改全局 SessionState（A 保持活动）",
            before,
            coordinator.sessionState,
        )
        assertEquals("a", coordinator.sessionState.targetId)
    }

    @Test
    fun commitPreparedSession_activatesTargetWithSnapshotAndInvalidatesLease() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("a", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                "a",
                1L,
                1L,
                operationKind = EditorOperationKind.INSERT,
                contentChanged = true,
                contentDelta = EditorContentDelta(insertedChars = "textA".length),
                lease = lease("a"),
            ),
        )
        // 提交前无活动目标 — 没有可签发的 lease（窗口未绑定）。
        val staleLease = lease("a")
        coordinator.registerTargetMeta("b", TextEditorProfile.DocumentBody, persistent = true)

        val handle =
            PreparedSessionHandle(
                targetId = "b",
                sessionId = 0UL,
                snapshot = TargetSnapshot("textB", 5, 2L, 0, 5),
                mode = PreparedSessionMode.Created,
                previousRecord = null,
            )
        assertTrue(coordinator.commitPreparedSession(handle))

        val state = coordinator.sessionState
        assertEquals("b", state.targetId)
        // #624 评论17 问题2：commitPreparedSession 后 target 进入 Detached（不造假窗口）。
        // 真实窗口绑定由 prepareSessionForEdit + completeWindowAttach 完成。
        assertNull(state.activeTargetId)
        assertEquals(0UL, state.sessionId)
        assertEquals(EditingState.IDLE, state.editingState)
        assertEquals(WindowBindingState.Detached("b", 0UL, handle.snapshot), state.bindingState)
        // #624 评论9：SessionState 无 text 镜像（正文在 TargetSnapshot.text 冷路径）。
        assertEquals(2L, state.revision)

        // 模拟真实窗口绑定完成 — 激活 target 以签发 lease。
        coordinator.activateAttachedForTest("b")

        // 提交使旧 lease 失效 — 旧 View 晚到的输入不能再进入会话层。
        assertFalse("提交后旧 lease 必须失效", coordinator.isInputLeaseCurrent(staleLease, "a"))
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                "a",
                9L,
                9L,
                operationKind = EditorOperationKind.INSERT,
                contentChanged = true,
                contentDelta = EditorContentDelta(insertedChars = "late input from stale view".length),
                lease = staleLease,
            ),
        )
        assertEquals(
            "旧 A 的晚到输入不得写入 B 的会话",
            "b",
            coordinator.sessionState.targetId,
        )
        // 新绑定签发的 lease 被接受。
        val leaseB = coordinator.currentInputLease()
        assertNotNull("提交后活动目标可签发新 lease", leaseB)
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                "b",
                3L,
                11L,
                operationKind = EditorOperationKind.INSERT,
                contentChanged = true,
                contentDelta = EditorContentDelta(insertedChars = "textB typed".length),
                lease = leaseB!!,
            ),
        )
        // #624 评论9：SessionState 无 text 镜像 — revision 足以证明输入已应用。
        assertEquals(3L, coordinator.sessionState.revision)
    }

    @Test
    fun commitPreparedSession_rejectsHandleMismatchingRecord() {
        // #595 一：防御 — 复用既有 session 时，记录已不再指向该 session → 拒绝提交。
        // 新建事务不要求记录已存在 handle.sessionId（prepare 不写 store），故 reject
        // 场景改为复用事务：handle 声称复用 7UL，但记录 sessionId 仍是 0UL。
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("b", TextEditorProfile.DocumentBody, persistent = true)
        val handle =
            PreparedSessionHandle(
                targetId = "b",
                sessionId = 7UL,
                snapshot = TargetSnapshot("textB", 5, 1L, 0, 5),
                mode = PreparedSessionMode.Borrowed,
                previousRecord = EditorSessionRecord("b", sessionId = 7UL),
            )
        assertFalse("复用事务记录不再指向 handle session 必须拒绝提交", coordinator.commitPreparedSession(handle))
        assertNull(coordinator.sessionState.targetId)
    }

    @Test
    fun commitPreparedSession_newlyCreatedWritesStoreSessionId() {
        // #595 一：新建 session（prepare 不写 store，记录 sessionId=0UL）提交时必须
        // 把 handle.sessionId 写入正式记录 — 旧实现要求 record.sessionId==handle.sessionId，
        // 新建 session（0UL != 7UL）永远失败，首次打开新章节必然 LoadFailed。
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("b", TextEditorProfile.DocumentBody, persistent = true)
        val handle =
            PreparedSessionHandle(
                targetId = "b",
                sessionId = 7UL,
                snapshot = TargetSnapshot("textB", 5, 2L, 0, 5),
                mode = PreparedSessionMode.Created,
                previousRecord = null,
            )
        assertTrue("新建 session 提交必须成功（不要求记录已存在该 sessionId）", coordinator.commitPreparedSession(handle))
        val state = coordinator.sessionState
        assertEquals(7UL, state.sessionId)
        // #624 评论17 问题2：commitPreparedSession 后 target 进入 Detached，activeTargetId=null。
        assertNull(state.activeTargetId)
        // #624 评论9：SessionState 无 text 镜像（正文在 TargetSnapshot.text 冷路径）。
        assertEquals(2L, state.revision)
        // store 记录的 sessionId 必须与 SessionState 一致（不再分裂为 0UL）。
        assertEquals(7UL, coordinator.getPersistentSessionId("b"))
    }

    @Test
    fun commitPreparedSession_newlyCreatedRejectsIfRecordReplaced() {
        // #595 一：新建事务期间记录被并发替换为有效 session（9UL）→ prepare 前值
        // 是 0UL，但记录已是 9UL，句柄失效，拒绝提交。
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("b", TextEditorProfile.DocumentBody, persistent = true)
        val handle =
            PreparedSessionHandle(
                targetId = "b",
                sessionId = 7UL,
                snapshot = TargetSnapshot("textB", 5, 2L, 0, 5),
                mode = PreparedSessionMode.Created,
                previousRecord = null,
            )
        // 模拟并发：直接通过复用事务把记录 sessionId 占用为 9UL。
        coordinator.commitPreparedSession(handle.copy(sessionId = 9UL))
        assertFalse("记录已被并发占用为 9UL，原句柄失效必须拒绝", coordinator.commitPreparedSession(handle))
    }

    @Test
    fun releasePreparedTarget_newlyCreated_removesRecordOnlyIfStillOwned() {
        // 记录已被其他路径替换（sessionId 不同）时，回滚不得删除新记录。
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("b", TextEditorProfile.DocumentBody, persistent = true)
        // 模拟事务期间记录被替换为另一个 session。
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                "b",
                1L,
                1L,
                operationKind = EditorOperationKind.INSERT,
                contentChanged = true,
                contentDelta = EditorContentDelta(insertedChars = "replaced".length),
                lease = lease("b"),
            ),
        )
        val handle =
            PreparedSessionHandle(
                targetId = "b",
                // 与替换后的记录 sessionId 相同（0UL）— 属于本事务新建
                sessionId = 0UL,
                snapshot = TargetSnapshot("textB", 5, 1L, 0, 5),
                mode = PreparedSessionMode.Created,
                previousRecord = null,
            )
        coordinator.releasePreparedTarget(handle)
        assertFalse("新建 session 回滚后记录必须移除", coordinator.isTargetRegistered("b"))
    }

    @Test
    fun prepareTargetSessionForCommit_existsWithThreeParams() {
        // prepareTargetSessionForCommit 拆分为扩展函数，编译为 EditorSessionLifecycleOpsKt 静态方法。
        // 扩展函数参数 = 接收者 + 3 个显式参数 = 4。
        val extClass = Class.forName("com.xiwei.sujian.feature.editor.session.EditorSessionLifecycleOpsKt")
        val method =
            extClass.declaredMethods.firstOrNull {
                it.name == "prepareTargetSessionForCommit" &&
                    it.parameterTypes.size == 4 &&
                    it.parameterTypes[0] == EditorSessionCoordinator::class.java
            }
        assertNotNull("prepareTargetSessionForCommit(targetId, initialText, initialSelection) 必须存在", method)
    }

    @Test
    fun preparedSessionHandle_carriesAbortFacts() {
        val handle =
            PreparedSessionHandle(
                targetId = "b",
                sessionId = 5UL,
                snapshot = TargetSnapshot("t", 1, 0L, 0, 1),
                mode = PreparedSessionMode.Created,
                previousRecord = null,
            )
        assertTrue(handle.mode is PreparedSessionMode.Created)
        assertNull(handle.previousRecord)
        assertEquals(5UL, handle.sessionId)
        val borrowed = handle.copy(mode = PreparedSessionMode.Borrowed, previousRecord = EditorSessionRecord("b"))
        assertTrue(borrowed.mode is PreparedSessionMode.Borrowed)
        assertEquals("b", borrowed.previousRecord?.targetId)
    }

    /**
     * #624 评论15 问题2：prepareTargetSessionForCommit 必须做"文档版本预准备" —
     * 不能无条件复用既有持久 session。当既有 session 的 snapshot 正文与
     * initialText 不一致且 localDirty=false 时，必须创建一个装有 initialText
     * 的 candidate session，记录被替换的旧 session ID；prepare 阶段不修改
     * store、不关闭旧 session。
     *
     * 旧实现：只看 existingId 有效就复用，完全不比较正文 — B 章节之前打开过
     * （旧正文），后台同步更新磁盘，再次打开 B 时 Repository 读到新正文
     * （loaded.text），但 prepare 复用旧 Rust session。switchCommit 又把
     * loaded.text/hash 写进 UI 并 applyExternalContentFact 标成新版本，
     * 最终磁盘/UI 是新正文，Rust editor session 还是旧正文。
     */
    @Test
    fun prepareTargetSessionForCommit_createsCandidateWhenSnapshotDiffersFromInitialText() {
        val coordinator = FakeSessionCoordinatorForPrepareSwap()
        coordinator.registerTargetMeta("b", TextEditorProfile.DocumentBody, persistent = true)
        // 模拟 B 之前打开过：store 中 "b" 的 record.sessionId = 100UL，旧正文 "oldText"。
        coordinator.installExistingPersistentSession(
            targetId = "b",
            sessionId = 100UL,
            text = "oldText",
            revision = 1L,
        )

        val handle = coordinator.prepareTargetSessionForCommit("b", "newTextFromDisk", 11)

        assertNotNull(
            "snapshot 不一致且 localDirty=false 时必须返回 candidate handle（不能复用旧 session）",
            handle,
        )
        handle!!
        assertEquals(
            "candidate session 必须是新创建的 ID（不是旧 100）",
            200UL,
            handle.sessionId,
        )
        assertEquals(
            "mode 必须是 Replacement 且 oldSessionId 记录被替换的旧 session 100",
            100UL,
            (handle.mode as PreparedSessionMode.Replacement).oldSessionId,
        )
        assertTrue("candidate swap 时 mode 必须是 Replacement", handle.mode is PreparedSessionMode.Replacement)
        assertEquals(
            "prepare 不得关闭旧 session（candidate 失败/回滚时旧 session 仍可用）",
            0,
            coordinator.closeCallCount(100UL),
        )
        assertEquals(
            "prepare 必须创建一个 candidate session",
            1,
            coordinator.createCallCount(),
        )
        assertEquals(
            "prepare 不得修改 store 记录（commit 是唯一写入点）",
            100UL,
            coordinator.store.record("b")?.sessionId,
        )
    }

    /**
     * #624 评论15 问题2：snapshot 正文与 initialText 一致且 localDirty=false →
     * 复用既有 session，保留 Undo/Redo；不创建 candidate。
     */
    @Test
    fun prepareTargetSessionForCommit_reusesSessionWhenSnapshotMatchesInitialText() {
        val coordinator = FakeSessionCoordinatorForPrepareSwap()
        coordinator.registerTargetMeta("b", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.installExistingPersistentSession(
            targetId = "b",
            sessionId = 100UL,
            text = "sameText",
            revision = 3L,
        )

        val handle = coordinator.prepareTargetSessionForCommit("b", "sameText", 8)

        assertNotNull("snapshot 一致时必须返回复用 handle", handle)
        handle!!
        assertEquals("复用既有 session ID 100", 100UL, handle.sessionId)
        assertTrue("复用时 mode 必须是 Borrowed", handle.mode is PreparedSessionMode.Borrowed)
        assertFalse("复用时 mode 不得是 Replacement", handle.mode is PreparedSessionMode.Replacement)
        assertEquals("复用时不得创建 candidate", 0, coordinator.createCallCount())
        assertEquals("复用时不得关闭既有 session", 0, coordinator.closeCallCount(100UL))
    }

    /**
     * #624 评论15 问题2：localDirty=true 时直接返回 null — 不能拿 Repository 内容
     * 覆盖本地未保存编辑，也不能提前把 committedVersion 标成新版本。
     */
    @Test
    fun prepareTargetSessionForCommit_returnsNullWhenLocalDirtyTrue() {
        val coordinator = FakeSessionCoordinatorForPrepareSwap()
        coordinator.registerTargetMeta("b", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.installExistingPersistentSession(
            targetId = "b",
            sessionId = 100UL,
            text = "userEditing",
            revision = 5L,
            localDirty = true,
        )

        val handle = coordinator.prepareTargetSessionForCommit("b", "newTextFromSync", 13)

        assertNull(
            "localDirty=true 时必须返回 null（类型化失败）— 不能用 Repository 内容覆盖本地编辑",
            handle,
        )
        assertEquals("localDirty 拒绝时不得关闭既有 session", 0, coordinator.closeCallCount(100UL))
        assertEquals("localDirty 拒绝时不得创建 candidate", 0, coordinator.createCallCount())
        assertEquals(
            "localDirty 拒绝时 store 记录不得修改",
            100UL,
            coordinator.store.record("b")?.sessionId,
        )
    }

    /**
     * #624 评论15 问题2：candidate swap commit 成功后必须关闭旧 session —
     * 旧 session 的 Undo/Redo/正文被 candidate 替换，不再保留孤儿 session。
     */
    @Test
    fun commitPreparedSession_candidateSwap_closesOldSessionAndActivatesCandidate() {
        val coordinator = FakeSessionCoordinatorForPrepareSwap()
        coordinator.registerTargetMeta("b", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.installExistingPersistentSession(
            targetId = "b",
            sessionId = 100UL,
            text = "oldText",
            revision = 1L,
        )
        val handle =
            PreparedSessionHandle(
                targetId = "b",
                sessionId = 200UL,
                snapshot = TargetSnapshot("newText", 7, 2L, 0, 7),
                mode = PreparedSessionMode.Replacement(100UL),
                previousRecord = coordinator.store.record("b"),
            )
        coordinator.installCandidateSnapshot(200UL, "newText", 2L)

        assertTrue(
            "candidate swap commit 必须成功",
            coordinator.commitPreparedSession(handle),
        )
        assertEquals(
            "commit 成功后必须关闭被替换的旧 session 100",
            1,
            coordinator.closeCallCount(100UL),
        )
        assertEquals(
            "commit 后活动 session 必须是 candidate 200",
            200UL,
            coordinator.sessionState.sessionId,
        )
        assertEquals(
            "commit 后 store 记录必须指向 candidate 200",
            200UL,
            coordinator.store.record("b")?.sessionId,
        )
    }

    /**
     * #624 评论15 问题2：candidate swap 回滚只关闭 candidate session，旧 session
     * 原样保留（Undo/Redo 不丢），store 恢复 previousRecord。
     */
    @Test
    fun releasePreparedTarget_candidateSwap_closesCandidateOnlyOldSessionPreserved() {
        val coordinator = FakeSessionCoordinatorForPrepareSwap()
        coordinator.registerTargetMeta("b", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.installExistingPersistentSession(
            targetId = "b",
            sessionId = 100UL,
            text = "oldText",
            revision = 1L,
        )
        val previousRecord = coordinator.store.record("b")
        val handle =
            PreparedSessionHandle(
                targetId = "b",
                sessionId = 200UL,
                snapshot = TargetSnapshot("newText", 7, 2L, 0, 7),
                mode = PreparedSessionMode.Replacement(100UL),
                previousRecord = previousRecord,
            )

        coordinator.releasePreparedTarget(handle)

        assertEquals(
            "回滚必须关闭 candidate session 200",
            1,
            coordinator.closeCallCount(200UL),
        )
        assertEquals(
            "回滚不得关闭旧 session 100（保留 Undo/Redo）",
            0,
            coordinator.closeCallCount(100UL),
        )
        assertEquals(
            "回滚后 store 必须恢复 previousRecord（sessionId=100）",
            100UL,
            coordinator.store.record("b")?.sessionId,
        )
    }

    /**
     * #624 评论15 问题2：candidate 创建失败（createSession 返回 null）时
     * 必须返回 null，旧 session 不动，store 不动。
     */
    @Test
    fun prepareTargetSessionForCommit_candidateCreationFails_returnsNullAndPreservesOldSession() {
        val coordinator = FakeSessionCoordinatorForPrepareSwap(allowCreateSession = false)
        coordinator.registerTargetMeta("b", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.installExistingPersistentSession(
            targetId = "b",
            sessionId = 100UL,
            text = "oldText",
            revision = 1L,
        )

        val handle = coordinator.prepareTargetSessionForCommit("b", "newText", 7)

        assertNull("candidate 创建失败时必须返回 null", handle)
        assertEquals(
            "candidate 创建失败时旧 session 不得关闭",
            0,
            coordinator.closeCallCount(100UL),
        )
        assertEquals(
            "candidate 创建失败时 store 不得修改",
            100UL,
            coordinator.store.record("b")?.sessionId,
        )
    }

    // ── #624 评论16 问题1：prepare 不关闭旧 session + 回滚不覆盖新记录 ──

    /**
     * #624 评论16 问题1：prepare 阶段不得关闭旧 session —
     * 无论 snapshot 读取失败还是 session 已失效，都不在 prepare 阶段 closeSession。
     * 无效旧 session 也不要在 prepare 阶段清理，真正 commit 新 candidate 后再清旧 ID。
     *
     * 旧缺陷：prepareTargetSessionForCommit 在 querySnapshotForSession 返回 null 时
     * 直接 closeSession(existingIdNonNull)，破坏旧 session 的 Undo/Redo/正文。
     */
    @Test
    fun prepareTargetSessionForCommit_doesNotCloseOldSessionInAnyFailureCase() {
        // 场景1：snapshot 读取失败（validateSession true 但 querySnapshotForSession null）
        val coord1 = FakeSessionCoordinatorForPrepareSwap()
        coord1.registerTargetMeta("b", TextEditorProfile.DocumentBody, persistent = true)
        coord1.installExistingPersistentSession("b", 100UL, "oldText", 1L)
        coord1.breakSnapshot(100UL)
        coord1.prepareTargetSessionForCommit("b", "newText", 7)
        assertEquals("snapshot 失败时 prepare 不得关闭旧 session", 0, coord1.closeCallCount(100UL))
        assertEquals("snapshot 失败时 prepare 不得修改 store", 100UL, coord1.store.record("b")?.sessionId)

        // 场景2：session 已失效（validateSession false）
        val coord2 = FakeSessionCoordinatorForPrepareSwap()
        coord2.registerTargetMeta("b", TextEditorProfile.DocumentBody, persistent = true)
        coord2.installExistingPersistentSession("b", 100UL, "oldText", 1L)
        coord2.invalidateSession(100UL)
        coord2.prepareTargetSessionForCommit("b", "newText", 7)
        assertEquals("session 失效时 prepare 不得 closeSession", 0, coord2.closeCallCount(100UL))
    }

    /**
     * #624 评论16 问题1：回滚不得覆盖事务期间被推进的新记录 —
     * candidate swap 和借用 session 的 abort 都不 store.put(previousRecord)。
     *
     * 旧缺陷：releaseCandidateSwap 无条件 store.put(previousRecord)，
     * 事务期间记录被并发推进（另一个 commit/applyLocalEdit 把 sessionId 改成 300）后，
     * 回滚把 sessionId 写回 100，丢失新记录。
     */
    @Test
    fun releasePreparedTarget_doesNotOverwriteRecordAdvancedDuringTransaction() {
        // 场景1：candidate swap 回滚
        val coord1 = FakeSessionCoordinatorForPrepareSwap()
        coord1.registerTargetMeta("b", TextEditorProfile.DocumentBody, persistent = true)
        coord1.installExistingPersistentSession("b", 100UL, "oldText", 1L)
        val prev1 = coord1.store.record("b")
        val handle1 =
            PreparedSessionHandle(
                targetId = "b",
                sessionId = 200UL,
                snapshot = TargetSnapshot("newText", 7, 2L, 0, 7),
                mode = PreparedSessionMode.Replacement(100UL),
                previousRecord = prev1,
            )
        coord1.store.put(prev1!!.copy(sessionId = 300UL))
        coord1.releasePreparedTarget(handle1)
        assertEquals("candidate swap 回滚不得覆盖新记录", 300UL, coord1.store.record("b")?.sessionId)

        // 场景2：借用 session 回滚
        val coord2 = FakeSessionCoordinatorForPrepareSwap()
        coord2.registerTargetMeta("b", TextEditorProfile.DocumentBody, persistent = true)
        coord2.installExistingPersistentSession("b", 100UL, "sameText", 1L)
        val prev2 = coord2.store.record("b")
        val handle2 =
            PreparedSessionHandle(
                targetId = "b",
                sessionId = 100UL,
                snapshot = TargetSnapshot("sameText", 8, 1L, 0, 8),
                mode = PreparedSessionMode.Borrowed,
                previousRecord = prev2,
            )
        coord2.store.put(prev2!!.copy(sessionId = 300UL))
        coord2.releasePreparedTarget(handle2)
        assertEquals("借用 session 回滚不得覆盖新记录", 300UL, coord2.store.record("b")?.sessionId)
    }
}

/**
 * #624 评论15 问题2：可控制 createSession/validateSession/closeSession/querySnapshotForSession
 * 的 fake — 让测试能模拟"既有持久 session + 旧正文"场景，验证 prepareTargetSessionForCommit
 * 的 candidate swap 行为。
 *
 * EditorSessionCoordinator 的 createSession/validateSession/closeSession 已改为
 * `internal open fun`（可测试性改进，不改变运行时行为），fake 可以 override 来
 * 返回可控 session ID 并追踪关闭事件。
 */
private class FakeSessionCoordinatorForPrepareSwap(
    private val allowCreateSession: Boolean = true,
) : EditorSessionCoordinator(
        com.xiwei.sujian.core.interop.app.AppServiceBridge(
            com.xiwei.sujian.core.interop.app.WriterAppServiceHolder(
                "/tmp/sujian_test_workspace_624_comment15_p2",
                "/tmp/sujian_test_workspace_624_comment15_p2",
            ),
        ),
    ) {
    private val snapshots = mutableMapOf<ULong, TargetSnapshot>()
    private val validSessions = mutableSetOf<ULong>()
    private val closeCounts = mutableMapOf<ULong, Int>()
    private var createCalls = 0
    private var nextSessionId = 200UL

    /**
     * 模拟 B 之前打开过：直接写 store 记录（sessionId + DocumentState.localDirty）
     * 并装上 snapshot，让 validateSession 返回 true、querySnapshotForSession 返回旧正文。
     */
    fun installExistingPersistentSession(
        targetId: String,
        sessionId: ULong,
        text: String,
        revision: Long,
        localDirty: Boolean = false,
    ) {
        validSessions.add(sessionId)
        val cursor = text.toByteArray(Charsets.UTF_8).size
        snapshots[sessionId] = TargetSnapshot(text, cursor, revision, 0, cursor)
        store.put(
            EditorSessionRecord(
                targetId = targetId,
                sessionId = sessionId,
                persistent = true,
                documentState =
                    DocumentState(
                        revision = revision,
                        selectionAnchorUtf8 = 0,
                        selectionHeadUtf8 = cursor,
                        localDirty = localDirty,
                    ),
            ),
        )
    }

    fun installCandidateSnapshot(
        sessionId: ULong,
        text: String,
        revision: Long,
    ) {
        val cursor = text.toByteArray(Charsets.UTF_8).size
        snapshots[sessionId] = TargetSnapshot(text, cursor, revision, 0, cursor)
        validSessions.add(sessionId)
    }

    /**
     * #624 评论16 问题1：让 validateSession 返回 true 但 querySnapshotForSession 返回 null —
     * 模拟既有 session 有效但 snapshot 读取失败的场景，验证 prepare 不关闭旧 session。
     */
    fun breakSnapshot(sessionId: ULong) {
        snapshots.remove(sessionId)
    }

    /**
     * #624 评论16 问题1：让 validateSession 返回 false —
     * 模拟既有 session 已失效的场景，验证 prepare 不关闭旧 session。
     */
    fun invalidateSession(sessionId: ULong) {
        validSessions.remove(sessionId)
        snapshots.remove(sessionId)
    }

    fun closeCallCount(sessionId: ULong): Int = closeCounts[sessionId] ?: 0

    fun createCallCount(): Int = createCalls

    internal override fun createSession(
        targetId: String,
        text: String,
        cursorByteOffset: Int,
        isPersistent: Boolean,
    ): ULong? {
        createCalls++
        if (!allowCreateSession) return null
        val id = nextSessionId++
        val cursor = text.toByteArray(Charsets.UTF_8).size
        snapshots[id] = TargetSnapshot(text, cursor, 1L, 0, cursor)
        validSessions.add(id)
        return id
    }

    internal override fun closeSession(sessionId: ULong) {
        if (sessionId == 0UL) return
        closeCounts[sessionId] = (closeCounts[sessionId] ?: 0) + 1
        validSessions.remove(sessionId)
        snapshots.remove(sessionId)
    }

    internal override fun validateSession(sessionId: ULong): Boolean = sessionId != 0UL && sessionId in validSessions

    internal override fun querySnapshotForSession(sessionId: ULong): TargetSnapshot? = snapshots[sessionId]
}
