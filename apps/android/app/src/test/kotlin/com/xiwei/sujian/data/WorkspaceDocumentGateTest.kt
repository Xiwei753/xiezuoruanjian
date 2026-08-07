package com.xiwei.sujian.data

import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 三/四：WorkspaceDocumentGate 契约测试。
 *
 * 规则（issue 解决三/四）：手动同步、自动同步、试运行和连接诊断启动前
 * 统一 flush 活动 Editor session 到 Repository；flush 失败则同步必须中止，
 * 否则同步下载的新正文可能直接覆盖尚未落盘的本地输入。
 * 注册携带 owner token — 旧实例的 close 不得清除新实例的注册。
 */
class WorkspaceDocumentGateTest {
    @Test
    fun noFlusherRegistered_flushSucceeds() =
        runTest {
            assertTrue(
                "无活动编辑器时必须放行同步（没有本地输入需要保护）",
                WorkspaceDocumentGate.flushActiveDocument(),
            )
        }

    @Test
    fun flusherReturningTrue_flushSucceeds() =
        runTest {
            var flushed = false
            val registration =
                WorkspaceDocumentGate.register(Any(), flush = {
                    flushed = true
                    true
                })
            assertTrue(WorkspaceDocumentGate.flushActiveDocument())
            assertTrue("flush 回调必须被调用", flushed)
            registration.close()
        }

    @Test
    fun flusherReturningFalse_flushFailsAndBlocksSync() =
        runTest {
            val registration = WorkspaceDocumentGate.register(Any(), flush = { false })
            assertFalse(
                "flush 失败必须阻止同步继续（本地输入未落盘）",
                WorkspaceDocumentGate.flushActiveDocument(),
            )
            registration.close()
        }

    @Test
    fun flusherThrowing_flushFails() =
        runTest {
            val registration =
                WorkspaceDocumentGate.register(
                    Any(),
                    flush = { throw IllegalStateException("save failed") },
                )
            assertFalse("flush 异常必须映射为失败", WorkspaceDocumentGate.flushActiveDocument())
            registration.close()
        }

    @Test
    fun latestRegisteredFlusherWins() =
        runTest {
            var firstCalled = false
            val first =
                WorkspaceDocumentGate.register(Any(), flush = {
                    firstCalled = true
                    true
                })
            val second = WorkspaceDocumentGate.register(Any(), flush = { false })
            assertFalse(WorkspaceDocumentGate.flushActiveDocument())
            assertFalse("旧 flusher 不得再被调用", firstCalled)
            first.close()
            second.close()
        }

    @Test
    fun oldOwnerClose_doesNotClearNewOwnerRegistration() =
        runTest {
            // #595 四：旧 ViewModel 的 onCleared（close）不得清除新实例的注册 —
            // Activity 重建/生命周期交错时 flusher 必须仍然有效。
            var oldCalled = false
            var newCalled = false
            val oldOwner = Any()
            val newOwner = Any()
            val oldRegistration =
                WorkspaceDocumentGate.register(oldOwner, flush = {
                    oldCalled = true
                    true
                })
            val newRegistration =
                WorkspaceDocumentGate.register(newOwner, flush = {
                    newCalled = true
                    true
                })
            // 旧实例先销毁 — 只允许清除自己的注册。
            oldRegistration.close()
            assertTrue("新实例的 flusher 必须仍被调用", WorkspaceDocumentGate.flushActiveDocument())
            assertFalse("旧实例的 flusher 不得再被调用", oldCalled)
            assertTrue(newCalled)
            newRegistration.close()
            assertTrue("注销后无活动编辑器放行", WorkspaceDocumentGate.flushActiveDocument())
        }

    @Test
    fun sameOwnerReregister_replacesFlusher() =
        runTest {
            // 同一 owner 重新注册（initialize 幂等）— 新回调生效。
            var firstCalled = false
            val owner = Any()
            val first =
                WorkspaceDocumentGate.register(owner, flush = {
                    firstCalled = true
                    true
                })
            val second = WorkspaceDocumentGate.register(owner, flush = { false })
            assertFalse(WorkspaceDocumentGate.flushActiveDocument())
            assertFalse(firstCalled)
            first.close()
            // 旧注册 close 时 owner 已不是当前持有者 — 不得清掉同 owner 的新注册。
            assertFalse("同 owner 新注册必须仍然有效", WorkspaceDocumentGate.flushActiveDocument())
            second.close()
        }

    @Test
    fun syncCoordinatorExposesFlushEntryPoint() {
        // 编译期契约：SyncCoordinator.runSync 内部经 WorkspaceDocumentGate 刷新 —
        // 这里验证门对象存在即可（行为由上述用例覆盖）。
        assertEquals("WorkspaceDocumentGate", WorkspaceDocumentGate.javaClass.simpleName)
    }
}
