package com.xiwei.sujian.core.interop.sync

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

// #600 评论 #4 问题三：测试用配置 JSON 常量 — 避免 StringLiteralDuplication。
private const val CONFIG_ENABLED_TRUE = "{\"enabled\":true}"
private const val CONFIG_ENABLED_FALSE = "{\"enabled\":false}"

/**
 * #600 评论 #4 问题三：AppSyncProfileStore 应用级版本化提交契约测试。
 *
 * 镜像 [SyncProfileGenerationTest]（作品级）的测试模式，验证应用级 store 的
 * generation + commit marker 行为：初始态、stageAndCommit 推进 activeGeneration、
 * 失败提交保留旧 generation、clearStaleStagedMarkers 清理崩溃遗留、clear 清空。
 *
 * 应用级 store 不带 projectId，使用独立 DataStore 文件 (`app_sync_profile`)，
 * 与作品级 store (`sync_profile`) 互不干扰。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class AppSyncProfileStoreTest {
    private lateinit var store: AppSyncProfileStore

    @Before
    fun setup() {
        val context: Context = ApplicationProvider.getApplicationContext()
        store = AppSyncProfileStore(context)
        kotlinx.coroutines.runBlocking {
            store.clear()
        }
    }

    @Test
    fun initial_state_isLegacyEmpty() =
        runTest {
            val state = store.readState()
            assertEquals(0L, state.activeGeneration)
            assertFalse(state.hasCommittedProfile)
        }

    @Test
    fun nextGeneration_incrementsFromActive() =
        runTest {
            assertEquals(1L, store.nextGeneration())
        }

    @Test
    fun stageAndCommit_advancesActiveGenerationAtomically() =
        runTest {
            store.stageConfig(1L, CONFIG_ENABLED_TRUE)
            store.stageSecrets(1L)
            store.commitGeneration(1L, CONFIG_ENABLED_TRUE)

            val state = store.readState()
            assertEquals(1L, state.activeGeneration)
            assertEquals(1L, state.stagedConfigGeneration)
            assertEquals(1L, state.stagedSecretsGeneration)
            assertTrue(state.hasCommittedProfile)
            assertEquals(CONFIG_ENABLED_TRUE, state.committedConfigJson)
        }

    @Test
    fun failedCommit_oldGenerationRemainsActive() =
        runTest {
            // 模拟 staged secrets 保存失败：只有 config 被 staged，activeGeneration 不推进。
            store.stageConfig(1L, CONFIG_ENABLED_TRUE)

            val state = store.readState()
            assertEquals(0L, state.activeGeneration)
            assertFalse(state.hasCommittedProfile)
            assertEquals(2L, store.nextGeneration())
        }

    @Test
    fun committedProfile_survivesFailedStagingOfNextGeneration() =
        runTest {
            // 第一次提交成功（generation 1）
            store.stageConfig(1L, CONFIG_ENABLED_TRUE)
            store.stageSecrets(1L)
            store.commitGeneration(1L, CONFIG_ENABLED_TRUE)
            // 第二次提交在 staged secrets 阶段失败：只有 config 被 staged（generation 2）
            store.stageConfig(2L, CONFIG_ENABLED_FALSE)

            val state = store.readState()
            assertEquals(1L, state.activeGeneration)
            assertTrue(state.hasCommittedProfile)
            assertEquals(CONFIG_ENABLED_TRUE, state.committedConfigJson)
        }

    @Test
    fun clearStaleStagedMarkers_removesCrashLeftoverUncommittedMarkers() =
        runTest {
            store.stageConfig(2L, CONFIG_ENABLED_FALSE)
            store.stageSecrets(2L)
            store.commitGeneration(3L, CONFIG_ENABLED_TRUE)
            store.clearStaleStagedMarkers(3L)

            val state = store.readState()
            assertEquals(-1L, state.stagedConfigGeneration)
            assertEquals("", state.stagedConfigJson)
            assertEquals(-1L, state.stagedSecretsGeneration)
            assertEquals(3L, state.activeGeneration)
            assertTrue(state.hasCommittedProfile)
        }

    @Test
    fun clearStaleStagedMarkers_keepsMarkersMatchingActiveGeneration() =
        runTest {
            store.stageConfig(1L, CONFIG_ENABLED_TRUE)
            store.stageSecrets(1L)
            store.commitGeneration(1L, CONFIG_ENABLED_TRUE)

            store.clearStaleStagedMarkers(1L)

            val state = store.readState()
            assertEquals(1L, state.stagedConfigGeneration)
            assertEquals(CONFIG_ENABLED_TRUE, state.stagedConfigJson)
            assertEquals(1L, state.stagedSecretsGeneration)
            assertEquals(1L, state.activeGeneration)
        }

    @Test
    fun clear_resetsToLegacyEmpty() =
        runTest {
            store.stageConfig(1L, CONFIG_ENABLED_TRUE)
            store.stageSecrets(1L)
            store.commitGeneration(1L, CONFIG_ENABLED_TRUE)
            assertTrue(store.readState().hasCommittedProfile)

            store.clear()

            val state = store.readState()
            assertEquals(0L, state.activeGeneration)
            assertFalse(state.hasCommittedProfile)
            assertEquals("", state.committedConfigJson)
            assertEquals(-1L, state.stagedConfigGeneration)
            assertEquals(-1L, state.stagedSecretsGeneration)
        }
}
