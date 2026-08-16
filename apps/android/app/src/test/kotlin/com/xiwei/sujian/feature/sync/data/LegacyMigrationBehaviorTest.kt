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
import com.xiwei.sujian.feature.sync.data.model.SyncSecrets
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
 * #630 评论第 4 点 / D + 第 5 点 Part A/B/C-Android：[SyncRepository.migrateLegacyProfileIfNeeded]
 * 与 [SyncRepository.ensureGlobalProfileMigrated] 行为测试。
 *
 * 用 fake [SyncBridge] / [AppServiceBridge] 覆盖 Core 迁移接口的不同返回，验证 Android 侧：
 * - `not_needed` / `no_legacy_config` → null（继续正常提交）；
 * - `migrated` → profileStore 推进到新 generation，完整事务写入 secrets，返回 null；
 * - `needs_reconfigure` → Failed(SYNC_CONFIG)，不继续提交；
 * - Core 抛错（BridgeResult.Error）→ Failed(SYNC_CONFIG)，不继续提交；
 * - 原生库未加载（BridgeResult.NotLoaded）→ Failed(SYNC_CONFIG)，不继续提交；
 * - 已有 committed profile → null（跳过迁移）；
 * - `migrated` 但缺 config / secrets / 未知 outcomeKind → Failed（Part B 类型化失败）；
 * - 首次读自动迁移（Part A）：loadCommittedSyncProfile 在无 committed profile 时触发迁移；
 * - 首次写同 helper（Part A）：commitSyncProfile 也走 ensureGlobalProfileMigrated；
 * - PAT generation 完整提交（Part B）：migrated 后 snapshotSyncProfile 验证 PAT 能读回；
 * - 后续保存失败仍可读（Part B）：迁移 generation 已提交后，下次保存失败 PAT 仍可读。
 *
 * 失败/冲突路径不删旧凭据 — 由 Core 侧保证（Core 失败时不删）；Android 侧只决定是否继续提交。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class LegacyMigrationBehaviorTest {
    companion object {
        private const val KIND_MIGRATED = "migrated"
        private const val KIND_NOT_NEEDED = "not_needed"
        private const val TEST_TOKEN = "ghp_test_token_for_migration"
        private const val TEST_REMOTE_URL = "https://example.com/r.git"
    }

    /**
     * Fake [SyncBridge]：覆盖 [migrateLegacySyncProfileWithMetadata] 返回预设结果。
     *
     * 其他方法不 override，调用时会触发真实 holder.service 并抛 UnsatisfiedLinkError → NotLoaded，
     * 但本测试只调迁移/secrets 相关方法，不触及其他方法。
     */
    private class FakeSyncBridge(
        holder: WriterAppServiceHolder,
        private val outcome: BridgeResult<LegacyMigrationOutcome>,
    ) : SyncBridge(holder) {
        override fun migrateLegacySyncProfileWithMetadata(
            metadata: List<com.xiwei.sujian.feature.sync.data.model.LegacyProfileMetadata>,
        ): BridgeResult<LegacyMigrationOutcome> = outcome
    }

    /**
     * Fake [AppServiceBridge]：覆盖 [syncBridge] 返回 [FakeSyncBridge]，
     * 并用 in-memory map fake [saveSyncSecretsForGeneration] / [loadSyncSecretsForGeneration]
     * 供 Part B 完整事务测试验证 PAT 能从 active generation 读回。
     */
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

    private fun newHolder(): WriterAppServiceHolder =
        WriterAppServiceHolder(
            appDataRoot = "/home/xiwei/.cache/agent-tmp/sujian-test-legacy-migration-data",
            projectsRoot = "/home/xiwei/.cache/agent-tmp/sujian-test-legacy-migration-projects",
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

    private suspend fun readStoreState(repo: SyncRepository): SyncProfileStore.ProfileCommitState =
        storeOf(repo).readState()

    private fun successOutcome(
        kind: String,
        config: SyncConfig? = null,
        secrets: SyncSecrets? = null,
    ): BridgeResult<LegacyMigrationOutcome> =
        BridgeResult.Success(LegacyMigrationOutcome(outcomeKind = kind, config = config, secrets = secrets))

    @Test
    fun notNeeded_returnsNullAndDoesNotTouchStore() =
        runTest {
            val repo = newRepo(successOutcome(KIND_NOT_NEEDED))
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
                        config = SyncConfig(enabled = true, remoteUrl = TEST_REMOTE_URL),
                        secrets = SyncSecrets(token = TEST_TOKEN),
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
    fun migrated_withoutConfig_returnsFailedWithType() =
        runTest {
            val repo = newRepo(successOutcome(KIND_MIGRATED, config = null, secrets = SyncSecrets(token = TEST_TOKEN)))
            storeOf(repo).clear()
            val result = repo.migrateLegacyProfileIfNeeded()
            assertTrue(
                "migrated 但 config 缺失必须类型化返回 Failed（Part B 不静默返回 null）",
                result is SettingsSaveResult.Failed,
            )
            val failed = result as SettingsSaveResult.Failed
            assertEquals(SaveField.SYNC_CONFIG, failed.failures.single().field)
            val state = readStoreState(repo)
            assertTrue("config 缺失不应推进 generation", !state.hasCommittedProfile)
        }

    @Test
    fun migrated_withoutSecrets_returnsFailedWithType() =
        runTest {
            val repo =
                newRepo(
                    successOutcome(
                        KIND_MIGRATED,
                        config = SyncConfig(enabled = true, remoteUrl = TEST_REMOTE_URL),
                        secrets = null,
                    ),
                )
            storeOf(repo).clear()
            val result = repo.migrateLegacyProfileIfNeeded()
            assertTrue(
                "migrated 但 secrets 缺失必须类型化返回 Failed（Part B 不静默返回 null）",
                result is SettingsSaveResult.Failed,
            )
            val failed = result as SettingsSaveResult.Failed
            assertEquals(SaveField.SYNC_SECRETS, failed.failures.single().field)
            val state = readStoreState(repo)
            assertTrue("secrets 缺失不应推进 generation", !state.hasCommittedProfile)
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
    fun unknownOutcomeKind_returnsFailedWithType() =
        runTest {
            val repo = newRepo(successOutcome("unknown_future_kind"))
            storeOf(repo).clear()
            val result = repo.migrateLegacyProfileIfNeeded()
            assertTrue(
                "未知 outcomeKind 必须类型化返回 Failed（Part B 不静默返回 null）",
                result is SettingsSaveResult.Failed,
            )
            val failed = result as SettingsSaveResult.Failed
            assertEquals(SaveField.SYNC_CONFIG, failed.failures.single().field)
        }

    // ── Part A：首次读/写自动迁移 ──

    @Test
    fun loadCommittedSyncProfile_triggersMigrationWhenNoCommittedProfile() =
        runTest {
            val repo =
                newRepo(
                    successOutcome(
                        KIND_MIGRATED,
                        config = SyncConfig(enabled = true, remoteUrl = TEST_REMOTE_URL),
                        secrets = SyncSecrets(token = TEST_TOKEN),
                    ),
                )
            storeOf(repo).clear()
            // 首次 loadCommittedSyncProfile 应触发迁移，迁移成功后 snapshot 读回 migrated profile
            val readResult = repo.loadCommittedSyncProfile()
            assertTrue(
                "首次 load 应触发迁移并读到 migrated profile（Found 或 NotConfigured）",
                readResult !is SyncProfileReadResult.Failed,
            )
            val state = readStoreState(repo)
            assertTrue("首次 load 后迁移必须已推进 generation", state.hasCommittedProfile)
        }

    @Test
    fun loadCommittedSyncProfile_migrationFailedReturnsTypedFailed() =
        runTest {
            val repo = newRepo(successOutcome(KIND_MIGRATED, config = null, secrets = null))
            storeOf(repo).clear()
            val readResult = repo.loadCommittedSyncProfile()
            assertTrue(
                "迁移失败时 loadCommittedSyncProfile 必须类型化返回 Failed（Part A 不静默降级）",
                readResult is SyncProfileReadResult.Failed,
            )
            val state = readStoreState(repo)
            assertTrue("迁移失败不应推进 generation", !state.hasCommittedProfile)
        }

    @Test
    fun commitSyncProfile_usesEnsureGlobalProfileMigrated() =
        runTest {
            // 用 needs_reconfigure outcome 让迁移返回 Failed，
            // 验证 commitSyncProfile 走了 ensureGlobalProfileMigrated：
            // 若没走迁移会继续提交并触发真实 native 调用（UnsatisfiedLinkError），
            // 走了迁移则返回 Failed 且不推进 generation。
            val repo =
                newRepo(
                    BridgeResult.Success(
                        LegacyMigrationOutcome(
                            outcomeKind = "needs_reconfigure",
                            reason = "test reconfigure",
                        ),
                    ),
                )
            storeOf(repo).clear()
            val commitResult =
                repo.commitSyncProfile(
                    SyncConfig(enabled = true, remoteUrl = TEST_REMOTE_URL),
                    SyncSecrets(token = "new_token"),
                )
            assertTrue(
                "commitSyncProfile 必须走 ensureGlobalProfileMigrated，迁移失败时返回 Failed",
                commitResult is SettingsSaveResult.Failed,
            )
            val state = readStoreState(repo)
            assertTrue("迁移失败时 commitSyncProfile 不应推进 generation", !state.hasCommittedProfile)
        }

    // ── Part B：PAT generation 完整事务 ──

    @Test
    fun migrated_secretsPersistedToGenerationAndReadableBySnapshot() =
        runTest {
            val generationSecrets = mutableMapOf<ULong, SyncSecrets>()
            val repo =
                newRepo(
                    successOutcome(
                        KIND_MIGRATED,
                        config = SyncConfig(enabled = true, remoteUrl = TEST_REMOTE_URL),
                        secrets = SyncSecrets(token = TEST_TOKEN),
                    ),
                    generationSecrets,
                )
            storeOf(repo).clear()
            val migrateResult = repo.migrateLegacyProfileIfNeeded()
            assertNull("migrated 返回 null", migrateResult)
            val state = readStoreState(repo)
            val migrationGen = state.activeGeneration
            assertTrue("迁移 generation 必须 > 0", migrationGen > 0L)
            // PAT 必须已写入 active generation 的安全存储
            val stored = generationSecrets[migrationGen.toULong()]
            assertNotNull("PAT 必须已写入迁移 generation 的安全存储", stored)
            assertEquals(TEST_TOKEN, stored?.token)
            // snapshotSyncProfile 必须能从 active generation 读回 PAT
            val snapshot = repo.snapshotSyncProfile()
            assertTrue("snapshot 必须读到 migrated profile（Found）", snapshot is SyncProfileReadResult.Found)
            val found = snapshot as SyncProfileReadResult.Found
            assertEquals(TEST_TOKEN, found.snapshot.secrets.token)
            assertEquals(migrationGen, found.snapshot.generation)
        }

    @Test
    fun migrated_subsequentSaveFailureStillReadableFromMigrationGeneration() =
        runTest {
            val generationSecrets = mutableMapOf<ULong, SyncSecrets>()
            // 第一次：migrated 成功
            val repo1 =
                newRepo(
                    successOutcome(
                        KIND_MIGRATED,
                        config = SyncConfig(enabled = true, remoteUrl = TEST_REMOTE_URL),
                        secrets = SyncSecrets(token = TEST_TOKEN),
                    ),
                    generationSecrets,
                )
            storeOf(repo1).clear()
            val migrateResult = repo1.migrateLegacyProfileIfNeeded()
            assertNull("migrated 返回 null", migrateResult)
            val stateAfterMigration = readStoreState(repo1)
            val migrationGen = stateAfterMigration.activeGeneration
            assertTrue("迁移 generation 必须 > 0", migrationGen > 0L)

            // 第二次：模拟下一次保存失败 — 用同一个 store（同 DataStore 文件）+ 同一个 generationSecrets map
            // 但用一个新的 repo，其 fake bridge 在 saveSyncSecretsForGeneration 时返回 Failed
            val context = org.robolectric.RuntimeEnvironment.getApplication()
            val holder = newHolder()
            val failingBridge =
                object : AppServiceBridge(holder) {
                    override val syncBridge: SyncBridge =
                        object : SyncBridge(holder) {
                            override fun migrateLegacySyncProfileWithMetadata(
                                metadata: List<com.xiwei.sujian.feature.sync.data.model.LegacyProfileMetadata>,
                            ): BridgeResult<LegacyMigrationOutcome> {
                                return BridgeResult.Success(LegacyMigrationOutcome(outcomeKind = KIND_NOT_NEEDED))
                            }
                        }

                    override fun saveSyncSecretsForGeneration(
                        generation: ULong,
                        secrets: SyncSecrets,
                    ): BridgeResult<Boolean> {
                        return BridgeResult.Error(
                            ResultEnvelope.errorOf("IO_ERROR", "simulated save failure"),
                            syncFailureKind = null,
                        )
                    }

                    override fun loadSyncSecretsForGeneration(generation: ULong): BridgeResult<SyncSecrets?> =
                        BridgeResult.Success(generationSecrets[generation])
                }
            val repo2 = SyncRepository(context, failingBridge)
            // repo2 共用同一个 DataStore 文件（同 context），state 应仍是迁移后的状态
            val stateBeforeSecondSave = readStoreState(repo2)
            assertEquals("第二次保存前 state 应仍是迁移 generation", migrationGen, stateBeforeSecondSave.activeGeneration)

            // 第二次保存失败
            val secondResult =
                repo2.commitSyncProfile(
                    SyncConfig(enabled = true, remoteUrl = "https://example.com/new.git"),
                    SyncSecrets(token = "new_token_should_fail"),
                )
            assertTrue("第二次保存必须失败", secondResult is SettingsSaveResult.Failed)

            // PAT 仍可从迁移 generation 读
            val snapshot = repo2.snapshotSyncProfile()
            assertTrue(
                "保存失败后 PAT 仍可从迁移 generation 读（Found）",
                snapshot is SyncProfileReadResult.Found,
            )
            val found = snapshot as SyncProfileReadResult.Found
            assertEquals(TEST_TOKEN, found.snapshot.secrets.token)
            assertEquals(migrationGen, found.snapshot.generation)
        }
}
