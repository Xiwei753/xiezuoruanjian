package com.xiwei.sujian.core.interop.sync

import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #592 五/六：版本化提交与完整快照行为测试。
 *
 * 结构契约（方法存在性）已移入
 * [com.xiwei.sujian.arch.CommitSyncProfileRollbackArchitectureTest]；本文件只保留运行时行为：
 * - ProjectSyncProfileSnapshot 携带 generation/config/secrets。
 */
class CommitSyncProfileRollbackTest {
    @Test
    fun syncProfileSnapshot_carriesGenerationConfigSecrets() {
        val snapshot =
            ProjectSyncProfileSnapshot(
                generation = 3L,
                config = com.xiwei.sujian.feature.sync.data.model.SyncConfig(enabled = true),
                secrets = com.xiwei.sujian.feature.sync.data.model.SyncSecrets(token = "t"),
            )
        assertTrue(snapshot.generation == 3L)
        assertTrue(snapshot.config.enabled == true)
        assertTrue(snapshot.secrets.token == "t")
    }
}
