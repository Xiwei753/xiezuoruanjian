@file:Suppress("StringLiteralDuplication")

package com.xiwei.sujian.feature.editor.session

import com.xiwei.sujian.feature.editor.window.EditingState
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #624 评论5294575627 要求1：commitActiveSession/cancelActiveSession 改成单 mutation
 * "先认领再 close" — 不再有 COMMITTING/CANCELLING 中间态和两次进锁。
 *
 * 新实现：一次 mutateSession 完成 invalidateLease → removeRecord → sessionState=Idle →
 * 收集 closeSessionId → 解锁后 closeSession。closeSession 在 mutation 之后（锁外）调用。
 *
 * 测试验证：closeSession（锁外）期间 activeTargetId 被其他线程改换时，最终态正确 —
 * editingState=IDLE、bindingState=Idle、activeTargetId 保留被改换的值（不覆盖新状态）。
 * 新实现下 sessionState 在 mutation 内已直接落 Idle，closeSession 锁外改换 activeTargetId
 * 不影响已落定的 Idle/Idle，只改 activeTargetId。
 *
 * 测试策略：使用 fake coordinator 在 closeSession（锁外调用）时改换 activeTargetId，
 * 模拟锁外期间被其他线程改换的并发场景，验证最终态正确。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class CommitActiveSessionMidTxResetTest {
    /**
     * 模拟锁外 closeSession 期间 activeTargetId 被其他线程改换 —
     * fake 在 closeSession 时把 activeTargetId 改为 [swapTargetId]。
     *
     * closeSession 在 commitActiveSession/cancelActiveSession 的 mutation 之后
     * （锁外）调用，此时另一线程的 mutateSession 可以获取锁并改换 activeTargetId —
     * 这是真实的并发场景。新实现下 sessionState 已在 mutation 内落 Idle，
     * closeSession 锁外改换只影响 activeTargetId。
     */
    private class MidTxSwapCoordinator(
        private val swapTargetId: String,
    ) : EditorSessionCoordinator(
            com.xiwei.sujian.core.interop.app.AppServiceBridge(
                com.xiwei.sujian.core.interop.app.WriterAppServiceHolder(
                    "/home/xiwei/.cache/agent-tmp/sujian_test_624_c17_midtx_swap",
                    "/home/xiwei/.cache/agent-tmp/sujian_test_624_c17_midtx_swap",
                ),
            ),
        ) {
        internal override fun closeSession(sessionId: ULong) {
            // 不调用 super.closeSession — 测试环境无 native，且本测试只关心锁外
            // 期间 activeTargetId 被改换的状态机行为，不关心 Core session 关闭。
            // 模拟锁外期间被其他线程改换 activeTargetId。
            mutateSession {
                sessionState = sessionState.copy(activeTargetId = swapTargetId)
            }
        }
    }

    /**
     * 建立活动 session：注册非持久 target A，设 activeTargetId=A，
     * record(A).sessionId=1UL，bindingState=Attached(w1,A,1)，editingState=EDITING。
     */
    private fun setupActiveSessionA(coordinator: EditorSessionCoordinator) {
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
     * commitActiveSession 锁外 closeSession 期间 activeTargetId 被改换时，
     * 最终态必须正确 — editingState=IDLE、bindingState=Idle、activeTargetId 保留被改换的值。
     *
     * 新实现（单 mutation "先认领再 close"）：mutation 内 sessionState 已直接落 Idle，
     * 锁外 closeSession 期间 fake 改换 activeTargetId 不影响已落定的 Idle/Idle。
     */
    @Test
    fun commitActiveSession_finalStateCorrectWhenActiveTargetSwappedDuringClose() {
        val coordinator = MidTxSwapCoordinator(swapTargetId = "B")
        setupActiveSessionA(coordinator)

        // 调用 commitActiveSession — mutation 内 sessionState 落 Idle + removeRecord(A)，
        // 锁外 closeSession 时 fake 把 activeTargetId 改为 B。
        coordinator.commitActiveSession(null)

        val state = coordinator.sessionState
        assertEquals(
            "锁外期间 activeTargetId 被改换时，COMMITTING 必须复位为 IDLE（不卡死）",
            EditingState.IDLE,
            state.editingState,
        )
        assertTrue(
            "锁外期间 activeTargetId 被改换时，Committing(A) 必须复位为 Idle（不卡死），实际=" +
                state.bindingState.toString(),
            state.bindingState is WindowBindingState.Idle,
        )
        assertEquals(
            "activeTargetId 应保留为被改换的 B（不覆盖新状态）",
            "B",
            state.activeTargetId,
        )
    }

    /**
     * cancelActiveSession 锁外 closeSession 期间 activeTargetId 被改换时，
     * 最终态必须正确 — editingState=IDLE、bindingState=Idle、activeTargetId 保留被改换的值。
     *
     * 新实现（单 mutation "先认领再 close"）：mutation 内 sessionState 已直接落 Idle，
     * 锁外 closeSession 期间 fake 改换 activeTargetId 不影响已落定的 Idle/Idle。
     */
    @Test
    fun cancelActiveSession_finalStateCorrectWhenActiveTargetSwappedDuringClose() {
        val coordinator = MidTxSwapCoordinator(swapTargetId = "B")
        setupActiveSessionA(coordinator)

        coordinator.cancelActiveSession()

        val state = coordinator.sessionState
        assertEquals(
            "锁外期间 activeTargetId 被改换时，CANCELLING 必须复位为 IDLE（不卡死）",
            EditingState.IDLE,
            state.editingState,
        )
        assertTrue(
            "锁外期间 activeTargetId 被改换时，Cancelling(A) 必须复位为 Idle（不卡死），实际=" +
                state.bindingState.toString(),
            state.bindingState is WindowBindingState.Idle,
        )
        assertEquals(
            "activeTargetId 应保留为被改换的 B（不覆盖新状态）",
            "B",
            state.activeTargetId,
        )
    }

    /**
     * commitActiveSession 正常路径（锁外未改换 activeTargetId）仍重置 sessionState —
     * 回归保护，确保修复不破坏正常路径。
     */
    @Test
    fun commitActiveSession_normalPathStillResetsSessionState() {
        val coordinator =
            EditorSessionCoordinator(
                com.xiwei.sujian.core.interop.app.AppServiceBridge(
                    com.xiwei.sujian.core.interop.app.WriterAppServiceHolder(
                        "/home/xiwei/.cache/agent-tmp/sujian_test_624_c17_midtx_normal",
                        "/home/xiwei/.cache/agent-tmp/sujian_test_624_c17_midtx_normal",
                    ),
                ),
            )
        setupActiveSessionA(coordinator)

        // 正常路径：closeSession 不改换 activeTargetId（使用真实 coordinator）。
        // closeSession 调用真实 bridge（测试环境无 native，失败但 no-op）。
        coordinator.commitActiveSession(null)

        val state = coordinator.sessionState
        assertEquals(
            "正常路径 commitActiveSession 后 editingState 必须是 IDLE",
            EditingState.IDLE,
            state.editingState,
        )
        assertTrue(
            "正常路径 commitActiveSession 后 bindingState 必须是 Idle",
            state.bindingState is WindowBindingState.Idle,
        )
        assertEquals(
            "正常路径 commitActiveSession 后 activeTargetId 必须是 null",
            null,
            state.activeTargetId,
        )
    }
}
