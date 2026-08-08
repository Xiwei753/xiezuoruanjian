package com.xiwei.sujian.core.interop.sync

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #600 评论 #3 问题二：ProjectSyncProfileStore per-project 隔离契约测试。
 *
 * 验证两个不同 projectId 的 ProjectSyncProfileStore 互不干扰：
 * - 作品 A 的 commit/staged 标记不影响作品 B 的 readState；
 * - key 前缀 `<projectId>.` 保证不同作品的标记在同一个 DataStore 文件中独立。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class SyncProfileStorePerProjectIsolationTest {
    private lateinit var store: ProjectSyncProfileStore
    private val projectA = "project-uuid-a"
    private val projectB = "project-uuid-b"
    private val configEnabledTrue = "{\"enabled\":true}"
    private val configEnabledFalse = "{\"enabled\":false}"
    private val configEnabledTrueUrlA = "{\"enabled\":true,\"remoteUrl\":\"https://a.git\"}"

    @Before
    fun setup() {
        val context: Context = ApplicationProvider.getApplicationContext()
        store = ProjectSyncProfileStore(context)
        kotlinx.coroutines.runBlocking {
            store.clear(projectA)
            store.clear(projectB)
        }
    }

    @Test
    fun initialState_bothProjectsAreLegacyEmpty() =
        runTest {
            val stateA = store.readState(projectA)
            val stateB = store.readState(projectB)
            assertEquals(0L, stateA.activeGeneration)
            assertEquals(0L, stateB.activeGeneration)
            assertFalse(stateA.hasCommittedProfile)
            assertFalse(stateB.hasCommittedProfile)
        }

    @Test
    fun commitProjectA_doesNotAffectProjectB() =
        runTest {
            // 作品 A 提交 generation 1
            store.stageConfig(projectA, 1L, configEnabledTrueUrlA)
            store.stageSecrets(projectA, 1L)
            store.commitGeneration(projectA, 1L, configEnabledTrueUrlA)

            // 作品 B 仍是初始态
            val stateA = store.readState(projectA)
            val stateB = store.readState(projectB)
            assertEquals(1L, stateA.activeGeneration)
            assertTrue(stateA.hasCommittedProfile)
            assertEquals(0L, stateB.activeGeneration)
            assertFalse(stateB.hasCommittedProfile)
        }

    @Test
    fun commitBothProjects_independentGenerations() =
        runTest {
            // 作品 A 提交 generation 1
            store.stageConfig(projectA, 1L, configEnabledTrue)
            store.stageSecrets(projectA, 1L)
            store.commitGeneration(projectA, 1L, configEnabledTrue)
            // 作品 B 提交 generation 1（独立计数）
            store.stageConfig(projectB, 1L, configEnabledFalse)
            store.stageSecrets(projectB, 1L)
            store.commitGeneration(projectB, 1L, configEnabledFalse)

            val stateA = store.readState(projectA)
            val stateB = store.readState(projectB)
            assertEquals(1L, stateA.activeGeneration)
            assertEquals(configEnabledTrue, stateA.committedConfigJson)
            assertEquals(1L, stateB.activeGeneration)
            assertEquals(configEnabledFalse, stateB.committedConfigJson)
            assertNotEquals(stateA.committedConfigJson, stateB.committedConfigJson)
        }

    @Test
    fun nextGeneration_independentPerProject() =
        runTest {
            // 作品 A 提交到 generation 3
            store.stageConfig(projectA, 3L, configEnabledTrue)
            store.stageSecrets(projectA, 3L)
            store.commitGeneration(projectA, 3L, configEnabledTrue)
            // 作品 B 仍是初始态
            assertEquals(4L, store.nextGeneration(projectA))
            assertEquals(1L, store.nextGeneration(projectB))
        }

    @Test
    fun clearProjectA_doesNotAffectProjectB() =
        runTest {
            // 两个作品都提交
            store.commitGeneration(projectA, 1L, configEnabledTrue)
            store.commitGeneration(projectB, 2L, configEnabledFalse)
            // 清空作品 A
            store.clear(projectA)

            val stateA = store.readState(projectA)
            val stateB = store.readState(projectB)
            assertEquals(0L, stateA.activeGeneration)
            assertFalse(stateA.hasCommittedProfile)
            // 作品 B 不受影响
            assertEquals(2L, stateB.activeGeneration)
            assertTrue(stateB.hasCommittedProfile)
        }

    @Test
    fun clearStaleStagedMarkers_isolatedPerProject() =
        runTest {
            // 作品 A 残留 staged=2 但 active=3
            store.stageConfig(projectA, 2L, configEnabledFalse)
            store.stageSecrets(projectA, 2L)
            store.commitGeneration(projectA, 3L, configEnabledTrue)
            // 作品 B staged=2 与 active=2 一致（正常提交后）
            store.stageConfig(projectB, 2L, configEnabledTrue)
            store.stageSecrets(projectB, 2L)
            store.commitGeneration(projectB, 2L, configEnabledTrue)

            // 清理作品 A 的拗留标记（active=3，staged=2 应清除）
            store.clearStaleStagedMarkers(projectA, 3L)
            // 清理作品 B（active=2，staged=2 应保留）
            store.clearStaleStagedMarkers(projectB, 2L)

            val stateA = store.readState(projectA)
            val stateB = store.readState(projectB)
            // A 的拗留 staged 被清除
            assertEquals(-1L, stateA.stagedConfigGeneration)
            assertEquals(-1L, stateA.stagedSecretsGeneration)
            // B 的正常 staged 保留
            assertEquals(2L, stateB.stagedConfigGeneration)
            assertEquals(2L, stateB.stagedSecretsGeneration)
        }

    @Test
    fun stageConfig_isolatedPerProject() =
        runTest {
            store.stageConfig(projectA, 1L, configEnabledTrue)
            store.stageConfig(projectB, 5L, configEnabledFalse)

            val stateA = store.readState(projectA)
            val stateB = store.readState(projectB)
            assertEquals(1L, stateA.stagedConfigGeneration)
            assertEquals(configEnabledTrue, stateA.stagedConfigJson)
            assertEquals(5L, stateB.stagedConfigGeneration)
            assertEquals(configEnabledFalse, stateB.stagedConfigJson)
        }
}
