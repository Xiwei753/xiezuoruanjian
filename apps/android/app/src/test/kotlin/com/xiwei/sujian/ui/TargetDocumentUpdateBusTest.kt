package com.xiwei.sujian.ui

import com.xiwei.sujian.editor.v2.coordinator.DocumentFactOrigin
import com.xiwei.sujian.editor.v2.coordinator.DocumentVersion
import com.xiwei.sujian.editor.v2.coordinator.TargetDocumentFact
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
 * #595 二：按 target 分区的最新文档事实总线契约测试。
 *
 * 旧缺陷：documentUpdates 是 Channel.receiveAsFlow()（单消费者事件流），
 * 章节快速重组或 collector 短暂重叠时，事件可能被错误页面取走后过滤掉。
 *
 * 修复：TargetDocumentUpdateBus 按 target 分区保存完整文档事实
 * （text + sourceVersion + baseVersion，而非"最后一个事件对象"）—
 * - 新 collector 立即拿到该 target 的当前文档事实（replay 语义，不丢失）；
 * - 只投递自己 target 的事实（分区隔离）；
 * - 同 target 连续事实合并为最新值，重放旧事实由 reducer 的 sourceVersion
 *   幂等判断忽略，不会再次执行副作用。
 */
class TargetDocumentUpdateBusTest {
    private fun repoFact(
        targetId: String,
        version: Long,
        text: String = "t$version",
    ) = TargetDocumentFact(
        targetId = targetId,
        text = text,
        sourceVersion = DocumentVersion(contentHash = "hash-$version"),
        baseVersion = DocumentVersion(),
        origin = DocumentFactOrigin.REPOSITORY_LOAD,
    )

    @Test
    fun newCollector_receivesLatestFactImmediately() =
        runTest {
            val bus = TargetDocumentUpdateBus()
            bus.emit(repoFact("t1", 1L, "v1"))
            bus.emit(repoFact("t1", 2L, "v2"))

            // 新 collector（页面重组后重新收集）立即拿到当前文档事实 — 不丢失。
            val received = bus.updates("t1").first()
            assertEquals("t1", received.targetId)
            assertEquals("v2", received.text)
            assertEquals("hash-2", received.sourceVersion.contentHash)
        }

    @Test
    fun facts_arePartitionedByTarget() =
        runTest {
            val bus = TargetDocumentUpdateBus()
            val t1Events = async { bus.updates("t1").take(2).toList() }
            runCurrent()

            bus.emit(repoFact("t1", 1L))
            runCurrent()
            // t2 的事实不得投递给 t1 collector（分区隔离）。
            bus.emit(repoFact("t2", 1L))
            runCurrent()
            bus.emit(repoFact("t1", 2L))
            runCurrent()

            val received = t1Events.await()
            assertEquals(2, received.size)
            assertTrue("t1 collector 只收到 t1 事实", received.all { it.targetId == "t1" })
            assertEquals("hash-1", received[0].sourceVersion.contentHash)
            assertEquals("hash-2", received[1].sourceVersion.contentHash)
        }

    @Test
    fun multipleCollectors_allReceiveFacts() =
        runTest {
            val bus = TargetDocumentUpdateBus()
            // 两个 collector 并发收集 — 验证不是单消费者 Channel。
            val collectorA = async { bus.updates("t1").take(2).toList() }
            val collectorB = async { bus.updates("t1").take(1).toList() }
            runCurrent()

            bus.emit(repoFact("t1", 1L))
            runCurrent()
            bus.emit(repoFact("t1", 2L))
            runCurrent()

            assertEquals(2, collectorA.await().size)
            assertEquals(1, collectorB.await().size)
            assertEquals("hash-1", collectorB.await()[0].sourceVersion.contentHash)
        }

    @Test
    fun sameTargetSequentialFacts_conflateToLatest() =
        runTest {
            val bus = TargetDocumentUpdateBus()
            bus.emit(repoFact("t1", 1L))
            bus.emit(repoFact("t1", 2L))
            bus.emit(repoFact("t1", 3L))

            val received = bus.updates("t1").first()
            assertEquals("最新事实必须胜出", "hash-3", received.sourceVersion.contentHash)
        }

    @Test
    fun busStoresDocumentFactNotRawCommand() =
        runTest {
            // #595 二：总线保存的是文档事实（text + sourceVersion + baseVersion），
            // 不是"最后一个事件对象" — 新 collector 读到的是当前文档状态。
            val bus = TargetDocumentUpdateBus()
            bus.emit(
                TargetDocumentFact(
                    targetId = "t1",
                    text = "merged",
                    sourceVersion = DocumentVersion(contentHash = "h9", syncCommitId = "commit-42"),
                    baseVersion = DocumentVersion(contentHash = "h8"),
                    origin = DocumentFactOrigin.SYNC_MERGED,
                ),
            )
            val fact = bus.updates("t1").first()
            assertEquals("merged", fact.text)
            assertEquals("h9", fact.sourceVersion.contentHash)
            assertEquals("commit-42", fact.sourceVersion.syncCommitId)
            assertEquals("h8", fact.baseVersion.contentHash)
            assertEquals(DocumentFactOrigin.SYNC_MERGED, fact.origin)
        }
}
