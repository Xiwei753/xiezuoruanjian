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
 * #624 评论17 问题4：commitActiveSession/cancelActiveSession 第二次进锁时
 * 即使 activeTargetId 不匹配也要复位 COMMITTING/CANCELLING 中间态，
 * 避免 editorAttachDecision 卡在 Hold。
 *
 * 旧缺陷：两次进锁模式 — 第一次进锁设 COMMITTING + Committing(A)，
 * 锁外 closeSession，第二次进锁若 activeTargetId != pendingTargetId（锁外期间
 * 被其他线程改换）不重置 sessionState，保留 COMMITTING + Committing(A)。
 * 后续 editorAttachDecision 对 Committing 返回 Hold，新 target 的附着
 * LaunchedEffect 持续不触发 beginEdit → 永久卡死。
 *
 * 修复：第二次进锁的 else 分支复位自己设的中间态：
 * COMMITTING → IDLE，Committing(pendingTargetId) → Idle。
 *
 * 测试策略：由于难以精确控制两次进锁间的线程交错，使用 fake coordinator
 * 在 closeSession（锁外调用）时改换 activeTargetId，模拟锁外期间被其他
 * 线程改换的场景，直接验证第二次进锁的复位语义。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class CommitActiveSessionMidTxResetTest {
    /**
     * 模拟锁外 closeSession 期间 activeTargetId 被其他线程改换 —
     * fake 在 closeSession 时把 activeTargetId 改为 [swapTargetId]。
     *
     * closeSession 在 commitActiveSession/cancelActiveSession 的锁外执行
     * （Core 调用不得持 mutationLock），此时另一线程的 mutateSession 可以
     * 获取锁并改换 activeTargetId — 这是真实的并发场景。
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
     * 第二次进锁必须复位 COMMITTING + Committing(oldTarget) 中间态，
     * 不能保留卡死状态。
     *
     * 修复前（RED）：editingState 拟留 COMMITTING，bindingState 拟留 Committing(A)，
     * editorAttachDecision 返回 Hold → 永久卡死。
     * 修复后（GREEN）：editingState 复位 IDLE，bindingState 复位 Idle，
     * editorAttachDecision 返回 BeginEdit → 新窗口可附着。
     */
    @Test
    fun commitActiveSession_resetsCommittingMidStateWhenActiveTargetSwappedDuringClose() {
        val coordinator = MidTxSwapCoordinator(swapTargetId = "B")
        setupActiveSessionA(coordinator)

        // 调用 commitActiveSession — 第一次进锁设 COMMITTING + Committing(A)，
        // 锁外 closeSession 时 fake 把 activeTargetId 改为 B，
        // 第二次进锁 activeTargetId=B != A=pendingTargetId。
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
     * 第二次进锁必须复位 CANCELLING + Cancelling(oldTarget) 中间态。
     *
     * 修复前（RED）：editingState 拟留 CANCELLING，bindingState 拟留 Cancelling(A)，
     * editorAttachDecision 返回 Hold → 永久卡死。
     * 修复后（GREEN）：editingState 复位 IDLE，bindingState 复位 Idle。
     */
    @Test
    fun cancelActiveSession_resetsCancellingMidStateWhenActiveTargetSwappedDuringClose() {
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
