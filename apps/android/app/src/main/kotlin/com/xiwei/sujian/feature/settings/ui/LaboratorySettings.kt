package com.xiwei.sujian.feature.settings.ui

import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.runtime.getValue
import androidx.compose.ui.res.stringResource
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.xiwei.sujian.R
import com.xiwei.sujian.core.designsystem.component.SujianSwitchRow

/**
 * #630 评论13/评论15/评论5324547885项2: 行级 LazyColumn — 每个真实设置控件是独立 item，有稳定 key。
 * 使用 [SettingsExpandedRowContainer] 替代旧的 [SettingsGroupItemContainer] +
 * [SettingsFieldRowContainer] 嵌套；展开内容在外层 Low 内缩 High 表面里连续拼接。
 * 每个 item 只 collect 自己的 row-level StateFlow，避免整分类重组。
 * 展开字段使用 [SettingsExpandedItemContent] 统一 fadeIn100/fadeOut70/placement120。
 */
fun LazyListScope.laboratorySettingsItems(
    vm: SettingsViewModel,
    closeOuterGroup: Boolean,
) {
    item(key = "laboratory.fullscreen", contentType = CONTENT_TYPE_EXPANDED_FIELD) {
        val checked by vm.immersiveFullscreenRow.collectAsStateWithLifecycle()
        SettingsExpandedItemContent {
            SettingsExpandedRowContainer(
                closeOuterGroup = closeOuterGroup,
                firstInCategory = true,
                lastInCategory = true,
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
}
