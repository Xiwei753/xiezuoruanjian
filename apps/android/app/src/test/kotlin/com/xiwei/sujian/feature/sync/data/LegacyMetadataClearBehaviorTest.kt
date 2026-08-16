package com.xiwei.sujian.feature.sync.data

import android.content.Context
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.longPreferencesKey
import androidx.datastore.preferences.core.stringPreferencesKey
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

/**
 * #630 评论 5307423953 Part C 行为测试：[LegacySyncProfileMetadataReader.clearLegacyMetadata]
 *
 * 验证：
 * 1. 清空旧 app_sync_profile DataStore 的 active_generation/committed_config_json/staged_*；
 * 2. 新 sync_profile DataStore 中只删除带 `<projectId>.` 前缀的旧 key；
 * 3. **绝对不删**当前不带前缀的全局 key。
 *
 * 用真实 Robolectric DataStore 验证行为，不 mock。
 * 使用生产代码的 internal DataStore 委托（同包同模块可见），避免多实例冲突。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class LegacyMetadataClearBehaviorTest {
    private val keyActiveGen = longPreferencesKey("active_generation")
    private val keyCommittedConfig = stringPreferencesKey("committed_config_json")
    private val keyStagedConfigGen = longPreferencesKey("staged_config_generation")
    private val keyStagedConfigJson = stringPreferencesKey("staged_config_json")
    private val keyStagedSecretsGen = longPreferencesKey("staged_secrets_generation")
    private val keyProj1ActiveGen = longPreferencesKey("proj-1.active_generation")
    private val keyProj1CommittedConfig = stringPreferencesKey("proj-1.committed_config_json")
    private val keyProj2ActiveGen = longPreferencesKey("proj-2.active_generation")
    private val keyProj2CommittedConfig = stringPreferencesKey("proj-2.committed_config_json")

    private fun appContext(): Context = RuntimeEnvironment.getApplication()

    private fun newReader(): LegacySyncProfileMetadataReader {
        val context = appContext()
        val store = SyncProfileStore(context)
        return LegacySyncProfileMetadataReader(context, store)
    }

    private fun newProfileStore(): SyncProfileStore = SyncProfileStore(appContext())

    private suspend fun writeLegacyAppMetadata() {
        // 使用生产代码的 internal 委托 legacyAppSyncProfileDataStore（同一 DataStore 实例）
        val store = appContext().legacyAppSyncProfileDataStore
        store.edit { prefs ->
            prefs[keyActiveGen] = 1L
            prefs[keyCommittedConfig] = "{\"old\":\"config\"}"
            prefs[keyStagedConfigGen] = 1L
            prefs[keyStagedConfigJson] = "{\"staged\":\"config\"}"
            prefs[keyStagedSecretsGen] = 1L
        }
    }

    private suspend fun readLegacyAppMetadata(): Preferences = appContext().legacyAppSyncProfileDataStore.data.first()

    private suspend fun writeSyncProfileMixedKeys() {
        // 全局 key 通过 SyncProfileStore 公开 API 写
        val store = newProfileStore()
        store.commitGeneration(generation = 2L, committedConfigJson = "{\"new\":\"global\"}")
        store.stageConfig(generation = 3L, configJson = "{\"staged\":\"global\"}")
        store.stageSecrets(generation = 3L)
        // 旧 project 前缀 key 通过生产代码的 internal 委托 syncProfileDataStore 直接写
        // （同一 DataStore 实例，SyncProfileStore 内部也用它）
        val ds = appContext().syncProfileDataStore
        ds.edit { prefs ->
            prefs[keyProj1ActiveGen] = 1L
            prefs[keyProj1CommittedConfig] = "{\"old\":\"proj1\"}"
            prefs[keyProj2ActiveGen] = 1L
            prefs[keyProj2CommittedConfig] = "{\"old\":\"proj2\"}"
        }
    }

    private suspend fun readSyncProfilePrefs(): Preferences = appContext().syncProfileDataStore.data.first()

    @Test
    fun clearLegacyMetadata_clearsOldAppSyncProfileKeys() =
        runTest {
            writeLegacyAppMetadata()
            val reader = newReader()
            reader.clearLegacyMetadata()
            val prefs = readLegacyAppMetadata()
            assertNull("active_generation 应被清空", prefs[keyActiveGen])
            assertNull("committed_config_json 应被清空", prefs[keyCommittedConfig])
            assertNull("staged_config_generation 应被清空", prefs[keyStagedConfigGen])
            assertNull("staged_config_json 应被清空", prefs[keyStagedConfigJson])
            assertNull("staged_secrets_generation 应被清空", prefs[keyStagedSecretsGen])
        }

    @Test
    fun clearLegacyMetadata_deletesLegacyProjectPrefixedKeys() =
        runTest {
            writeSyncProfileMixedKeys()
            val reader = newReader()
            reader.clearLegacyMetadata()
            val prefs = readSyncProfilePrefs()
            assertNull("proj-1.active_generation 应被删", prefs[keyProj1ActiveGen])
            assertNull("proj-1.committed_config_json 应被删", prefs[keyProj1CommittedConfig])
            assertNull("proj-2.active_generation 应被删", prefs[keyProj2ActiveGen])
            assertNull("proj-2.committed_config_json 应被删", prefs[keyProj2CommittedConfig])
        }

    @Test
    fun clearLegacyMetadata_preservesGlobalKeysWithoutPrefix() =
        runTest {
            writeSyncProfileMixedKeys()
            val reader = newReader()
            reader.clearLegacyMetadata()
            val prefs = readSyncProfilePrefs()
            assertEquals("全局 active_generation 不带前缀，必须保留", 2L, prefs[keyActiveGen])
            assertEquals("全局 committed_config_json 不带前缀，必须保留", "{\"new\":\"global\"}", prefs[keyCommittedConfig])
            assertEquals("全局 staged_config_generation 不带前缀，必须保留", 3L, prefs[keyStagedConfigGen])
            assertEquals("全局 staged_secrets_generation 不带前缀，必须保留", 3L, prefs[keyStagedSecretsGen])
        }

    @Test
    fun clearLegacyMetadata_idempotent() =
        runTest {
            writeLegacyAppMetadata()
            writeSyncProfileMixedKeys()
            val reader = newReader()
            reader.clearLegacyMetadata()
            reader.clearLegacyMetadata()
            val prefs = readSyncProfilePrefs()
            assertEquals("第二次 clear 后全局 key 仍保留", 2L, prefs[keyActiveGen])
            assertNull("第二次 clear 后旧 project key 仍为 null", prefs[keyProj1ActiveGen])
        }
}
