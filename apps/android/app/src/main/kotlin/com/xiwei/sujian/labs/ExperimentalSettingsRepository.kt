package com.xiwei.sujian.labs

import android.content.Context
import android.content.SharedPreferences
import androidx.core.content.edit

/**
 * 实验室功能设置仓库 — 使用独立 SharedPreferences: sujian_experiments
 *
 * 不塞在 sujian_diagnostics 中，独立存储。
 */
class ExperimentalSettingsRepository(context: Context) {
    private val prefs: SharedPreferences =
        context.getSharedPreferences("sujian_experiments", Context.MODE_PRIVATE)

    fun isEnabled(featureId: String): Boolean {
        val feature = ExperimentalFeatureRegistry.findById(featureId) ?: return false
        return prefs.getBoolean(featureId, feature.defaultEnabled)
    }

    fun setEnabled(
        featureId: String,
        enabled: Boolean,
    ) {
        prefs.edit { putBoolean(featureId, enabled) }
    }

    /**
     * 迁移旧字段：从 LocalSettings.experimentalFullscreenMode 迁移到 registry + prefs
     * 读取时做一次迁移，迁移后删除旧字段（或标记已迁移）
     */
    fun migrateFromLegacy(legacyFullscreenMode: Boolean) {
        if (!prefs.contains("migrated_from_legacy")) {
            // 只在未迁移时执行
            prefs.edit {
                if (!prefs.contains("fullscreen_immersive")) {
                    putBoolean("fullscreen_immersive", legacyFullscreenMode)
                    putBoolean("migrated_from_legacy", true)
                } else {
                    putBoolean("migrated_from_legacy", true)
                }
            }
        }
    }
}
