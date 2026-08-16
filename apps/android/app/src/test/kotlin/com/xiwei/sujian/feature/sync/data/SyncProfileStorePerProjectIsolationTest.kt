package com.xiwei.sujian.feature.sync.data

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

/**
 * #630 评论 #1：全局 SyncProfileStore 契约测试。
 *
 * 全应用只存在一份同步配置（取代旧的 per-project 隔离）。本测试验证全局 store 的
 * generation + commit marker 行为：初始态、stageAndCommit 推进 activeGeneration、
 * clearStaleStagedMarkers 清理崩溃遗留、clear 清空。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class SyncProfileStorePerProjectIsolationTest {
    private lateinit var store: SyncProfileStore
    private val configEnabledTrue = "{\"enabled\":true}"
    private val configEnabledFalse = "{\"enabled\":false}"

    @Before
    fun setup() {
        val context: Context = ApplicationProvider.getApplicationContext()
        store = SyncProfileStore(context)
        kotlinx.coroutines.runBlocking {
            store.clear()
        }
    }

    @Test
    fun initialState_isLegacyEmpty() =
        runTest {
            val state = store.readState()
            assertEquals(0L, state.activeGeneration)
            assertFalse(state.hasCommittedProfile)
        }

    @Test
    fun stageAndCommit_advancesActiveGeneration() =
        runTest {
            store.stageConfig(1L, configEnabledTrue)
            store.stageSecrets(1L)
            store.commitGeneration(1L, configEnabledTrue)

            val state = store.readState()
            assertEquals(1L, state.activeGeneration)
            assertTrue(state.hasCommittedProfile)
            assertEquals(configEnabledTrue, state.committedConfigJson)
        }

    @Test
    fun nextGeneration_advancesFromActive() =
        runTest {
            store.stageConfig(3L, configEnabledTrue)
            store.stageSecrets(3L)
            store.commitGeneration(3L, configEnabledTrue)
            assertEquals(4L, store.nextGeneration())
        }

    @Test
    fun clear_resetsToLegacyEmpty() =
        runTest {
            store.commitGeneration(1L, configEnabledTrue)
            store.clear()

            val state = store.readState()
            assertEquals(0L, state.activeGeneration)
            assertFalse(state.hasCommittedProfile)
        }

    @Test
    fun clearStaleStagedMarkers_removesStaleEgeNotMatchingActive() =
        runTest {
            // 拗留 staged=2 但 active=3
            store.stageConfig(2L, configEnabledFalse)
            store.stageSecrets(2L)
            store.commitGeneration(3L, configEnabledTrue)

            // 清理拗留标记（active=3，staged=2 应清除）
            store.clearStaleStagedMarkers(3L)

            val state = store.readState()
            assertEquals(-1L, state.stagedConfigGeneration)
            assertEquals(-1L, state.stagedSecretsGeneration)
        }

    @Test
    fun clearStaleStagedMarkers_keepsStagedMatchingActive() =
        runTest {
            // 正常提交后 staged=2 与 active=2 一致
            store.stageConfig(2L, configEnabledTrue)
            store.stageSecrets(2L)
            store.commitGeneration(2L, configEnabledTrue)

            // 清理（active=2，staged=2 应保留）
            store.clearStaleStagedMarkers(2L)

            val state = store.readState()
            assertEquals(2L, state.stagedConfigGeneration)
            assertEquals(2L, state.stagedSecretsGeneration)
        }

    @Test
    fun stageConfig_storesConfigJson() =
        runTest {
            store.stageConfig(1L, configEnabledTrue)

            val state = store.readState()
            assertEquals(1L, state.stagedConfigGeneration)
            assertEquals(configEnabledTrue, state.stagedConfigJson)
        }
}
