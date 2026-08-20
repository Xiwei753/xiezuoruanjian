package com.xiwei.sujian.feature.settings.ui

import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.runtime.getValue
import androidx.compose.ui.res.stringResource
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.xiwei.sujian.R
import com.xiwei.sujian.core.designsystem.component.SujianSwitchRow

/**
 * #630 R14 合并字段组 — 一个真实字段组一个 High Surface item，组内多个字段普通布局。
 * 使用 [SettingsExpandedGroupContainer] 统一 16dp content padding、12dp 圆角。
 * 每个 item 只 collect 自己需要的 row-level StateFlow，避免整分类重组。
 */
fun LazyListScope.aiSettingsItems(
    vm: SettingsViewModel,
    closeOuterGroup: Boolean,
) {
    item(key = "ai.enabled_group", contentType = CONTENT_TYPE_EXPANDED_FIELD_GROUP) {
        val available by vm.aiAvailableRow.collectAsStateWithLifecycle()
        if (!available) return@item
        val checked by vm.aiEnabledRow.collectAsStateWithLifecycle()
        SettingsExpandedGroupContainer(
            closeOuterGroup = closeOuterGroup,
            firstInGroup = true,
            lastInGroup = true,
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
