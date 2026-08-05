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

/**
 * #592 五：同步配置版本化提交的 DataStore 侧实现。
 *
 * 单一 DataStore 文件保存：
 * - active_generation：当前完整提交的版本号（唯一 commit marker）
 * - committed_config_json：与 active_generation 在同一 updateData 中原子写入的
 *   已提交配置载荷 — 它永远只属于 activeGeneration，读取者据此读取完整版本
 * - staged_config_generation / staged_config_json：stagedConfig(N) 的标记与载荷
 * - staged_secrets_generation：stagedSecrets(N) 的标记（凭据本体在安全存储，按 generation 保存）
 *
 * commit（[commitGeneration]）通过单次 `updateData` 原子更新 active_generation 与
 * committed_config_json，失败时旧 generation 继续有效；读取者只读取
 * activeGeneration 对应的完整版本（committed_config_json + 按 generation 保存的凭据），
 * 不会读到 staged 但未提交的 config/secrets。
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

    suspend fun readState(): ProfileCommitState {
        val prefs = dataStore.data.first()
        return ProfileCommitState(
            activeGeneration = prefs[PREF_ACTIVE] ?: 0L,
            committedConfigJson = prefs[PREF_COMMITTED_CONFIG_JSON] ?: "",
            stagedConfigGeneration = prefs[PREF_STAGED_CONFIG_GEN] ?: -1L,
            stagedConfigJson = prefs[PREF_STAGED_CONFIG_JSON] ?: "",
            stagedSecretsGeneration = prefs[PREF_STAGED_SECRETS_GEN] ?: -1L,
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
    suspend fun stageConfig(generation: Long, configJson: String) {
        dataStore.edit { prefs ->
            prefs[PREF_STAGED_CONFIG_GEN] = generation
            prefs[PREF_STAGED_CONFIG_JSON] = configJson
        }
    }

    /** 写 stagedSecrets(generation=N) 标记（凭据已按 generation 写入安全存储）。 */
    suspend fun stageSecrets(generation: Long) {
        dataStore.edit { prefs ->
            prefs[PREF_STAGED_SECRETS_GEN] = generation
        }
    }

    /**
     * 原子提交：单次 updateData 同时推进 activeGeneration 与 committedConfigJson。
     * 失败时旧 generation 继续有效（staged 标记停留在 N，读取者仍读旧 committed 版本）。
     */
    suspend fun commitGeneration(generation: Long, committedConfigJson: String) {
        dataStore.edit { prefs ->
            prefs[PREF_ACTIVE] = generation
            prefs[PREF_COMMITTED_CONFIG_JSON] = committedConfigJson
        }
    }

    /** 测试隔离：清空全部提交状态。 */
    suspend fun clear() {
        dataStore.edit { prefs ->
            prefs.remove(PREF_ACTIVE)
            prefs.remove(PREF_COMMITTED_CONFIG_JSON)
            prefs.remove(PREF_STAGED_CONFIG_GEN)
            prefs.remove(PREF_STAGED_CONFIG_JSON)
            prefs.remove(PREF_STAGED_SECRETS_GEN)
        }
    }

    companion object {
        private const val KEY_ACTIVE_GENERATION = "active_generation"
        private const val KEY_COMMITTED_CONFIG_JSON = "committed_config_json"
        private const val KEY_STAGED_CONFIG_GENERATION = "staged_config_generation"
        private const val KEY_STAGED_CONFIG_JSON = "staged_config_json"
        private const val KEY_STAGED_SECRETS_GENERATION = "staged_secrets_generation"

        private val PREF_ACTIVE = longPreferencesKey(KEY_ACTIVE_GENERATION)
        private val PREF_COMMITTED_CONFIG_JSON = stringPreferencesKey(KEY_COMMITTED_CONFIG_JSON)
        private val PREF_STAGED_CONFIG_GEN = longPreferencesKey(KEY_STAGED_CONFIG_GENERATION)
        private val PREF_STAGED_CONFIG_JSON = stringPreferencesKey(KEY_STAGED_CONFIG_JSON)
        private val PREF_STAGED_SECRETS_GEN = longPreferencesKey(KEY_STAGED_SECRETS_GENERATION)
    }
}

/**
 * #592 六：一次同步操作使用的完整不可变配置快照。
 *
 * 正式同步、自动同步、试运行和连接诊断都先从统一仓库取得一次完整 snapshot，
 * 随后整个操作只使用这份 snapshot，不再从磁盘二次读取 config/secrets。
 */
data class SyncProfileSnapshot(
    val generation: Long,
    val config: SyncConfig,
    val secrets: SyncSecrets,
)
