package com.xiwei.sujian.data

import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.async
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.withTimeoutOrNull
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/**
 * #592 六：SyncProfileGate 锁序契约测试。
 *
 * 生产代码的锁序约定：
 * - commitSyncProfile 在 commitExclusive 内完成 stagedConfig → stagedSecrets →
 *   activeGeneration 原子提交，随后**释放锁后**才调用
 *   AutoSyncScheduler.scheduleFromSettings（后者获取 snapshotExclusive 读取快照）；
 * - 若 scheduleFromSettings 在锁内被调用，同一把进程级 Mutex 自重入会死锁。
 *
 * 本测试直接验证该契约依赖的互斥性质：commitExclusive 持有期间
 * snapshotExclusive 必须挂起，释放后立即完成（即锁序“提交先完成、读取后进入”
 * 不会死锁，且锁不泄漏）。
 */
class SyncProfileGateOrderTest {

    @Test
    fun snapshotRead_waitsForCommitInProgress_andCompletesAfterRelease() = runTest {
        val gateEntered = CompletableDeferred<Unit>()
        val releaseCommit = CompletableDeferred<Unit>()
        val commitDone = CompletableDeferred<Unit>()

        val commitJob = launch {
            SyncProfileGate.commitExclusive {
                gateEntered.complete(Unit)
                releaseCommit.await()
            }
            commitDone.complete(Unit)
        }
        gateEntered.await()

        val snapshotRead = async {
            SyncProfileGate.snapshotExclusive { 42 }
        }
        // commitExclusive 未释放前，snapshot 读取必须挂起 — 这正是
        // scheduleFromSettings 只能在提交锁释放后被调用的原因。
        val completedWhileCommitLocked = withTimeoutOrNull(200) { snapshotRead.await() }
        assertNull(
            "snapshotExclusive must suspend while commitExclusive is held",
            completedWhileCommitLocked,
        )

        releaseCommit.complete(Unit)
        commitJob.join()
        assertEquals("snapshot read must complete after commit lock release", 42, snapshotRead.await())
    }

    @Test
    fun commit_waitsForSnapshotReadInProgress_andCompletesAfterRelease() = runTest {
        val readEntered = CompletableDeferred<Unit>()
        val releaseRead = CompletableDeferred<Unit>()
        val readDone = CompletableDeferred<Unit>()

        val readJob = launch {
            SyncProfileGate.snapshotExclusive {
                readEntered.complete(Unit)
                releaseRead.await()
            }
            readDone.complete(Unit)
        }
        readEntered.await()

        val commit = async {
            SyncProfileGate.commitExclusive { "committed" }
        }
        val completedWhileReadLocked = withTimeoutOrNull(200) { commit.await() }
        assertNull(
            "commitExclusive must suspend while snapshotExclusive is held",
            completedWhileReadLocked,
        )

        releaseRead.complete(Unit)
        readJob.join()
        assertEquals("commit must complete after snapshot read lock release", "committed", commit.await())
    }

    @Test
    fun consecutiveCommitAndRead_doNotLeaveLockHeld() = runTest {
        val first = SyncProfileGate.commitExclusive { 1 }
        assertEquals(1, first)
        val second = SyncProfileGate.snapshotExclusive { 2 }
        assertEquals(2, second)
        // 再次进入必须立即可用（无锁泄漏/未释放分支）。
        val third = SyncProfileGate.commitExclusive { 3 }
        assertEquals(3, third)
    }
}
