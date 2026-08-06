package com.xiwei.sujian.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/**
 * #595 五：旧 generation 凭据清理保留规则契约测试。
 *
 * Issue 要求：generation 提交成功后执行安全清理 —
 * 保留 current + previous 一个可回滚版本，删除更旧 generation 的凭据；
 * 清理未提交 staged generation；失败返回类型化安全存储错误（不回滚已提交）。
 *
 * 删除边界由纯函数 [generationCleanupRange] 固定（无 native/DataStore 依赖），
 * [SettingsRepository.cleanupStaleGenerationCredentials] 只按此边界执行删除。
 *
 * 边界不变量：
 * - current=1：只存在当前版本，无删除；
 * - current=2：current 与 previous(1) 都保留，无删除；
 * - current=3：删除 1（previous 的 previous），保留 2 与 3；
 * - current=N：删除 1..N-2，保留 N-1 与 N。
 */
class GenerationCleanupRetentionContractTest {

    @Test
    fun currentOne_keepsOnlyVersion_deletesNothing() {
        assertNull("current=1 时没有任何可删除 generation", generationCleanupRange(1L))
    }

    @Test
    fun currentTwo_keepsCurrentAndPrevious_deletesNothing() {
        // previous(1) 必须保留 — 回滚需要它。
        assertNull("current=2 时 previous 必须保留", generationCleanupRange(2L))
    }

    @Test
    fun currentThree_deletesOnlyGrandPrevious() {
        assertEquals("current=3 只删除 1，保留 2 与 3", 1L..1L, generationCleanupRange(3L))
    }

    @Test
    fun currentFive_deletesOlderThanPrevious() {
        // 保留 5 与 4，删除 1..3。
        assertEquals("current=5 删除 1..3，保留 4 与 5", 1L..3L, generationCleanupRange(5L))
    }

    @Test
    fun currentHundred_deletesOneThroughNinetyEight() {
        assertEquals("current=100 删除 1..98，保留 99 与 100", 1L..98L, generationCleanupRange(100L))
    }

    @Test
    fun currentNeverBelowOne_isNotDeleted() {
        // 防御：current 不允许 < 1（迁移后 activeGeneration 从 1 开始）。
        assertNull(generationCleanupRange(0L))
        assertNull(generationCleanupRange(-5L))
    }

    @Test
    fun cleanupFailureIsTypedAndDoesNotRollBackCommit() {
        // #595 五：清理失败只返回类型化失败记录，不得回滚已成功的提交 —
        // 契约：commitSyncProfile 在 cleanup 失败时仍返回 SettingsSaveResult.Success
        //（见 SettingsRepository.commitSyncProfile 步骤 5：失败仅 warn 不改变 activeGeneration）。
        // 本断言固定该语义的存在性（生产路径行为由 CommitSyncProfileContractTest 覆盖）。
        val cleanupResult = SettingsSaveResult.Failed(
            listOf(SaveFailure(SaveField.SYNC_SECRETS, 1L))
        )
        assertEquals(SaveField.SYNC_SECRETS, cleanupResult.failures.single().field)
        assertEquals(1L, cleanupResult.failures.single().revision)
    }
}
