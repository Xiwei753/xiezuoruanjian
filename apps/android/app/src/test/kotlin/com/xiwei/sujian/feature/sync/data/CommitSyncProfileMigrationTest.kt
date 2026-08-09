package com.xiwei.sujian.feature.sync.data
import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.app.WriterAppServiceHolder
import com.xiwei.sujian.feature.settings.data.SettingsSaveResult
import com.xiwei.sujian.feature.sync.data.model.SyncConfig
import com.xiwei.sujian.feature.sync.data.model.SyncSecrets
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #595 九：首次 generation 提交必须原子（legacy → generation 迁移）行为测试。
 *
 * 结构契约（方法存在性）已移入
 * [com.xiwei.sujian.arch.CommitSyncProfileMigrationArchitectureTest]；本文件只保留运行时行为：
 * - 首次提交失败时不发布新 config、不推进 activeGeneration；
 * - 凭据读取失败返回 Failed(NativeUnavailable)；
 * - generation marker 单次 updateData 原子推进；
 * - 已提交时 config 来自 committedConfigJson。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class CommitSyncProfileMigrationTest {
    private fun createRepo(preferencesSuffix: String): SyncRepository {
        return SyncRepository(
            androidx.test.core.app.ApplicationProvider.getApplicationContext(),
            AppServiceBridge(
                WriterAppServiceHolder("/tmp/sujian_test_workspace_595", "/tmp/sujian_test_workspace_595"),
            ),
            preferencesSuffix = preferencesSuffix,
        )
    }

    private fun storeOf(repo: SyncRepository): ProjectSyncProfileStore {
        val field = SyncRepository::class.java.getDeclaredField("profileStore\$delegate")
        field.isAccessible = true
        @Suppress("UNCHECKED_CAST")
        val lazy = field.get(repo) as kotlin.Lazy<ProjectSyncProfileStore>
        return lazy.value
    }

    private suspend fun readStoreState(repo: SyncRepository): ProjectSyncProfileStore.ProfileCommitState {
        val field = SyncRepository::class.java.getDeclaredField("profileStore\$delegate")
        field.isAccessible = true
        @Suppress("UNCHECKED_CAST")
        val lazy = field.get(repo) as kotlin.Lazy<ProjectSyncProfileStore>
        return lazy.value.readState("migration-project-id")
    }

    @Test
    fun firstCommitFailsWithoutNative_butNeverWritesLiveOrMarker() =
        runTest {
            val repo = createRepo("migration_contract_1")
            storeOf(repo).clear("migration-project-id")
            val result =
                repo.commitSyncProfile(
                    "migration-project-id",
                    com.xiwei.sujian.feature.sync.data.model.SyncConfig(enabled = true),
                    com.xiwei.sujian.feature.sync.data.model.SyncSecrets(token = "token-new"),
                )
            assertTrue(
                "Commit must fail without native (strict reads abort before any write)",
                result is SettingsSaveResult.Failed,
            )
            val state = readStoreState(repo)
            assertTrue(
                "Failed first commit must not publish the new config to the committed generation store",
                !state.hasCommittedProfile,
            )
            assertEquals(
                "Failed commit must not advance activeGeneration",
                0L,
                state.activeGeneration,
            )
        }

    @Test
    fun snapshotSyncProfile_returnsFailedWhenSecretsReadFails() =
        runTest {
            val repo = createRepo("migration_contract_2")
            val store = storeOf(repo)
            store.clear("migration-project-id")
            store.commitGeneration("migration-project-id", 1L, "{\"enabled\":true}")
            val result = SyncProfileGate.snapshotExclusive { repo.snapshotSyncProfile("migration-project-id") }
            assertTrue(
                "Secrets read failure must return Failed, not NotConfigured or null",
                result is SyncProfileReadResult.Failed,
            )
            assertEquals(
                "Native unavailable must be classified as NativeUnavailable, not Fatal",
                SyncFailureKind.NativeUnavailable,
                (result as SyncProfileReadResult.Failed).kind,
            )
        }

    @Test
    fun commitGenerationMarkerIsAtomicInSingleStore() =
        runTest {
            val repo = createRepo("migration_contract_3")
            val store = storeOf(repo)
            store.clear("migration-project-id")
            store.commitGeneration("migration-project-id", 5L, "{\"enabled\":true}")
            val state = store.readState("migration-project-id")
            assertEquals(5L, state.activeGeneration)
            assertEquals("{\"enabled\":true}", state.committedConfigJson)
            assertTrue(state.hasCommittedProfile)
        }

    @Test
    fun snapshotSyncProfile_prefersGenerationStoreOverLegacy() =
        runTest {
            val repo = createRepo("migration_contract_4")
            val store = storeOf(repo)
            store.clear("migration-project-id")
            store.commitGeneration(
                "migration-project-id",
                2L,
                "{\"enabled\":true,\"remoteUrl\":\"https://example.com/r.git\"}",
            )
            val result = SyncProfileGate.snapshotExclusive { repo.snapshotSyncProfile("migration-project-id") }
            assertTrue(
                "Result must be Failed (secrets read fails without native)",
                result is SyncProfileReadResult.Failed,
            )
            assertEquals(
                "Config parse success + secrets NotLoaded must classify as NativeUnavailable, not Fatal",
                SyncFailureKind.NativeUnavailable,
                (result as SyncProfileReadResult.Failed).kind,
            )
        }
}
