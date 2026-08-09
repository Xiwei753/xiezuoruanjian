package com.xiwei.sujian.feature.sync.data

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * #592 三：SyncProfileGate 事务门控行为测试。
 *
 * 结构契约（commitSyncProfile 方法存在性）已移入
 * [com.xiwei.sujian.arch.CommitSyncProfileArchitectureTest]；本文件只保留运行时行为：
 * - commitExclusive / snapshotExclusive 保持原返回值。
 */
class CommitSyncProfileGateTest {
    @Test
    fun syncProfileGate_commitExclusive_preservesReturnValue() =
        kotlinx.coroutines.test.runTest {
            assertEquals("committed", SyncProfileGate.commitExclusive { "committed" })
        }

    @Test
    fun syncProfileGate_snapshotExclusive_preservesReturnValue() =
        kotlinx.coroutines.test.runTest {
            assertEquals(42, SyncProfileGate.snapshotExclusive { 42 })
        }
}
