package com.xiwei.sujian.feature.settings.ui

import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.runtime.getValue
import androidx.compose.ui.res.stringResource
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.xiwei.sujian.R
import com.xiwei.sujian.core.designsystem.component.SujianSwitchRow

/**
 * #630 R14 字段组模式 — 一个真实字段组一个 High Surface item，组内多个字段普通布局。
 * 使用 [SettingsExpandedGroupContainer] 统一 16dp content padding、12dp 圆角。
 * 每个 item 只 collect 自己的 row-level StateFlow，避免整分类重组。
 */
fun LazyListScope.laboratorySettingsItems(
    vm: SettingsViewModel,
    closeOuterGroup: Boolean,
) {
    item(key = "laboratory.fullscreen", contentType = CONTENT_TYPE_EXPANDED_FIELD_GROUP) {
        val checked by vm.immersiveFullscreenRow.collectAsStateWithLifecycle()
        SettingsExpandedGroupContainer(
            closeOuterGroup = closeOuterGroup,
            firstInGroup = true,
            lastInGroup = true,
        ) {
            SujianSwitchRow(
                title = stringResource(id = R.string.lab_fullscreen_immersive),
                checked = checked,
                onCheckedChange = { c ->
                    vm.handleIntent(SettingsIntent.UpdateLocal { it.copy(experimentalFullscreenMode = c) })
                },
                supportingText = stringResource(id = R.string.lab_fullscreen_immersive_summary),
            )
        }
    }
}
