package com.xiwei.sujian.feature.settings.ui

import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.runtime.getValue
import androidx.compose.ui.res.stringResource
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.xiwei.sujian.R
import com.xiwei.sujian.core.designsystem.component.SujianSwitchRow

/**
 * #630 评论13/评论15/评论16/评论5324547885项2: 行级 LazyColumn — 每个真实设置控件是独立 item，有稳定 key。
 * 使用 [SettingsExpandedRowContainer] 替代旧的 [SettingsGroupItemContainer] +
 * [SettingsFieldRowContainer] 嵌套；展开内容在外层 Low 内缩 High 表面里连续拼接。
 * 每个 item 只 collect 自己的 row-level StateFlow，避免整分类重组。
 * 展开字段使用 [SettingsExpandedItemContent] 统一 fadeIn100/fadeOut70/placement120。
 *
 * 评论 #16: 标题用 aiAvailableRow（只收 available），开关用 aiEnabledRow（只收 enabled），
 * 切 enabled 时标题不会重组。
 */
fun LazyListScope.aiSettingsItems(vm: SettingsViewModel) {
    item(key = "ai.enabled", contentType = CONTENT_TYPE_EXPANDED_FIELD) {
        val available by vm.aiAvailableRow.collectAsStateWithLifecycle()
        if (!available) return@item
        val checked by vm.aiEnabledRow.collectAsStateWithLifecycle()
        SettingsExpandedItemContent {
            SettingsExpandedRowContainer(
                firstInCategory = true,
                lastInCategory = true,
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
}
