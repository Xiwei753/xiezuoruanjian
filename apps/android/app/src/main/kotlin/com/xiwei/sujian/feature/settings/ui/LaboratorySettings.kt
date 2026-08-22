package com.xiwei.sujian.feature.settings.ui

import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.runtime.getValue
import androidx.compose.ui.res.stringResource
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.xiwei.sujian.R
import com.xiwei.sujian.core.designsystem.component.SujianSwitchRow

/**
 * #632 评论 5377052579：实验室设置 — 每个重控件一个 Lazy item。
 * 用 [SettingsExpandedFieldContainer] 渲染，[ExpandedFieldPosition.Only] 表示字段组只有一个 item。
 */
fun LazyListScope.laboratorySettingsItems(
    vm: SettingsViewModel,
    closeOuterGroup: Boolean,
) {
    item(key = "laboratory.fullscreen", contentType = CONTENT_TYPE_SWITCH) {
        val checked by vm.immersiveFullscreenRow.collectAsStateWithLifecycle()
        SettingsExpandedFieldContainer(
            position = ExpandedFieldPosition.Only,
            closeOuterGroup = closeOuterGroup,
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
