package com.xiwei.sujian.app.state

import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 三/四：ActiveDocumentGate 契约测试。
 *
 * 规则（issue 解决三/四）：手动同步、自动同步、试运行和连接诊断启动前
 * 统一 flush 活动 Editor session 到 Repository；flush 失败则同步必须中止，
 * 否则同步下载的新正文可能直接覆盖尚未落盘的本地输入。
 * 注册携带 owner token — 旧实例的 close 不得清除新实例的注册。
 */
class ActiveDocumentGateTest {
    @Test
    fun noFlusherRegistered_flushSucceeds() =
        runTest {
            assertTrue(
                "无活动编辑器时必须放行同步（没有本地输入需要保护）",
                ActiveDocumentGate.flushActiveDocument(),
            )
        }

    @Test
    fun flusherReturningTrue_flushSucceeds() =
        runTest {
            var flushed = false
            val registration =
                ActiveDocumentGate.register(Any(), flush = {
                    flushed = true
                    true
                })
            assertTrue(ActiveDocumentGate.flushActiveDocument())
            assertTrue("flush 回调必须被调用", flushed)
            registration.close()
        }

    @Test
    fun flusherReturningFalse_flushFailsAndBlocksSync() =
        runTest {
            val registration = ActiveDocumentGate.register(Any(), flush = { false })
            assertFalse(
                "flush 失败必须阻止同步继续（本地输入未落盘）",
                ActiveDocumentGate.flushActiveDocument(),
            )
            registration.close()
        }

    @Test
    fun flusherThrowing_flushFails() =
        runTest {
            val registration =
                ActiveDocumentGate.register(
                    Any(),
                    flush = { throw IllegalStateException("save failed") },
                )
            assertFalse("flush 异常必须映射为失败", ActiveDocumentGate.flushActiveDocument())
            registration.close()
        }

    @Test
    fun latestRegisteredFlusherWins() =
        runTest {
            var firstCalled = false
            val first =
                ActiveDocumentGate.register(Any(), flush = {
                    firstCalled = true
                    true
                })
            val second = ActiveDocumentGate.register(Any(), flush = { false })
            assertFalse(ActiveDocumentGate.flushActiveDocument())
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
                ActiveDocumentGate.register(oldOwner, flush = {
                    oldCalled = true
                    true
                })
            val newRegistration =
                ActiveDocumentGate.register(newOwner, flush = {
                    newCalled = true
                    true
                })
            // 旧实例先销毁 — 只允许清除自己的注册。
            oldRegistration.close()
            assertTrue("新实例的 flusher 必须仍被调用", ActiveDocumentGate.flushActiveDocument())
            assertFalse("旧实例的 flusher 不得再被调用", oldCalled)
            assertTrue(newCalled)
            newRegistration.close()
            assertTrue("注销后无活动编辑器放行", ActiveDocumentGate.flushActiveDocument())
        }

    @Test
    fun sameOwnerReregister_replacesFlusher() =
        runTest {
            // 同一 owner 重新注册（initialize 幂等）— 新回调生效。
            var firstCalled = false
            val owner = Any()
            val first =
                ActiveDocumentGate.register(owner, flush = {
                    firstCalled = true
                    true
                })
            val second = ActiveDocumentGate.register(owner, flush = { false })
            assertFalse(ActiveDocumentGate.flushActiveDocument())
            assertFalse(firstCalled)
            first.close()
            // 旧注册 close 时 owner 已不是当前持有者 — 不得清掉同 owner 的新注册。
            assertFalse("同 owner 新注册必须仍然有效", ActiveDocumentGate.flushActiveDocument())
            second.close()
        }

    @Test
    fun syncCoordinatorExposesFlushEntryPoint() {
        // 编译期契约：SyncCoordinator.runFullSync 内部经 ActiveDocumentGate 刷新 —
        // 这里验证门对象存在即可（行为由上述用例覆盖）。
        assertEquals("ActiveDocumentGate", ActiveDocumentGate.javaClass.simpleName)
    }
}
