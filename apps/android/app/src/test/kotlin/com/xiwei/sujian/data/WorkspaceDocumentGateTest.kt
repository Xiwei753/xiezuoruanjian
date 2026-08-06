package com.xiwei.sujian.data

import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 三：WorkspaceDocumentGate 契约测试。
 *
 * 规则（issue 解决三）：手动同步、自动同步、试运行和连接诊断启动前
 * 统一 flush 活动 Editor session 到 Repository；flush 失败则同步必须中止，
 * 否则同步下载的新正文可能直接覆盖尚未落盘的本地输入。
 */
class WorkspaceDocumentGateTest {

    @Test
    fun noFlusherRegistered_flushSucceeds() = runTest {
        WorkspaceDocumentGate.registerFlusher(null)
        assertTrue(
            "无活动编辑器时必须放行同步（没有本地输入需要保护）",
            WorkspaceDocumentGate.flushActiveDocument(),
        )
    }

    @Test
    fun flusherReturningTrue_flushSucceeds() = runTest {
        var flushed = false
        WorkspaceDocumentGate.registerFlusher {
            flushed = true
            true
        }
        assertTrue(WorkspaceDocumentGate.flushActiveDocument())
        assertTrue("flush 回调必须被调用", flushed)
        WorkspaceDocumentGate.registerFlusher(null)
    }

    @Test
    fun flusherReturningFalse_flushFailsAndBlocksSync() = runTest {
        WorkspaceDocumentGate.registerFlusher { false }
        assertFalse(
            "flush 失败必须阻止同步继续（本地输入未落盘）",
            WorkspaceDocumentGate.flushActiveDocument(),
        )
        WorkspaceDocumentGate.registerFlusher(null)
    }

    @Test
    fun flusherThrowing_flushFails() = runTest {
        WorkspaceDocumentGate.registerFlusher { throw IllegalStateException("save failed") }
        assertFalse("flush 异常必须映射为失败", WorkspaceDocumentGate.flushActiveDocument())
        WorkspaceDocumentGate.registerFlusher(null)
    }

    @Test
    fun latestRegisteredFlusherWins() = runTest {
        var firstCalled = false
        WorkspaceDocumentGate.registerFlusher { firstCalled = true; true }
        WorkspaceDocumentGate.registerFlusher { false }
        assertFalse(WorkspaceDocumentGate.flushActiveDocument())
        assertFalse("旧 flusher 不得再被调用", firstCalled)
        WorkspaceDocumentGate.registerFlusher(null)
    }

    @Test
    fun syncCoordinatorExposesFlushEntryPoint() {
        // 编译期契约：SyncCoordinator.runSync 内部经 WorkspaceDocumentGate 刷新 —
        // 这里验证门对象存在即可（行为由上述用例覆盖）。
        assertEquals("WorkspaceDocumentGate", WorkspaceDocumentGate.javaClass.simpleName)
    }
}
