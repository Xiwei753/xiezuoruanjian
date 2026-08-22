package com.xiwei.sujian.feature.settings.ui

import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.runtime.getValue
import androidx.compose.ui.res.stringResource
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.xiwei.sujian.R
import com.xiwei.sujian.core.designsystem.component.SujianSwitchRow

/**
 * #632 评论 5377052579：AI 设置 — 每个重控件一个 Lazy item。
 * 用 [SettingsExpandedFieldContainer] 渲染，[ExpandedFieldPosition.Only] 表示字段组只有一个 item。
 */
fun LazyListScope.aiSettingsItems(
    vm: SettingsViewModel,
    closeOuterGroup: Boolean,
) {
    item(key = "ai.enabled", contentType = CONTENT_TYPE_SWITCH) {
        val available by vm.aiAvailableRow.collectAsStateWithLifecycle()
        if (!available) return@item
        val checked by vm.aiEnabledRow.collectAsStateWithLifecycle()
        SettingsExpandedFieldContainer(
            position = ExpandedFieldPosition.Only,
            closeOuterGroup = closeOuterGroup,
        ) {
            SujianSwitchRow(
                title = stringResource(id = R.string.pref_ai_enabled),
                checked = checked,
                onCheckedChange = { c ->
                    vm.handleIntent(SettingsIntent.UpdateLocal { it.copy(aiEnabled = c) })
                },
            )
        }
    }
}
