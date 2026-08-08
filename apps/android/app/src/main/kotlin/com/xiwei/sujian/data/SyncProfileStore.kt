package com.xiwei.sujian.data

import android.content.Context
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.longPreferencesKey
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import com.xiwei.sujian.model.SyncConfig
import com.xiwei.sujian.model.SyncSecrets
import kotlinx.coroutines.flow.first

private val Context.syncProfileDataStore by preferencesDataStore(name = "sync_profile")
private val Context.appSyncProfileDataStore by preferencesDataStore(name = "app_sync_profile")

/**
 * #592 五 / #600 评论 #3 问题二：作品级同步配置版本化提交的 DataStore 侧实现（per-project）。
 *
 * 单一 DataStore 文件保存所有作品的提交标记与恢复载荷，按 projectId 前缀区分 key：
 * - `<projectId>.active_generation`：当前完整提交的版本号（唯一 commit marker）
 * - `<projectId>.committed_config_json`：与 active_generation 在同一 updateData 中原子写入的
 *   已提交配置载荷 — 它永远只属于 activeGeneration，读取者据此读取完整版本
 * - `<projectId>.staged_config_generation` / `<projectId>.staged_config_json`：stagedConfig(N) 的标记与载荷
 * - `<projectId>.staged_secrets_generation`：stagedSecrets(N) 的标记（凭据本体在安全存储，按 generation 保存）
 *
 * commit（[commitGeneration]）通过单次 `updateData` 原子更新 active_generation 与
 * committed_config_json，失败时旧 generation 继续有效；读取者只读取
 * activeGeneration 对应的完整版本（committed_config_json + 按 generation 保存的凭据），
 * 不会读到 staged 但未提交的 config/secrets。
 *
 * 本仓库只保存提交标记与恢复载荷，不复制 Core 的同步状态机；
 * 业务真相（config/secrets 内容）仍由 Rust Core 唯一持有。
 *
 * #600 评论 #4 问题三：原 `SyncProfileStore` 重命名为 `ProjectSyncProfileStore`，
 * 与应用级 [AppSyncProfileStore] 区分 — 两者结构对称但语义独立（作品级按 projectId 隔离，
 * 应用级单一全局槽）。
 */
class ProjectSyncProfileStore(context: Context) {
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

    /** #600 评论 #3 问题二：所有读写按 projectId 隔离 — key 前缀 `<projectId>.`。 */
    suspend fun readState(projectId: String): ProfileCommitState {
        val prefs = dataStore.data.first()
        return ProfileCommitState(
            activeGeneration = prefs[prefActive(projectId)] ?: 0L,
            committedConfigJson = prefs[prefCommittedConfigJson(projectId)] ?: "",
            stagedConfigGeneration = prefs[prefStagedConfigGen(projectId)] ?: -1L,
            stagedConfigJson = prefs[prefStagedConfigJson(projectId)] ?: "",
            stagedSecretsGeneration = prefs[prefStagedSecretsGen(projectId)] ?: -1L,
        )
    }

    /** 下一个待提交 generation = max(active, staged)+1。 */
    suspend fun nextGeneration(projectId: String): Long {
        val state = readState(projectId)
        return maxOf(
            state.activeGeneration,
            state.stagedConfigGeneration,
            state.stagedSecretsGeneration,
        ) + 1
    }

    /** 写 stagedConfig(generation=N) 标记与载荷（config 内容已写入 Core 配置存储）。 */
    suspend fun stageConfig(
        projectId: String,
        generation: Long,
        configJson: String,
    ) {
        dataStore.edit { prefs ->
            prefs[prefStagedConfigGen(projectId)] = generation
            prefs[prefStagedConfigJson(projectId)] = configJson
        }
    }

    /** 写 stagedSecrets(generation=N) 标记（凭据已按 generation 写入安全存储）。 */
    suspend fun stageSecrets(
        projectId: String,
        generation: Long,
    ) {
        dataStore.edit { prefs ->
            prefs[prefStagedSecretsGen(projectId)] = generation
        }
    }

    /**
     * 原子提交：单次 updateData 同时推进 activeGeneration 与 committedConfigJson。
     * 失败时旧 generation 继续有效（staged 标记停留在 N，读取者仍读旧 committed 版本）。
     */
    suspend fun commitGeneration(
        projectId: String,
        generation: Long,
        committedConfigJson: String,
    ) {
        dataStore.edit { prefs ->
            prefs[prefActive(projectId)] = generation
            prefs[prefCommittedConfigJson(projectId)] = committedConfigJson
        }
    }

