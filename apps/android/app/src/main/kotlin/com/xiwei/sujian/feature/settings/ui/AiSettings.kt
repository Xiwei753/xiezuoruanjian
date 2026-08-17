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
fun LazyListScope.aiSettingsItems(vm: SettingsViewModel) {
    item(key = "ai.enabled_group") {
        val state by vm.aiState.collectAsStateWithLifecycle()
        if (!state.available) return@item
        SettingsGroupItemContainer(isLast = true) {
            SettingsFieldGroup(title = stringResource(id = R.string.pref_category_ai)) {
                SujianSwitchRow(
                    title = stringResource(id = R.string.pref_ai_enabled),
                    checked = state.enabled,
                    onCheckedChange = { checked ->
                        vm.handleIntent(SettingsIntent.UpdateLocal { it.copy(aiEnabled = checked) })
                    },
                )
            }
        }
    }
}
