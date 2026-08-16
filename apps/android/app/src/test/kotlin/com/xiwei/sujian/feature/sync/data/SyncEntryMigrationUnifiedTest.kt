package com.xiwei.sujian.feature.sync.data

import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.app.WriterAppServiceHolder
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.feature.sync.data.interop.SyncBridge
import com.xiwei.sujian.feature.sync.data.model.FullSyncResult
import com.xiwei.sujian.feature.sync.data.model.LegacyMigrationOutcome
import com.xiwei.sujian.feature.sync.data.model.SyncCapabilityData
import com.xiwei.sujian.feature.sync.data.model.SyncConfig
import com.xiwei.sujian.feature.sync.data.model.SyncSecrets
import com.xiwei.sujian.feature.sync.data.model.SyncStatus
import com.xiwei.sujian.feature.sync.data.model.SyncTrigger
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #630 评论 5307423953 Part A 行为测试：验证顶栏手动同步入口（[SyncCoordinator.runFullSync]
 * snapshot==null）与设置页入口共用同一个全局 profile 读取/迁移入口
 * [SyncRepository.loadCommittedSyncProfile]。
 *
 * 行为判定：runFullSync(snapshot=null) 在无 committed profile + Core 返回 migrated 时，
 * 必须触发迁移（迁移后 profileStore 有 committed profile）。旧代码走 snapshotSyncProfile()
 * 不触发迁移，profileStore 不会有 committed profile。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class SyncEntryMigrationUnifiedTest {
    companion object {
        private const val TEST_TOKEN = "ghp_test_token_part_a"
        private const val TEST_REMOTE_URL = "https://example.com/part-a.git"
    }

    private class FakeSyncBridge(
        holder: WriterAppServiceHolder,
        private val outcome: BridgeResult<LegacyMigrationOutcome>,
    ) : SyncBridge(holder) {
        override fun migrateLegacySyncProfileWithMetadata(
            metadata: List<com.xiwei.sujian.feature.sync.data.model.LegacyProfileMetadata>,
        ): BridgeResult<LegacyMigrationOutcome> = outcome
    }

    private class FakeAppServiceBridge(
        holder: WriterAppServiceHolder,
        syncBridge: SyncBridge,
        private val generationSecrets: MutableMap<ULong, SyncSecrets> = mutableMapOf(),
    ) : AppServiceBridge(holder) {
        override val syncBridge: SyncBridge = syncBridge

        override fun saveSyncSecretsForGeneration(
            generation: ULong,
            secrets: SyncSecrets,
        ): BridgeResult<Boolean> {
            generationSecrets[generation] = secrets
            return BridgeResult.Success(true)
        }

        override fun loadSyncSecretsForGeneration(generation: ULong): BridgeResult<SyncSecrets?> =
            BridgeResult.Success(generationSecrets[generation])
    }

    /**
     * Fake [SyncExecutionPort]：capability 返回 canRun=true，perform 返回成功，
     * setSecretsOverride/clearSecretsOverride 返回 true。不触发真实 native 调用。
     */
    private class FakeSyncExecution : SyncExecutionPort {
        override suspend fun capability(): SyncCapabilityData = SyncCapabilityData(canRun = true)

        override suspend fun setSecretsOverride(secrets: SyncSecrets): Boolean = true

        override suspend fun perform(
            config: SyncConfig,
            forceSync: Boolean,
        ): BridgeResult<FullSyncResult> =
            BridgeResult.Success(
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

        override suspend fun clearSecretsOverride(): Boolean = true
    }

    private fun newHolder(): WriterAppServiceHolder =
        WriterAppServiceHolder(
            appDataRoot = "/home/xiwei/.cache/agent-tmp/sujian-test-part-a-data",
            projectsRoot = "/home/xiwei/.cache/agent-tmp/sujian-test-part-a-projects",
        )

    private fun newRepo(
        outcome: BridgeResult<LegacyMigrationOutcome>,
        generationSecrets: MutableMap<ULong, SyncSecrets> = mutableMapOf(),
    ): SyncRepository {
        val context = org.robolectric.RuntimeEnvironment.getApplication()
        val holder = newHolder()
        val fakeBridge = FakeAppServiceBridge(holder, FakeSyncBridge(holder, outcome), generationSecrets)
        return SyncRepository(context, fakeBridge)
    }

    private fun storeOf(repo: SyncRepository): SyncProfileStore {
        val field = SyncRepository::class.java.getDeclaredField("profileStore\$delegate")
        field.isAccessible = true
        @Suppress("UNCHECKED_CAST")
        val lazy = field.get(repo) as kotlin.Lazy<SyncProfileStore>
        return lazy.value
    }

    private fun migratedOutcome(): BridgeResult<LegacyMigrationOutcome> =
        BridgeResult.Success(
            LegacyMigrationOutcome(
                outcomeKind = "migrated",
                config = SyncConfig(enabled = true, remoteUrl = TEST_REMOTE_URL),
                secrets = SyncSecrets(token = TEST_TOKEN),
            ),
        )

    /**
     * 顶栏手动同步（snapshot==null）触发迁移 — 证明 runFullSync 走 loadCommittedSyncProfile。
     */
    @Test
    fun runFullSync_snapshotNull_triggersMigration() =
        runTest {
            val repo = newRepo(migratedOutcome())
            val store = storeOf(repo)
            store.clear()
            val statusRepo = SyncStatusRepository(repo)
            // 用反射构造 SyncCoordinator 注入 FakeSyncExecution（internal constructor）
            val coordinatorClass = SyncCoordinator::class.java
            val constructor =
                coordinatorClass.getDeclaredConstructor(
                    SyncRepository::class.java,
                    SyncStatusRepository::class.java,
                    AppSyncDataBarrier::class.java,
                    SyncExecutionPort::class.java,
                )
            constructor.isAccessible = true
            val coordinator = constructor.newInstance(repo, statusRepo, null, FakeSyncExecution()) as SyncCoordinator

            val outcome = coordinator.runFullSync(SyncTrigger.Manual, snapshot = null)

            // 迁移必须被触发 — profileStore 有 committed profile
            val state = store.readState()
            assertTrue(
                "runFullSync(snapshot=null) 必须走 loadCommittedSyncProfile 触发迁移，" +
                    "迁移后 profileStore 应有 committed profile；got hasCommittedProfile=${state.hasCommittedProfile}",
                state.hasCommittedProfile,
            )
            // outcome 不应是 Failed（迁移成功后应继续执行同步）
            assertTrue(
                "迁移成功后 runFullSync 不应返回 Failed（应继续执行同步），got $outcome",
                outcome !is SyncOutcome.RetryableFailure && outcome !is SyncOutcome.TerminalFailure,
            )
        }

    /**
     * 顶栏手动同步传入 snapshot 时不触发迁移（snapshot 已提供，无需读取）。
     * 这验证 snapshot!=null 路径不误触发迁移。
     */
    @Test
    fun runFullSync_snapshotProvided_doesNotTriggerMigration() =
        runTest {
            val repo = newRepo(migratedOutcome())
            val store = storeOf(repo)
            store.clear()
            val statusRepo = SyncStatusRepository(repo)
            val coordinatorClass = SyncCoordinator::class.java
            val constructor =
                coordinatorClass.getDeclaredConstructor(
                    SyncRepository::class.java,
                    SyncStatusRepository::class.java,
                    AppSyncDataBarrier::class.java,
                    SyncExecutionPort::class.java,
                )
            constructor.isAccessible = true
            val coordinator = constructor.newInstance(repo, statusRepo, null, FakeSyncExecution()) as SyncCoordinator

            val snapshot =
                SyncProfileSnapshot(
                    generation = 1L,
                    config = SyncConfig(enabled = true, remoteUrl = TEST_REMOTE_URL).normalize(),
                    secrets = SyncSecrets(token = TEST_TOKEN),
                )
            coordinator.runFullSync(SyncTrigger.Manual, snapshot = snapshot)

            val state = store.readState()
            // 传入 snapshot 时不走 loadCommittedSyncProfile，不触发迁移
            assertTrue(
                "snapshot 已提供时不应触发迁移，hasCommittedProfile should be false，got ${state.hasCommittedProfile}",
                !state.hasCommittedProfile,
            )
        }
}
