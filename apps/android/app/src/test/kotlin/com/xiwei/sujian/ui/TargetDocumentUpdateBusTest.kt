package com.xiwei.sujian.ui

import com.xiwei.sujian.editor.v2.coordinator.EditorDocumentUpdate
import kotlinx.coroutines.async
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.take
import kotlinx.coroutines.flow.toList
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 二：按 target 分区的最新正文事件总线契约测试。
 *
 * 旧缺陷：documentUpdates 是 Channel.receiveAsFlow()（单消费者事件流），
 * 章节快速重组或 collector 短暂重叠时，事件可能被错误页面取走后过滤掉。
 *
 * 修复：TargetDocumentUpdateBus 按 target 分区的最新事件 StateFlow —
 * - 新 collector 立即拿到该 target 的最新事件（replay 语义，不丢失）；
 * - 只投递自己 target 的事件（分区隔离）；
 * - 同 target 连续事件合并为最新值，旧事件由 reducer 的 contentVersion 丢弃。
 */
class TargetDocumentUpdateBusTest {

    private fun repoEvent(targetId: String, version: Long, text: String = "t$version") =
        EditorDocumentUpdate.RepositoryLoaded(
            targetId = targetId,
            text = text,
            fileHash = "hash-$version",
            revision = 0L,
            contentVersion = version,
        )

    @Test
    fun newCollector_receivesLatestEventImmediately() = runTest {
        val bus = TargetDocumentUpdateBus()
        bus.emit(repoEvent("t1", 1L, "v1"))
        bus.emit(repoEvent("t1", 2L, "v2"))

        // 新 collector（页面重组后重新收集）立即拿到最新事件 — 不丢失。
        val received = bus.updates("t1").first()
        assertEquals("t1", received.targetId)
        assertEquals("v2", received.text)
    }

    @Test
    fun events_arePartitionedByTarget() = runTest {
        val bus = TargetDocumentUpdateBus()
        val t1Events = async { bus.updates("t1").take(2).toList() }
        runCurrent()

        bus.emit(repoEvent("t1", 1L))
        runCurrent()
        // t2 的事件不得投递给 t1 collector（分区隔离）。
        bus.emit(repoEvent("t2", 1L))
        runCurrent()
        bus.emit(repoEvent("t1", 2L))
        runCurrent()

        val received = t1Events.await()
        assertEquals(2, received.size)
        assertTrue("t1 collector 只收到 t1 事件", received.all { it.targetId == "t1" })
        assertEquals(1L, received[0].contentVersion)
        assertEquals(2L, received[1].contentVersion)
    }

    @Test
    fun multipleCollectors_allReceiveEvents() = runTest {
        val bus = TargetDocumentUpdateBus()
        // 两个 collector 并发收集 — 验证不是单消费者 Channel。
        val collectorA = async { bus.updates("t1").take(2).toList() }
        val collectorB = async { bus.updates("t1").take(1).toList() }
        runCurrent()

        bus.emit(repoEvent("t1", 1L))
        runCurrent()
        bus.emit(repoEvent("t1", 2L))
        runCurrent()

        assertEquals(2, collectorA.await().size)
        assertEquals(1, collectorB.await().size)
        assertEquals(1L, collectorB.await()[0].contentVersion)
    }

    @Test
    fun sameTargetSequentialEvents_conflateToLatest() = runTest {
        val bus = TargetDocumentUpdateBus()
        bus.emit(repoEvent("t1", 1L))
        bus.emit(repoEvent("t1", 2L))
        bus.emit(repoEvent("t1", 3L))

        val received = bus.updates("t1").first()
        assertEquals("最新事件必须胜出", 3L, received.contentVersion)
    }
}
