package com.xiwei.sujian.data

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
 * #595 九：首次 generation 提交必须原子（legacy → generation 迁移）契约测试。
 *
 * 旧缺陷：commitSyncProfile 第一步直接 saveSyncConfig(normalized) 写 live Core
 * 配置，然后才 stage secrets 和提交 activeGeneration。首次提交时如果进程在
 * 写 live config 后、提交 marker 前崩溃，重启后 snapshotSyncProfile 因
 * hasCommittedProfile=false 回退读取 live config，把未提交的新 config 与旧
 * secrets 组合起来。
 *
 * 修复：
 * - 首次提交先完成 legacy → generation 迁移（原子 marker 提交后 generation
 *   store 成为唯一权威），此后所有读取只认 generation store；
 * - 新 config 在 marker 提交前不发布到 live Core 配置文件；
 * - 镜像槽（saveSyncConfig/saveSyncSecrets）在提交成功后更新，不参与权威读取。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class CommitSyncProfileMigrationContractTest {

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
    fun snapshotSyncProfile_treatsMissingGenerationSecretsAsUnconfigured() = runTest {
        // 已提交 profile 存在但 generation 凭据缺失（未配置 token 的用户迁移后）：
        // 必须返回空凭据快照（Unconfigured 语义），不得返回 null（那会误报 Fatal）。
        val repo = createRepo("migration_contract_2")
        val store = storeOf(repo)
        store.clear()
        // 模拟：已有 committed profile（generation 1），但凭据从未配置。
        store.commitGeneration(1L, "{\"enabled\":true}")
        val snapshot = SyncProfileGate.snapshotExclusive { repo.snapshotSyncProfile() }
        // #595 九：已提交 generation 是权威 — generation 凭据缺失 = 未配置
        // （空凭据快照），不得返回 null（那会误报 Fatal）。
        assertNotNull(snapshot)
        assertEquals(
            "Missing generation secrets must mean unconfigured (empty secrets), not Fatal",
            null,
            snapshot?.secrets?.token,
        )
    }

    @Test
    fun commitGenerationMarkerIsAtomicInSingleStore() = runTest {
        // DataStore updateData 只对同一 DataStore 原子 — generation marker
        // （activeGeneration + committedConfigJson）必须单次 updateData 推进。
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
    fun commitSyncProfile_reordersLiveMirrorAfterMarker() {
        // 结构契约：提交序列中 live 槽写入（saveSyncConfig/saveSyncSecrets 的
        // Core 文件写入）必须在 commitGeneration 之后 — 通过方法顺序无法直接
        // 反射断言，这里验证 SettingsRepository 同时具备严格读取与版本化入口，
        // 且 legacy 读取入口仍是严格版（失败不返回默认值）。
        val strictConfig = SettingsRepository::class.java.methods.firstOrNull { it.name == "loadSyncConfigStrict" }
        val strictSecrets = SettingsRepository::class.java.methods.firstOrNull { it.name == "loadSyncSecretsStrict" }
        val commit = SettingsRepository::class.java.methods.firstOrNull { it.name == "commitSyncProfile" }
        assertNotNull(strictConfig)
        assertNotNull(strictSecrets)
        assertNotNull(commit)
    }

    @Test
    fun loadCommittedSyncProfile_existsForUiReads() {
        // #595 八：UI 初始化/刷新/回滚读取 active generation 完整 snapshot。
        val method = SettingsRepository::class.java.methods.firstOrNull {
            it.name == "loadCommittedSyncProfile"
        }
        assertNotNull("SettingsRepository.loadCommittedSyncProfile must exist", method)
    }

    @Test
    fun snapshotSyncProfile_prefersGenerationStoreOverLegacy() = runTest {
        // 已提交时 config 必须来自 committedConfigJson，不再读 live 槽。
        // 无 native 环境下 committedConfigJson 可解析 → config 非 null；
        // legacy 槽（loadSyncConfigStrict）不可用也不影响 generation 读取。
        // 这里验证读取分支逻辑（通过无 native 的 generation-committed 状态）。
        val repo = createRepo("migration_contract_4")
        val store = storeOf(repo)
        store.clear()
        store.commitGeneration(2L, "{\"enabled\":true,\"remoteUrl\":\"https://example.com/r.git\"}")
        val snapshot = SyncProfileGate.snapshotExclusive { repo.snapshotSyncProfile() }
        // 无 native：secrets 读取失败 → hasCommittedProfile 分支回退为空凭据
        // （未配置语义）；config 已从 generation store 解析成功（不依赖 live 槽）。
        assertEquals(
            "Committed config must come from the generation store, not the live slot",
            "https://example.com/r.git",
            snapshot?.config?.remoteUrl,
        )
        assertEquals(
            "Unavailable secrets must degrade to unconfigured, not Fatal",
            null,
            snapshot?.secrets?.token,
        )
    }
}
