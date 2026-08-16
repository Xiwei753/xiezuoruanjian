package com.xiwei.sujian.feature.sync.data

import android.content.Context
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.longPreferencesKey
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import com.xiwei.sujian.feature.sync.data.model.LegacyProfileMetadata
import com.xiwei.sujian.feature.sync.data.model.SyncConfig
import com.xiwei.sujian.feature.sync.data.model.SyncSecrets
import kotlinx.coroutines.flow.first

private val Context.syncProfileDataStore by preferencesDataStore(name = "sync_profile")

/**
 * #630 评论 #1：全局同步配置版本化提交的 DataStore 侧实现。
 *
 * 全应用只存在一份同步配置（取代旧的 `ProjectSyncProfileStore` + `AppSyncProfileStore`），
 * DataStore 文件名固定为 `sync_profile`，key 不带 projectId 前缀。提交语义：
 * stagedConfig(N) → stagedSecrets(N) → 原子 commitGeneration(N)，读取者只读取
 * activeGeneration 对应的完整版本（committed_config_json + 按 generation 保存的凭据），
 * 不会读到 staged 但未提交的 config/secrets。
 *
 * commit（[commitGeneration]）通过单次 `updateData` 原子更新 active_generation 与
 * committed_config_json，失败时旧 generation 继续有效。
 *
 * 本仓库只保存提交标记与恢复载荷，不复制 Core 的同步状态机；
 * 业务真相（config/secrets 内容）仍由 Rust Core 唯一持有。
 */
class SyncProfileStore(context: Context) {
    private val dataStore = context.applicationContext.syncProfileDataStore

    /** 当前提交状态 — 从 DataStore 单次读取。 */
    data class ProfileCommitState(
        val activeGeneration: Long = 0L,
        val committedConfigJson: String = "",
        val stagedConfigGeneration: Long = -1L,
        val stagedConfigJson: String = "",
        val stagedSecretsGeneration: Long = -1L,
    ) {
        /** 已存在完整提交版本（非纯 legacy 初始态）。 */
        val hasCommittedProfile: Boolean
            get() = activeGeneration > 0L && committedConfigJson.isNotEmpty()
    }

    /** 当前提交状态 — 从 DataStore 单次读取。 */
    suspend fun readState(): ProfileCommitState {
        val prefs = dataStore.data.first()
        return ProfileCommitState(
            activeGeneration = prefs[KEY_ACTIVE_GENERATION] ?: 0L,
            committedConfigJson = prefs[KEY_COMMITTED_CONFIG_JSON] ?: "",
            stagedConfigGeneration = prefs[KEY_STAGED_CONFIG_GENERATION] ?: -1L,
            stagedConfigJson = prefs[KEY_STAGED_CONFIG_JSON] ?: "",
            stagedSecretsGeneration = prefs[KEY_STAGED_SECRETS_GENERATION] ?: -1L,
        )
    }

    /** 下一个待提交 generation = max(active, staged)+1。 */
    suspend fun nextGeneration(): Long {
        val state = readState()
        return maxOf(
            state.activeGeneration,
            state.stagedConfigGeneration,
            state.stagedSecretsGeneration,
        ) + 1
    }

    /** 写 stagedConfig(generation=N) 标记与载荷（config 内容已写入 Core 配置存储）。 */
    suspend fun stageConfig(
        generation: Long,
        configJson: String,
    ) {
        dataStore.edit { prefs ->
            prefs[KEY_STAGED_CONFIG_GENERATION] = generation
            prefs[KEY_STAGED_CONFIG_JSON] = configJson
        }
    }

    /** 写 stagedSecrets(generation=N) 标记（凭据已按 generation 写入安全存储）。 */
    suspend fun stageSecrets(generation: Long) {
        dataStore.edit { prefs ->
            prefs[KEY_STAGED_SECRETS_GENERATION] = generation
        }
    }

