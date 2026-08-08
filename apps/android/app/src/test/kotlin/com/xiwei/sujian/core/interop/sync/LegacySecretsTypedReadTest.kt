package com.xiwei.sujian.core.interop.sync
import com.xiwei.sujian.feature.sync.data.SyncRepository
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #595 四：legacy 槽凭据读取的类型化行为测试。
 *
 * 结构契约（方法存在性）已移入
 * [com.xiwei.sujian.arch.LegacySecretsTypedReadArchitectureTest]；本文件只保留运行时行为：
 * - 原生库未加载返回 Failed(NativeUnavailable)，不伪装成 NotConfigured；
 * - snapshotSyncProfile 不把读取失败掩盖成 NotConfigured。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class LegacySecretsTypedReadTest {
    private fun createRepo(): SyncRepository {
        val context = org.robolectric.RuntimeEnvironment.getApplication()
        return SyncRepository(context)
    }

    @Test
    fun nativeUnavailable_isFailedNotNotConfigured() {
        val result = createRepo().loadLegacySyncSecretsTyped("legacy-test-project")
        assertTrue(
            "原生库未加载必须返回 Failed(NativeUnavailable)，不得伪装成 NotConfigured，实际: $result",
            result is GenerationSecretsReadResult.Failed &&
                (result as GenerationSecretsReadResult.Failed).kind == SyncFailureKind.NativeUnavailable,
        )
    }

    @Test
    fun snapshotSyncProfile_neverMasksReadFailureAsNotConfigured() {
        val result =
            kotlinx.coroutines.runBlocking {
                SyncProfileGate.snapshotExclusive {
                    createRepo().snapshotSyncProfile("legacy-test-project")
                }
            }
        assertTrue(
            "读取失败必须返回 SyncProfileReadResult.Failed，不得伪装成 NotConfigured，实际: $result",
            result is SyncProfileReadResult.Failed,
        )
        assertTrue(
            "读取失败不是 NotConfigured",
            result !is SyncProfileReadResult.NotConfigured,
        )
    }
}
