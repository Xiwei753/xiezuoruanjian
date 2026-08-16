package com.xiwei.sujian.feature.sync.data

import android.content.Context
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.longPreferencesKey
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import com.xiwei.sujian.feature.sync.data.model.LegacyProfileMetadata
import kotlinx.coroutines.flow.first

internal val Context.legacyAppSyncProfileDataStore by preferencesDataStore(name = "app_sync_profile")

/**
 * #630 评论第 5 点 Part C-Android：旧 DataStore profile metadata 读取器。
 *
 * 只读旧 `app_sync_profile` 和 `sync_profile` DataStore 的 `active_generation` /
 * `committed_config_json`，构造 [LegacyProfileMetadata] 列表传给 Core
 * `migrate_legacy_sync_profile_with_metadata`，避免 Core 猜测 generation 上限。
 *
 * - `app_sync_profile` DataStore：keys `active_generation`, `committed_config_json`
 *   （旧 [AppSyncProfileStore] 单一全局槽，不带 projectId 前缀）
 * - `sync_profile` DataStore：keys `<projectId>.active_generation`,
 *   `<projectId>.committed_config_json`（旧 [ProjectSyncProfileStore] per-project）。
 *   新 [SyncProfileStore] 也用 `sync_profile` DataStore，但 key 不带前缀，与旧 project key
 *   （带 `.` 前缀）不冲突；旧 project metadata 读取委托给
 *   [SyncProfileStore.readLegacyProjectMetadata]，复用同一 DataStore 实例避免文件锁冲突。
 *
 * #630 评论 5307423953 Part C：读取由本类负责，清除也由本类负责（Core 访问不到
 * Android DataStore）。[clearLegacyMetadata] 在迁移成功后由
 * [SyncRepository.migrateLegacyProfileIfNeeded] 的 migrated 分支调用。
 * 不枚举作品 — project ID 列表从 DataStore key 前缀解析，不依赖 ProjectBridge。
 */
class LegacySyncProfileMetadataReader(
    context: Context,
    private val profileStore: SyncProfileStore,
) {
    private val appContext = context.applicationContext
    private val appStore = appContext.legacyAppSyncProfileDataStore

    /**
     * #630 评论 5307423953 Part C：清除旧 Android DataStore profile metadata。
     *
     * 迁移成功后调用（仅 [SyncRepository.migrateLegacyProfileIfNeeded] 的 migrated 分支，
     * 且新 generation 的 saveSyncSecretsForGeneration → stageSecrets → commitGeneration
     * 全部成功后）。失败/needs_reconfigure 不调用，保留旧 metadata 供用户手动恢复。
     *
     * 清理范围：
     * - 旧 `app_sync_profile` DataStore：`active_generation` / `committed_config_json` /
     *   `staged_config_generation` / `staged_config_json` / `staged_secrets_generation`；
     * - 新 `sync_profile` DataStore：只删除带 `<projectId>.` 前缀的旧 project key，
     *   **绝对不删**当前不带前缀的全局 key（委托给
     *   [SyncProfileStore.clearLegacyProjectMetadataKeys]）。
     */
    suspend fun clearLegacyMetadata() {
        // 1. 清旧 app_sync_profile DataStore 的所有 metadata key
        appStore.edit { prefs ->
            prefs.remove(KEY_ACTIVE_GENERATION)
            prefs.remove(KEY_COMMITTED_CONFIG_JSON)
            prefs.remove(KEY_STAGED_CONFIG_GENERATION)
            prefs.remove(KEY_STAGED_CONFIG_JSON)
            prefs.remove(KEY_STAGED_SECRETS_GENERATION)
        }
        // 2. 清新 sync_profile DataStore 中带 <projectId>. 前缀的旧 project key
        //    （不删当前不带前缀的全局 key）
        profileStore.clearLegacyProjectMetadataKeys()
    }

    /**
     * 读取所有旧 profile metadata：先 app，后 project。
     *
     * 返回空列表表示无旧 profile，调用方传空列表给 Core，Core 用 base key / 文件 fallback。
     */
    suspend fun readMetadata(): List<LegacyProfileMetadata> {
        val result = mutableListOf<LegacyProfileMetadata>()
        result.addAll(readAppMetadata())
        result.addAll(profileStore.readLegacyProjectMetadata())
        return result
    }

    /**
     * 读旧 `app_sync_profile` DataStore 的 app 级 metadata。
     *
     * 只有 `committed_config_json` 非空才算有旧 app profile（空字符串表示从未提交）。
     * `active_generation` 为 0 或缺失时返回 null（Core 回退 base key / 文件）。
     */
    private suspend fun readAppMetadata(): List<LegacyProfileMetadata> {
        val prefs = appStore.data.first()
        val committedConfigJson = prefs[KEY_COMMITTED_CONFIG_JSON]
        if (committedConfigJson.isNullOrEmpty()) return emptyList()
        val activeGeneration = prefs[KEY_ACTIVE_GENERATION]
        return listOf(
            LegacyProfileMetadata(
                source = "app",
                projectId = null,
                activeGeneration = activeGeneration?.takeIf { it > 0L },
            ),
        )
    }

    companion object {
        private val KEY_ACTIVE_GENERATION = longPreferencesKey("active_generation")
        private val KEY_COMMITTED_CONFIG_JSON = stringPreferencesKey("committed_config_json")
        private val KEY_STAGED_CONFIG_GENERATION = longPreferencesKey("staged_config_generation")
        private val KEY_STAGED_CONFIG_JSON = stringPreferencesKey("staged_config_json")
        private val KEY_STAGED_SECRETS_GENERATION = longPreferencesKey("staged_secrets_generation")
    }
}
