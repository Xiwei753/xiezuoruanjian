package com.xiwei.sujian.feature.settings.ui

import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.res.stringResource
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.xiwei.sujian.R
import com.xiwei.sujian.core.designsystem.component.SujianSwitchRow

/**
 * #633 评论 5379618506：AI 设置 — 一个逻辑字段组 = 一张 High 内卡。
 */
@Composable
fun AiSettingsContent(vm: SettingsViewModel) {
    val available by vm.aiAvailableRow.collectAsStateWithLifecycle()
    val checked by vm.aiEnabledRow.collectAsStateWithLifecycle()

    if (!available) return
    SettingsInnerCard {
        SujianSwitchRow(
            title = stringResource(id = R.string.pref_ai_enabled),
            checked = checked,
            onCheckedChange = { c ->
                vm.handleIntent(SettingsIntent.UpdateLocal { it.copy(aiEnabled = c) })
            },
        )
    }
}
