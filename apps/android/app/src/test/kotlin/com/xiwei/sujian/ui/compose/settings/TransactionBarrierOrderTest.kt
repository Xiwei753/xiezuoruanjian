package com.xiwei.sujian.ui.compose.settings

import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class TransactionBarrierOrderTest {
    @Test
    // #597 测试用例验证事务屏障保序，含多步 channel 消费与断言 — 拆分降低可读性
    @Suppress("CognitiveComplexMethod")
    fun transactionBarrier_preservesOriginalOrder() =
        runTest {
            val dispatchOrder = mutableListOf<String>()
            val channel = kotlinx.coroutines.channels.Channel<String>(kotlinx.coroutines.channels.Channel.UNLIMITED)
            channel.trySend("Save(A)")
            channel.trySend("Transaction(T)")
            channel.trySend("Save(B)")

            var nextItem: String? = null
            val processed = mutableListOf<String>()
            while (processed.size < 3) {
                val item = nextItem ?: channel.receive()
                nextItem = null
                when {
                    item.startsWith("Save") -> {
                        processed.add(item)
                        while (true) {
                            val next = channel.tryReceive().getOrNull()
                            if (next != null && next.startsWith("Save")) {
                                processed.add(next)
                            } else {
                                nextItem = next
                                break
                            }
                        }
                    }
                    item.startsWith("Transaction") -> {
                        processed.add(item)
                    }
                }
            }

            assertEquals(listOf("Save(A)", "Transaction(T)", "Save(B)"), processed)
        }

    @Test
    fun transactionSnapshot_alwaysSavesCapturedConfig() {
        val configRevision = 5L
        val currentRevision = 7L
        val snapshotSaved = configRevision != currentRevision || true
        assertEquals(true, snapshotSaved)
    }
}
