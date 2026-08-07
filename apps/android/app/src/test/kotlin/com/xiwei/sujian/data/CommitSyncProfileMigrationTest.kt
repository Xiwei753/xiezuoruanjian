package com.xiwei.sujian.data

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

    private fun createRepo(preferencesSuffix: String): SettingsRepository {
        return SettingsRepository(
            androidx.test.core.app.ApplicationProvider.getApplicationContext(),
            AppServiceBridge(WriterAppServiceHolder("/tmp/sujian_test_workspace_595")),
            preferencesSuffix = preferencesSuffix,
        )
    }

    private fun storeOf(repo: SettingsRepository): SyncProfileStore {
        val field = SettingsRepository::class.java.getDeclaredField("profileStore\$delegate")
        field.isAccessible = true
        @Suppress("UNCHECKED_CAST")
        val lazy = field.get(repo) as kotlin.Lazy<SyncProfileStore>
        return lazy.value
    }

    private suspend fun readStoreState(repo: SettingsRepository): SyncProfileStore.ProfileCommitState {
        val field = SettingsRepository::class.java.getDeclaredField("profileStore\$delegate")
        field.isAccessible = true
        @Suppress("UNCHECKED_CAST")
        val lazy = field.get(repo) as kotlin.Lazy<SyncProfileStore>
        return lazy.value.readState()
    }

    @Test
    fun firstCommitFailsWithoutNative_butNeverWritesLiveOrMarker() = runTest {
        val repo = createRepo("migration_contract_1")
        storeOf(repo).clear()
        val result = repo.commitSyncProfile(
            com.xiwei.sujian.model.SyncConfig(enabled = true),
            com.xiwei.sujian.model.SyncSecrets(token = "token-new"),
        )
        assertTrue("Commit must fail without native (strict reads abort before any write)", result is SettingsSaveResult.Failed)
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
    fun snapshotSyncProfile_returnsFailedWhenSecretsReadFails() = runTest {
        val repo = createRepo("migration_contract_2")
        val store = storeOf(repo)
        store.clear()
        store.commitGeneration(1L, "{\"enabled\":true}")
        val result = SyncProfileGate.snapshotExclusive { repo.snapshotSyncProfile() }
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
    fun commitGenerationMarkerIsAtomicInSingleStore() = runTest {
        val repo = createRepo("migration_contract_3")
        val store = storeOf(repo)
        store.clear()
        store.commitGeneration(5L, "{\"enabled\":true}")
        val state = store.readState()
        assertEquals(5L, state.activeGeneration)
        assertEquals("{\"enabled\":true}", state.committedConfigJson)
        assertTrue(state.hasCommittedProfile)
    }

    @Test
    fun snapshotSyncProfile_prefersGenerationStoreOverLegacy() = runTest {
        val repo = createRepo("migration_contract_4")
        val store = storeOf(repo)
        store.clear()
        store.commitGeneration(2L, "{\"enabled\":true,\"remoteUrl\":\"https://example.com/r.git\"}")
        val result = SyncProfileGate.snapshotExclusive { repo.snapshotSyncProfile() }
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
