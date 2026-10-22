package com.xiwei.sujian.feature.settings.ui

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import com.xiwei.sujian.app.navigation.SettingsSection

/** 一次设置定位请求：分类 + 可选的 Core 设置键。 */
data class SettingsLocationRequest(
    val section: SettingsSection,
    val settingKey: String? = null,
)

/**
 * 一次性设置定位请求 — 搜索结果跳转设置页时使用。
 *
 * 只表达「打开后展开哪个分类、定位哪个设置项」的瞬时 UI 意图，
 * 不保存设置业务状态；设置页消费后立即清空。
 */
object SettingsSectionRequest {
    var pending by mutableStateOf<SettingsLocationRequest?>(null)
        private set

    val requestedSection: SettingsSection?
        get() = pending?.section

    val requestedSettingKey: String?
        get() = pending?.settingKey

    fun request(
        section: SettingsSection?,
        settingKey: String? = null,
    ) {
        pending =
            section?.let {
                SettingsLocationRequest(
                    section = it,
                    settingKey = settingKey?.takeIf(String::isNotBlank),
                )
            }
    }

    fun consume() {
        pending = null
    }

    fun consume(request: SettingsLocationRequest) {
        if (pending == request) pending = null
    }

    fun consume(section: SettingsSection) {
        if (pending?.section == section) pending = null
    }
}
