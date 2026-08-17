package com.xiwei.sujian.feature.settings.ui

import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.runtime.getValue
import androidx.compose.ui.res.stringResource
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.xiwei.sujian.R
import com.xiwei.sujian.core.designsystem.component.SujianSwitchRow

/**
 * #630 评论13/评论15: 行级 LazyColumn — 每个真实设置控件是独立 item，有稳定 key。
 * 使用 [SettingsFieldRowContainer] 的 isFirst/isLast 保持 M3 高色阶卡片视觉。
 * 每个 item 只 collect 自己的 row-level StateFlow，避免整分类重组。
 */
fun LazyListScope.laboratorySettingsItems(vm: SettingsViewModel) {
    item(key = "laboratory.fullscreen_title") {
        SettingsGroupItemContainer(isLast = false, isFirst = true) {
            SettingsFieldRowContainer(isFirst = true, isLast = false) {
                SettingsFieldGroupTitle(title = stringResource(id = R.string.pref_category_laboratory))
            }
        }
    }

    item(key = "laboratory.fullscreen") {
        val checked by vm.immersiveFullscreenRow.collectAsStateWithLifecycle()
        SettingsGroupItemContainer(isLast = true) {
            SettingsFieldRowContainer(isFirst = false, isLast = true) {
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
