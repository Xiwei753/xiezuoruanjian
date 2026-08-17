package com.xiwei.sujian.feature.settings.ui

import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.runtime.getValue
import androidx.compose.ui.res.stringResource
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.xiwei.sujian.R
import com.xiwei.sujian.core.designsystem.component.SujianSwitchRow

/**
 * #630 评论13 项2: 扁平 LazyColumn — 向父 [LazyListScope] 注册行，
 * 每个 [SettingsFieldGroup] 是独立 item，有稳定 key。
 */
fun LazyListScope.laboratorySettingsItems(vm: SettingsViewModel) {
    item(key = "laboratory.fullscreen_group") {
        val state by vm.laboratoryState.collectAsStateWithLifecycle()
        SettingsGroupItemContainer(isLast = true) {
            SettingsFieldGroup(title = stringResource(id = R.string.pref_category_laboratory)) {
                SujianSwitchRow(
                    title = stringResource(id = R.string.lab_fullscreen_immersive),
                    checked = state.immersiveFullscreen,
                    onCheckedChange = { checked ->
                        vm.handleIntent(SettingsIntent.UpdateLocal { it.copy(experimentalFullscreenMode = checked) })
                    },
                    supportingText = stringResource(id = R.string.lab_fullscreen_immersive_summary),
                )
            }
        }
    }
}
