package com.xiwei.sujian.feature.sync.data

import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.app.WriterAppServiceHolder
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.core.interop.common.ResultEnvelope
import com.xiwei.sujian.feature.settings.data.SaveField
import com.xiwei.sujian.feature.settings.data.SettingsSaveResult
import com.xiwei.sujian.feature.sync.data.interop.SyncBridge
import com.xiwei.sujian.feature.sync.data.model.LegacyMigrationOutcome
import com.xiwei.sujian.feature.sync.data.model.SyncConfig
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #630 评论第 4 点 / D：[SyncRepository.migrateLegacyProfileIfNeeded] 行为测试。
 *
 * 用 fake [SyncBridge] 覆盖 Core 迁移接口的不同返回，验证 Android 侧：
 * - `not_needed` / `no_legacy_config` → null（继续正常提交）；
 * - `migrated` → profileStore 推进到新 generation，返回 null；
 * - `needs_reconfigure` → Failed(SYNC_CONFIG)，不继续提交；
 * - Core 抛错（BridgeResult.Error）→ Failed(SYNC_CONFIG)，不继续提交；
 * - 原生库未加载（BridgeResult.NotLoaded）→ Failed(SYNC_CONFIG)，不继续提交；
 * - 已有 committed profile → null（跳过迁移）。
 *
 * 失败/冲突路径不删旧凭据 — 由 Core 侧保证（Core 失败时不删）；Android 侧只决定是否继续提交。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class LegacyMigrationBehaviorTest {
    companion object {
        private const val KIND_MIGRATED = "migrated"
    }

    /**
     * Fake [SyncBridge]：只覆盖 [migrateLegacySyncProfile] 返回预设结果。
     *
     * 其他方法不 override，调用时会触发真实 holder.service 并抛 UnsatisfiedLinkError → NotLoaded，
     * 但本测试只调 [migrateLegacyProfileIfNeeded]，不触及其他方法。
     */
    private class FakeSyncBridge(
        holder: WriterAppServiceHolder,
        private val outcome: BridgeResult<LegacyMigrationOutcome>,
    ) : SyncBridge(holder) {
        override fun migrateLegacySyncProfile(): BridgeResult<LegacyMigrationOutcome> = outcome
    }

    /** Fake [AppServiceBridge]：覆盖 [syncBridge] 返回 [FakeSyncBridge]。 */
    private class FakeAppServiceBridge(
        holder: WriterAppServiceHolder,
        syncBridge: SyncBridge,
    ) : AppServiceBridge(holder) {
        override val syncBridge: SyncBridge = syncBridge
    }

    private fun newHolder(): WriterAppServiceHolder =
        WriterAppServiceHolder(
            appDataRoot = "/home/xiwei/.cache/agent-tmp/sujian-test-legacy-migration-data",
            projectsRoot = "/home/xiwei/.cache/agent-tmp/sujian-test-legacy-migration-projects",
        )

    private fun newRepo(outcome: BridgeResult<LegacyMigrationOutcome>): SyncRepository {
        val context = org.robolectric.RuntimeEnvironment.getApplication()
        val holder = newHolder()
        val fakeBridge = FakeAppServiceBridge(holder, FakeSyncBridge(holder, outcome))
        return SyncRepository(context, fakeBridge)
    }

    private fun storeOf(repo: SyncRepository): SyncProfileStore {
        val field = SyncRepository::class.java.getDeclaredField("profileStore\$delegate")
        field.isAccessible = true
        @Suppress("UNCHECKED_CAST")
        val lazy = field.get(repo) as kotlin.Lazy<SyncProfileStore>
        return lazy.value
    }

    private suspend fun readStoreState(repo: SyncRepository): SyncProfileStore.ProfileCommitState =
        storeOf(repo).readState()

    private fun successOutcome(
        kind: String,
        config: SyncConfig? = null,
    ): BridgeResult<LegacyMigrationOutcome> =
        BridgeResult.Success(LegacyMigrationOutcome(outcomeKind = kind, config = config))

    @Test
    fun notNeeded_returnsNullAndDoesNotTouchStore() =
        runTest {
            val repo = newRepo(successOutcome("not_needed"))
            storeOf(repo).clear()
            val result = repo.migrateLegacyProfileIfNeeded()
            assertNull("not_needed 必须返回 null 让 commitSyncProfile 继续正常提交", result)
            val state = readStoreState(repo)
            assertTrue("not_needed 不应推进 generation", !state.hasCommittedProfile)
        }

    @Test
    fun noLegacyConfig_returnsNullAndDoesNotTouchStore() =
        runTest {
            val repo = newRepo(successOutcome("no_legacy_config"))
            storeOf(repo).clear()
            val result = repo.migrateLegacyProfileIfNeeded()
            assertNull("no_legacy_config 必须返回 null 让 commitSyncProfile 继续正常提交", result)
            val state = readStoreState(repo)
            assertTrue("no_legacy_config 不应推进 generation", !state.hasCommittedProfile)
        }

    @Test
    fun migrated_advancesProfileStoreGenerationAndReturnsNull() =
        runTest {
            val repo =
                newRepo(
                    successOutcome(
                        KIND_MIGRATED,
                        config = SyncConfig(enabled = true, remoteUrl = "https://example.com/r.git"),
                    ),
                )
            val store = storeOf(repo)
            store.clear()
            val result = repo.migrateLegacyProfileIfNeeded()
            assertNull("migrated 必须返回 null 让 commitSyncProfile 继续正常提交", result)
            val state = readStoreState(repo)
            assertTrue("migrated 必须推进 profileStore 到已提交状态", state.hasCommittedProfile)
            assertEquals("migrated 后 activeGeneration 必须 > 0", 1L, state.activeGeneration)
            assertNotNull("migrated 后 committedConfigJson 必须非空", state.committedConfigJson)
        }

    @Test
    fun migrated_withoutConfig_returnsNullDefensively() =
        runTest {
            val repo = newRepo(successOutcome(KIND_MIGRATED, config = null))
            storeOf(repo).clear()
            val result = repo.migrateLegacyProfileIfNeeded()
            assertNull(
                "migrated 但 config 缺失时防御性返回 null（Core 不应产生此状态，但 Android 不崩溃）",
                result,
            )
            val state = readStoreState(repo)
            assertTrue("config 缺失不应推进 generation", !state.hasCommittedProfile)
        }

    @Test
    fun needsReconfigure_returnsFailedAndDoesNotCommit() =
        runTest {
            val repo =
                newRepo(
                    BridgeResult.Success(
                        LegacyMigrationOutcome(
                            outcomeKind = "needs_reconfigure",
                            reason = "project-a vs project-b remote mismatch",
                        ),
                    ),
                )
            storeOf(repo).clear()
            val result = repo.migrateLegacyProfileIfNeeded()
            assertTrue("needs_reconfigure 必须返回 Failed 让 UI 引导用户重选", result is SettingsSaveResult.Failed)
            val failed = result as SettingsSaveResult.Failed
            assertEquals(
                "Failed 必须标记 SYNC_CONFIG 字段",
                SaveField.SYNC_CONFIG,
                failed.failures.single().field,
            )
            val state = readStoreState(repo)
            assertTrue("needs_reconfigure 不应推进 generation（不偷偷挑 profile）", !state.hasCommittedProfile)
        }

    @Test
    fun coreError_returnsFailedAndDoesNotCommit() =
        runTest {
            val envelope = ResultEnvelope.errorOf("IO_ERROR", "simulated core failure")
            val repo =
                newRepo(
                    BridgeResult.Error(envelope, syncFailureKind = null),
                )
            storeOf(repo).clear()
            val result = repo.migrateLegacyProfileIfNeeded()
            assertTrue("Core 抛错必须返回 Failed，不继续提交", result is SettingsSaveResult.Failed)
            val failed = result as SettingsSaveResult.Failed
            assertEquals(SaveField.SYNC_CONFIG, failed.failures.single().field)
            val state = readStoreState(repo)
            assertTrue("Core 抛错不应推进 generation", !state.hasCommittedProfile)
        }

    @Test
    fun nativeNotLoaded_returnsFailedAndDoesNotCommit() =
        runTest {
            val repo = newRepo(BridgeResult.NotLoaded)
            storeOf(repo).clear()
            val result = repo.migrateLegacyProfileIfNeeded()
            assertTrue("原生库未加载必须返回 Failed，不继续提交", result is SettingsSaveResult.Failed)
            val state = readStoreState(repo)
            assertTrue("NotLoaded 不应推进 generation", !state.hasCommittedProfile)
        }

    @Test
    fun alreadyCommittedProfile_skipsMigrationAndReturnsNull() =
        runTest {
            val repo = newRepo(successOutcome(KIND_MIGRATED, config = SyncConfig(enabled = true)))
            val store = storeOf(repo)
            store.clear()
            // 先模拟一个已提交的 generation
            store.commitGeneration(5L, "{\"enabled\":true}")
            // 即使 Core 返回 migrated，已提交状态必须跳过迁移
            val result = repo.migrateLegacyProfileIfNeeded()
            assertNull("已有 committed profile 时必须跳过迁移返回 null", result)
            val state = readStoreState(repo)
            assertEquals("已提交 generation 不被迁移覆盖", 5L, state.activeGeneration)
        }

    @Test
    fun unknownOutcomeKind_returnsNullDefensively() =
        runTest {
            val repo = newRepo(successOutcome("unknown_future_kind"))
            storeOf(repo).clear()
            val result = repo.migrateLegacyProfileIfNeeded()
            assertNull(
                "未知 outcomeKind 防御性返回 null（向前兼容未来 Core 新增变体）",
                result,
            )
        }
}
