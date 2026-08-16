package com.xiwei.sujian.feature.sync.work

import com.xiwei.sujian.feature.sync.data.SyncOutcome
import com.xiwei.sujian.feature.sync.data.model.FullSyncResult
import com.xiwei.sujian.feature.sync.data.model.SyncStatus
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #630 评论 5308040939 Part 2：AutoSyncWorker 的 outcome → WorkManager Result 映射。
 *
 * 聚合保留错误类型优先级后：RecoverableError → RetryableFailure → Result.retry()；
 * Fatal/Dirty/Conflict → TerminalFailure → Result.failure()。自动同步的
 * retry/failure 区分必须与 Core 聚合语义一致，不能把临时网络失败当成确定性失败。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class AutoSyncWorkerResultMappingTest {
    private fun retryableFailure(status: SyncStatus = SyncStatus.RecoverableError): SyncOutcome =
        SyncOutcome.RetryableFailure(status, com.xiwei.sujian.feature.sync.data.SyncFailureKind.RetryableNetwork)

    private fun terminalFailure(status: SyncStatus): SyncOutcome =
        SyncOutcome.TerminalFailure(status, com.xiwei.sujian.feature.sync.data.SyncFailureKind.fromSyncStatus(status))

    @Test
    fun recoverableError_mapsToRetry() {
        assertEquals(
            androidx.work.ListenableWorker.Result.retry().toString(),
            AutoSyncWorker.mapOutcomeToWorkerResult(retryableFailure(SyncStatus.RecoverableError)).toString(),
        )
    }

    @Test
    fun fatalError_mapsToFailure() {
        assertEquals(
            androidx.work.ListenableWorker.Result.failure().toString(),
            AutoSyncWorker.mapOutcomeToWorkerResult(terminalFailure(SyncStatus.FatalError)).toString(),
        )
    }

    @Test
    fun dirtyRepoBlocked_mapsToFailure() {
        assertEquals(
            androidx.work.ListenableWorker.Result.failure().toString(),
            AutoSyncWorker.mapOutcomeToWorkerResult(terminalFailure(SyncStatus.DirtyRepoBlocked)).toString(),
        )
    }

    @Test
    fun conflict_mapsToFailure() {
        assertEquals(
            androidx.work.ListenableWorker.Result.failure().toString(),
            AutoSyncWorker.mapOutcomeToWorkerResult(terminalFailure(SyncStatus.Conflict)).toString(),
        )
    }

    @Test
    fun partialConflict_mapsToFailure() {
        assertEquals(
            androidx.work.ListenableWorker.Result.failure().toString(),
            AutoSyncWorker.mapOutcomeToWorkerResult(terminalFailure(SyncStatus.PartialConflict)).toString(),
        )
    }

    @Test
    fun completed_mapsToSuccess() {
        val completed =
            SyncOutcome.Completed(
                FullSyncResult(
                    overallStatus = SyncStatus.Success,
                    targets = emptyList(),
                    totalUploaded = 0,
                    totalDownloaded = 0,
                    totalLocalDeletes = 0,
                    totalRemoteDeletes = 0,
                    totalOverwritten = 0,
                    totalIgnored = 0,
                    totalConflicts = 0,
                    error = null,
                    errorCategory = null,
                    messageKey = null,
                ),
            )
        assertEquals(
            androidx.work.ListenableWorker.Result.success().toString(),
            AutoSyncWorker.mapOutcomeToWorkerResult(completed).toString(),
        )
    }

    @Test
    fun busy_mapsToRetry() {
        assertEquals(
            androidx.work.ListenableWorker.Result.retry().toString(),
            AutoSyncWorker.mapOutcomeToWorkerResult(SyncOutcome.Busy).toString(),
        )
    }

    @Test
    fun unconfigured_mapsToSuccess() {
        assertEquals(
            androidx.work.ListenableWorker.Result.success().toString(),
            AutoSyncWorker.mapOutcomeToWorkerResult(SyncOutcome.Unconfigured).toString(),
        )
    }
}