    /**
     * #595 五：清理崩溃遗留的未提交 staged 标记 — staged 标记不是
     * [ProfileCommitState.activeGeneration] 时清除（staged 但从未提交的 generation 在下次
     * 提交时会被覆盖，这里主动清理避免 DataStore 拗留）。
     */
    suspend fun clearStaleStagedMarkers(
        projectId: String,
        activeGeneration: Long,
    ) {
        dataStore.edit { prefs ->
            if ((prefs[prefStagedConfigGen(projectId)] ?: -1L) != activeGeneration) {
                prefs.remove(prefStagedConfigGen(projectId))
                prefs.remove(prefStagedConfigJson(projectId))
            }
            if ((prefs[prefStagedSecretsGen(projectId)] ?: -1L) != activeGeneration) {
                prefs.remove(prefStagedSecretsGen(projectId))
            }
        }
    }

    /** 测试隔离：清空指定作品的全部提交状态。 */
    suspend fun clear(projectId: String) {
        dataStore.edit { prefs ->
            prefs.remove(prefActive(projectId))
            prefs.remove(prefCommittedConfigJson(projectId))
            prefs.remove(prefStagedConfigGen(projectId))
            prefs.remove(prefStagedConfigJson(projectId))
            prefs.remove(prefStagedSecretsGen(projectId))
        }
    }

    companion object {
        private const val KEY_ACTIVE_GENERATION = "active_generation"
        private const val KEY_COMMITTED_CONFIG_JSON = "committed_config_json"
        private const val KEY_STAGED_CONFIG_GENERATION = "staged_config_generation"
        private const val KEY_STAGED_CONFIG_JSON = "staged_config_json"
        private const val KEY_STAGED_SECRETS_GENERATION = "staged_secrets_generation"

        /** #600 评论 #3 问题二：按 projectId 前缀生成 key，不同作品的标记互不干扰。 */
        private fun prefActive(projectId: String) = longPreferencesKey("$projectId.$KEY_ACTIVE_GENERATION")

        private fun prefCommittedConfigJson(projectId: String) =
            stringPreferencesKey("$projectId.$KEY_COMMITTED_CONFIG_JSON")

        private fun prefStagedConfigGen(projectId: String) =
            longPreferencesKey("$projectId.$KEY_STAGED_CONFIG_GENERATION")

        private fun prefStagedConfigJson(projectId: String) = stringPreferencesKey("$projectId.$KEY_STAGED_CONFIG_JSON")

        private fun prefStagedSecretsGen(projectId: String) =
            longPreferencesKey("$projectId.$KEY_STAGED_SECRETS_GENERATION")
    }
}

/**
 * #600 评论 #4 问题三：应用级同步配置版本化提交的 DataStore 侧实现。
 *
 * 与 [ProjectSyncProfileStore] 结构对称，但**不带 projectId** —
 * 应用级同步目标唯一（设置/全局星图/主题调色板），使用独立 DataStore 文件
 * (`app_sync_profile`)，key 不带前缀。提交语义与作品级一致：
 * stagedConfig(N) → stagedSecrets(N) → 原子 commitGeneration(N)，
 * 读取者只读取 activeGeneration 对应的完整版本。
 *
 * 业务真相（config/secrets 内容）仍由 Rust Core 唯一持有；
 * 本仓库只保存提交标记与恢复载荷，不复制 Core 的同步状态机。
 */
class AppSyncProfileStore(context: Context) {
    private val dataStore = context.applicationContext.appSyncProfileDataStore

