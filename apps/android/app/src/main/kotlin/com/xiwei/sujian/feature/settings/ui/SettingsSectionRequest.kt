package com.xiwei.sujian.feature.settings.ui

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import com.xiwei.sujian.app.navigation.SettingsSection

/**
 * 一次性设置分类定位请求 — 搜索结果跳转设置页时使用。
 *
 * 只表达「打开后展开哪个分类」的瞬时 UI 意图，不保存设置业务状态。
 */
object SettingsSectionRequest {
    var requestedSection by mutableStateOf<SettingsSection?>(null)
        private set

    fun request(section: SettingsSection?) {
        requestedSection = section
    }

    fun consume(section: SettingsSection) {
        if (requestedSection == section) requestedSection = null
    }
}
