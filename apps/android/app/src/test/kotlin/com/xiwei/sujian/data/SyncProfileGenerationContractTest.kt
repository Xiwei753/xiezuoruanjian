package com.xiwei.sujian.data

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
 * #592 五：同步配置 generation + commit marker 契约测试。
 *
 * 验证 stagedConfig(generation=N) → stagedSecrets(generation=N) →
 * 两项成功后原子更新 activeGeneration=N；失败时旧 generation 继续有效；
 * 读取者只读取 activeGeneration 对应的完整版本。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class SyncProfileGenerationContractTest {

    private lateinit var store: SyncProfileStore

    @Before
    fun setup() {
        val context: Context = ApplicationProvider.getApplicationContext()
        store = SyncProfileStore(context)
        // DataStore 在同进程测试间持久化，每个测试前清空以保证独立。
        kotlinx.coroutines.runBlocking {
            store.clear()
        }
    }

    @Test
    fun initial_state_isLegacyEmpty() = runTest {
        val state = store.readState()
        assertEquals(0L, state.activeGeneration)
        assertFalse(state.hasCommittedProfile)
    }

    @Test
    fun nextGeneration_incrementsFromActive() = runTest {
        assertEquals(1L, store.nextGeneration())
    }

    @Test
    fun stageAndCommit_advancesActiveGenerationAtomically() = runTest {
        store.stageConfig(1L, "{\"enabled\":true}")
        store.stageSecrets(1L)
        store.commitGeneration(1L, "{\"enabled\":true}")

        val state = store.readState()
        assertEquals(1L, state.activeGeneration)
        assertEquals(1L, state.stagedConfigGeneration)
        assertEquals(1L, state.stagedSecretsGeneration)
        assertTrue(state.hasCommittedProfile)
        // 读取者只读取 activeGeneration 对应的完整版本
        assertEquals("{\"enabled\":true}", state.committedConfigJson)
    }

    @Test
    fun failedCommit_oldGenerationRemainsActive() = runTest {
        // 模拟 staged secrets 保存失败：只有 config 被 staged，activeGeneration 不推进。
        store.stageConfig(1L, "{\"enabled\":true}")
        // 没有 stageSecrets/commitGeneration — 提交失败路径

        val state = store.readState()
        assertEquals(0L, state.activeGeneration)
        assertFalse(state.hasCommittedProfile)
        // 下一次提交从失败的 generation 之后继续，不会复用已失败版本
        assertEquals(2L, store.nextGeneration())
    }

    @Test
    fun committedProfile_survivesFailedStagingOfNextGeneration() = runTest {
        // 第一次提交成功（generation 1）
        store.stageConfig(1L, "{\"enabled\":true}")
        store.stageSecrets(1L)
        store.commitGeneration(1L, "{\"enabled\":true}")
        // 第二次提交在 staged secrets 阶段失败：只有 config 被 staged（generation 2）
        store.stageConfig(2L, "{\"enabled\":false}")

        val state = store.readState()
        // 旧 generation 继续有效，读取者仍读到 generation 1 的完整版本
        assertEquals(1L, state.activeGeneration)
        assertTrue(state.hasCommittedProfile)
        assertEquals("{\"enabled\":true}", state.committedConfigJson)
    }

    @Test
    fun nextGeneration_skipsStagedUncommitted() = runTest {
        store.stageConfig(1L, "{}")
        store.stageSecrets(1L)
        // 未 commit（进程死亡模拟）
        assertEquals(2L, store.nextGeneration())
    }
}
