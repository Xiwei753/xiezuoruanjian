@file:Suppress("StringLiteralDuplication")

package com.xiwei.sujian.feature.editor.session

import com.xiwei.sujian.feature.editor.window.EditingState
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #624 评论17 问题3：commitPreparedSession 单次 mutateSession CAS — 锁内校验 B 前置条件
 * 后在同一 mutation 内完成 A 退出 + B 写入 + state 切换，删除旧 finalizePreparedSessionCommit
 * 两段提交。
 *
 * 旧缺陷（已修复）：
 * 1. 入口 `store.record(handle.targetId)` 和 `record.sessionId != expectedSessionId` 在锁外读（TOCTOU）。
 * 2. finalizePreparedSessionCommit 的 putRecord 无条件覆盖 store 记录的 sessionId — 锁外
 *    closeSession 期间 record 被并发改换后，finalize 仍覆盖。
 *
 * 新设计：单次 mutateSession 内校验 + 提交，锁外只 closeSession(A)。校验失败返回 false，
 * 不改 A/B、不关闭 A。成功后 state 原子提交，锁外 closeSession 不影响已提交 state。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class CommitPreparedSessionPremiseRecheckTest {
    /**
     * 建立旧活动目标 A（非持久，sessionId=1UL，窗口绑定）—
     * commitPreparedSession 锁内会读取 A 的关闭信息，锁外 closeSession(1UL)。
     */
    private fun setupActiveTargetA(coordinator: EditorSessionCoordinator) {
        coordinator.registerTargetMeta("A", TextEditorProfile.DocumentBody, persistent = false)
        coordinator.mutateSession {
            putRecord(record("A")!!.copy(sessionId = 1UL))
            sessionState =
                sessionState.copy(
                    targetId = "A",
                    sessionId = 1UL,
                    activeTargetId = "A",
                    bindingState = WindowBindingState.Attached("w1", "A", 1UL),
                    editingState = EditingState.EDITING,
                )
        }
    }

    /**
     * 入口校验：record(handle.targetId) 不存在时返回 false —
     * 单次 mutation CAS 锁内校验失败，不修改 A/B。
     */
    @Test
    fun commitPreparedSession_rejectsWhenRecordMissing() {
        val coordinator = makeCoordinator()
        // 不注册 B — record(B) 不存在
        val handle = makeHandle("B", 7UL)

        assertFalse(
            "record(handle.targetId) 不存在时必须返回 false",
            coordinator.commitPreparedSession(handle),
        )
    }

    /**
     * 入口校验：record.sessionId != expectedSessionId 时返回 false —
     * 单次 mutation CAS 锁内校验失败，不修改 record。
     */
    @Test
    fun commitPreparedSession_rejectsWhenSessionIdMismatchAtEntry() {
        val coordinator = makeCoordinator()
        coordinator.registerTargetMeta("B", TextEditorProfile.DocumentBody, persistent = true)
        // record(B).sessionId = 9UL，但 expectedSessionId = 0UL（Created + previousRecord=null）
        coordinator.mutateSession {
            putRecord(record("B")!!.copy(sessionId = 9UL))
        }
        val handle = makeHandle("B", 7UL)

        assertFalse(
            "record.sessionId(9UL) != expectedSessionId(0UL) 时必须返回 false",
            coordinator.commitPreparedSession(handle),
        )
        // 不得修改 record
        assertEquals(
            "拒绝时 record(B).sessionId 必须保持不变",
            9UL,
            coordinator.store.record("B")?.sessionId,
        )
    }

    /**
     * 正常路径：单次 mutation CAS 锁内校验通过 → 同一 mutation 内完成 A 退出 + B 写入 +
     * state 切成 Detached(B)。commit 返回 true，record(B).sessionId=handle.sessionId，
     * A 的 record 被移除（非持久），activeTargetId=null。
     */
    @Test
    fun commitPreparedSession_normalPathCommitsInSingleMutation() {
        val coordinator = makeCoordinator()
        setupActiveTargetA(coordinator)
        coordinator.registerTargetMeta("B", TextEditorProfile.DocumentBody, persistent = true)
        val handle = makeHandle("B", 7UL)

        val result = coordinator.commitPreparedSession(handle)

        assertTrue("正常路径 commitPreparedSession 必须成功", result)
        assertEquals(
            "record(B).sessionId 必须是 handle.sessionId 7UL",
            7UL,
            coordinator.store.record("B")?.sessionId,
        )
        // A 的 record 被移除（非持久 + 窗口绑定但非持久 → removeRecord）
        assertFalse(
            "A 的 record 必须被移除（非持久）",
            coordinator.store.isRegistered("A"),
        )
        val state = coordinator.sessionState
        assertEquals(
            "state 必须切到 Detached(B, 7UL)",
            WindowBindingState.Detached("B", 7UL, handle.snapshot),
            state.bindingState,
        )
        assertEquals(
            "activeTargetId 必须为 null",
            null,
            state.activeTargetId,
        )
        assertEquals(
            "editingState 必须为 IDLE",
            EditingState.IDLE,
            state.editingState,
        )
    }

    /**
     * 单次 mutation CAS：锁内校验失败时不改 A、不改 B、不关闭 A、返回 false。
     * A 的 record/state 原样保留。
     */
    @Test
    fun commitPreparedSession_premiseFailureLeavesAandBUnchanged() {
        val coordinator = makeCoordinator()
        setupActiveTargetA(coordinator)
        coordinator.registerTargetMeta("B", TextEditorProfile.DocumentBody, persistent = true)
        // record(B).sessionId = 9UL != expectedSessionId(0UL) → 锁内校验失败
        coordinator.mutateSession {
            putRecord(record("B")!!.copy(sessionId = 9UL))
        }
        val handle = makeHandle("B", 7UL)

        val result = coordinator.commitPreparedSession(handle)

        assertFalse("锁内校验失败必须返回 false", result)
        // A 不改
        assertEquals(
            "A 的 record 必须原样保留（sessionId=1UL）",
            1UL,
            coordinator.store.record("A")?.sessionId,
        )
        assertEquals(
            "A 的 activeTargetId 必须原样保留",
            "A",
            coordinator.sessionState.activeTargetId,
        )
        // B 不改
        assertEquals(
            "B 的 record 必须原样保留（sessionId=9UL）",
            9UL,
            coordinator.store.record("B")?.sessionId,
        )
    }

    private fun makeCoordinator(): EditorSessionCoordinator =
        EditorSessionCoordinator(
            com.xiwei.sujian.core.interop.app.AppServiceBridge(
                com.xiwei.sujian.core.interop.app.WriterAppServiceHolder(
                    "/home/xiwei/.cache/agent-tmp/sujian_test_624_c17_premise_recheck",
                    "/home/xiwei/.cache/agent-tmp/sujian_test_624_c17_premise_recheck",
                ),
            ),
        )

    private fun makeHandle(
        targetId: String,
        sessionId: ULong,
    ): PreparedSessionHandle =
        PreparedSessionHandle(
            targetId = targetId,
            sessionId = sessionId,
            snapshot = TargetSnapshot("textB", 5, 2L, 0, 5),
            mode = PreparedSessionMode.Created,
            previousRecord = null,
        )
}