    /**
     * 原子提交：单次 updateData 同时推进 activeGeneration 与 committedConfigJson。
     * 失败时旧 generation 继续有效（staged 标记停留在 N，读取者仍读旧 committed 版本）。
     */
    suspend fun commitGeneration(
        generation: Long,
        committedConfigJson: String,
    ) {
        dataStore.edit { prefs ->
            prefs[KEY_ACTIVE_GENERATION] = generation
            prefs[KEY_COMMITTED_CONFIG_JSON] = committedConfigJson
        }
    }

    /**
     * #595 五：清理崩溃遗留的未提交 staged 标记 — staged 标记不是
     * [ProfileCommitState.activeGeneration] 时清除（staged 但从未提交的 generation 在下次
     * 提交时会被覆盖，这里主动清理避免 DataStore 拗留）。
     */
    suspend fun clearStaleStagedMarkers(activeGeneration: Long) {
        dataStore.edit { prefs ->
            if ((prefs[KEY_STAGED_CONFIG_GENERATION] ?: -1L) != activeGeneration) {
                prefs.remove(KEY_STAGED_CONFIG_GENERATION)
                prefs.remove(KEY_STAGED_CONFIG_JSON)
            }
            if ((prefs[KEY_STAGED_SECRETS_GENERATION] ?: -1L) != activeGeneration) {
                prefs.remove(KEY_STAGED_SECRETS_GENERATION)
            }
        }
    }

    /** 测试隔离：清空全部提交状态。 */
    suspend fun clear() {
        dataStore.edit { prefs ->
            prefs.remove(KEY_ACTIVE_GENERATION)
            prefs.remove(KEY_COMMITTED_CONFIG_JSON)
            prefs.remove(KEY_STAGED_CONFIG_GENERATION)
            prefs.remove(KEY_STAGED_CONFIG_JSON)
            prefs.remove(KEY_STAGED_SECRETS_GENERATION)
        }
    }

    /**
     * #630 评论第 5 点 Part C-Android：读取旧作品级 profile 的精确 generation metadata。
     *
     * 旧 [ProjectSyncProfileStore] 与本仓库共用同一个 `sync_profile` DataStore 文件，
     * 但旧作品级 key 带 `<projectId>.` 前缀（如 `proj-1.active_generation`），
     * 新全局 key 不带前缀（`active_generation`），两者通过 key 是否包含 `.` 区分。
     *
     * 遍历 DataStore 所有 key，解析带前缀的 `<projectId>.active_generation` /
     * `<projectId>.committed_config_json`，只返回 committed_config_json 非空的 project
     * （无 committed config 的 project 不参与迁移）。active_generation 为 0 或缺失时
     * 返回 null（Core 回退 base key / 文件）。
     *
     * 只读，不写 DataStore，不删旧 key（清理由 Core 迁移成功后统一做）。
     */
    internal suspend fun readLegacyProjectMetadata(): List<LegacyProfileMetadata> {
        val prefs = dataStore.data.first()
        val projectIds = mutableSetOf<String>()
        for (key in prefs.asMap().keys) {
            val name = key.name
            val dotIndex = name.indexOf('.')
            if (dotIndex <= 0) continue
            val suffix = name.substring(dotIndex + 1)
            if (suffix == KEY_NAME_ACTIVE_GENERATION || suffix == KEY_NAME_COMMITTED_CONFIG_JSON) {
                projectIds.add(name.substring(0, dotIndex))
            }
        }
        if (projectIds.isEmpty()) return emptyList()
        val result = mutableListOf<LegacyProfileMetadata>()
        for (projectId in projectIds) {
            val committedConfigJson = prefs[stringPreferencesKey("$projectId.$KEY_NAME_COMMITTED_CONFIG_JSON")]
            if (committedConfigJson.isNullOrEmpty()) continue
            val activeGeneration = prefs[longPreferencesKey("$projectId.$KEY_NAME_ACTIVE_GENERATION")]
            result.add(
                LegacyProfileMetadata(
                    source = "project:$projectId",
                    projectId = projectId,
                    activeGeneration = activeGeneration?.takeIf { it > 0L },
                ),
            )
        }
        return result
    }