    /** 当前提交状态 — 从 DataStore 单次读取。复用 [ProjectSyncProfileStore.ProfileCommitState] 结构。 */
    suspend fun readState(): ProjectSyncProfileStore.ProfileCommitState {
        val prefs = dataStore.data.first()
        return ProjectSyncProfileStore.ProfileCommitState(
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

    /** 写 stagedConfig(generation=N) 标记与载荷（config 内容已写入 Core 应用级配置存储）。 */
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
     * 失败时旧 generation 继续有效。
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
     * 清理崩溃遗留的未提交 staged 标记 — staged 标记不是 activeGeneration 时清除。
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

    /** 测试隔离：清空全部应用级提交状态。 */
    suspend fun clear() {
        dataStore.edit { prefs ->
            prefs.remove(KEY_ACTIVE_GENERATION)
            prefs.remove(KEY_COMMITTED_CONFIG_JSON)
            prefs.remove(KEY_STAGED_CONFIG_GENERATION)
            prefs.remove(KEY_STAGED_CONFIG_JSON)
            prefs.remove(KEY_STAGED_SECRETS_GENERATION)
        }
    }

    companion object {
        private val KEY_ACTIVE_GENERATION = longPreferencesKey("active_generation")
        private val KEY_COMMITTED_CONFIG_JSON = stringPreferencesKey("committed_config_json")
        private val KEY_STAGED_CONFIG_GENERATION = longPreferencesKey("staged_config_generation")
        private val KEY_STAGED_CONFIG_JSON = stringPreferencesKey("staged_config_json")
        private val KEY_STAGED_SECRETS_GENERATION = longPreferencesKey("staged_secrets_generation")
    }
}

/**
 * #595 五：旧 generation 凭据清理范围 — 保留 current + previous 一个可回滚版本，
 * 删除更旧（< current - 1）generation 的凭据。
 *
 * 纯函数（无 native/DataStore 依赖），供
 * [SettingsRepository.cleanupStaleGenerationCredentials] 决定删除边界：
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
 * #592 六：作品级一次同步操作使用的完整不可变配置快照。
 *
 * 正式同步、自动同步、试运行和连接诊断都先从统一仓库取得一次完整 snapshot，
 * 随后整个操作只使用这份 snapshot，不再从磁盘二次读取 config/secrets。
 *
 * #600 评论 #4 问题三：原 `SyncProfileSnapshot` 重命名为 `ProjectSyncProfileSnapshot`，
 * 与应用级 [AppSyncProfileSnapshot] 区分。
 */
data class ProjectSyncProfileSnapshot(
    val generation: Long,
    val config: SyncConfig,
    val secrets: SyncSecrets,
)

/**
 * #600 评论 #4 问题三：应用级一次同步操作使用的完整不可变配置快照。
 *
 * 与 [ProjectSyncProfileSnapshot] 结构对称，但 generation 来自 [AppSyncProfileStore]，
 * config/secrets 来自应用级 Core API（`loadAppSyncConfig` / 应用级 generation 凭据）。
 */
data class AppSyncProfileSnapshot(
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
 * #595 五：作品级同步配置完整快照读取的类型化结果。
 *
 * - [Found]：config + secrets 均成功读取，凭据非空。
 * - [NotConfigured]：config 读取成功，但凭据为空（用户未配置 token）。
 *   snapshot 仍携带 config 和空 secrets，调用方可据此显示"未配置"而非"错误"。
 * - [Failed]：config 解析失败或凭据读取失败 — 向设置页和同步状态返回类型化错误，
 *   不得转换成 [SyncSecrets] 或 null。
 *
 * #600 评论 #4 问题三：`Found/NotConfigured` 持有 [ProjectSyncProfileSnapshot]。
 */
sealed interface SyncProfileReadResult {
    data class Found(val snapshot: ProjectSyncProfileSnapshot) : SyncProfileReadResult

    data class NotConfigured(val snapshot: ProjectSyncProfileSnapshot) : SyncProfileReadResult

    data class Failed(val kind: SyncFailureKind, val message: String?) : SyncProfileReadResult
}

/**
 * #600 评论 #4 问题三：应用级同步配置完整快照读取的类型化结果。
 *
 * 与 [SyncProfileReadResult] 结构对称，但持有 [AppSyncProfileSnapshot]。
 * 独立 sealed interface 避免泛型化扩散到所有调用方。
 *
 * - [Found]：config + secrets 均成功读取，凭据非空。
 * - [NotConfigured]：config 读取成功，但凭据为空（用户未配置 token）。
 * - [Failed]：config 解析失败或凭据读取失败 — 类型化错误，不得转成 null。
 */
sealed interface AppSyncProfileReadResult {
    data class Found(val snapshot: AppSyncProfileSnapshot) : AppSyncProfileReadResult

    data class NotConfigured(val snapshot: AppSyncProfileSnapshot) : AppSyncProfileReadResult

    data class Failed(val kind: SyncFailureKind, val message: String?) : AppSyncProfileReadResult
}