    companion object {
        private const val KEY_NAME_ACTIVE_GENERATION = "active_generation"
        private const val KEY_NAME_COMMITTED_CONFIG_JSON = "committed_config_json"
        private const val KEY_NAME_STAGED_CONFIG_GENERATION = "staged_config_generation"
        private const val KEY_NAME_STAGED_CONFIG_JSON = "staged_config_json"
        private const val KEY_NAME_STAGED_SECRETS_GENERATION = "staged_secrets_generation"

        private val KEY_ACTIVE_GENERATION = longPreferencesKey(KEY_NAME_ACTIVE_GENERATION)
        private val KEY_COMMITTED_CONFIG_JSON = stringPreferencesKey(KEY_NAME_COMMITTED_CONFIG_JSON)
        private val KEY_STAGED_CONFIG_GENERATION = longPreferencesKey(KEY_NAME_STAGED_CONFIG_GENERATION)
        private val KEY_STAGED_CONFIG_JSON = stringPreferencesKey(KEY_NAME_STAGED_CONFIG_JSON)
        private val KEY_STAGED_SECRETS_GENERATION = longPreferencesKey(KEY_NAME_STAGED_SECRETS_GENERATION)
    }
}

/**
 * #595 五：旧 generation 凭据清理范围 — 保留 current + previous 一个可回滚版本，
 * 删除更旧（< current - 2）generation 的凭据。
 *
 * 纯函数（无 native/DataStore 依赖），供
 * [SyncRepository.cleanupStaleGenerationCredentials] 决定删除边界：
 * - current = 1：无可删除 generation（只存在当前版本）；
 * - current = 2：保留 2 与 1（previous），无删除；
 * - current = 3：删除 1，保留 3 与 2；
 * - current = N：删除 1..N-2，保留 N 与 N-1。
 *
 * 返回 null 表示没有需要删除的 generation，调用方跳过删除循环。
 */
internal fun generationCleanupRange(current: Long): LongRange? {
    val lastToDelete = current - 2
    if (lastToDelete < 1L) return null
    return 1L..lastToDelete
}

/**
 * #630 评论 #1：一次全量同步操作使用的完整不可变配置快照。
 *
 * 正式同步、自动同步、试运行和连接诊断都先从统一仓库取得一次完整 snapshot，
 * 随后整个操作只使用这份 snapshot，不再从磁盘二次读取 config/secrets。
 * generation 来自 [SyncProfileStore]，config/secrets 来自全局 Core API。
 */
data class SyncProfileSnapshot(
    val generation: Long,
    val config: SyncConfig,
    val secrets: SyncSecrets,
)

/**
 * #595 五：generation 凭据读取的类型化结果 — 不再把"没有 token"和"读取失败"
 * 压成同一个 null。
 *
 * - [Found]：安全存储中存在有效凭据（token 非空）。
 * - [NotConfigured]：该 generation 没有凭据条目或 token 为空 — 用户确实未配置。
 * - [Failed]：安全存储读取失败、解密失败或原生库未加载 — 不得当作"未配置"。
 */
sealed interface GenerationSecretsReadResult {
    data class Found(val secrets: SyncSecrets) : GenerationSecretsReadResult

    data object NotConfigured : GenerationSecretsReadResult

    data class Failed(val kind: SyncFailureKind, val message: String?) : GenerationSecretsReadResult
}

/**
 * #595 五 / #630 评论 #1：全局同步配置完整快照读取的类型化结果。
 *
 * - [Found]：config + secrets 均成功读取，凭据非空。
 * - [NotConfigured]：config 读取成功，但凭据为空（用户未配置 token）。
 *   snapshot 仍携带 config 和空 secrets，调用方可据此显示"未配置"而非"错误"。
 * - [Failed]：config 解析失败或凭据读取失败 — 向设置页和同步状态返回类型化错误，
 *   不得转换成 [SyncSecrets] 或 null。
 */
sealed interface SyncProfileReadResult {
    data class Found(val snapshot: SyncProfileSnapshot) : SyncProfileReadResult

    data class NotConfigured(val snapshot: SyncProfileSnapshot) : SyncProfileReadResult

    data class Failed(val kind: SyncFailureKind, val message: String?) : SyncProfileReadResult
}
