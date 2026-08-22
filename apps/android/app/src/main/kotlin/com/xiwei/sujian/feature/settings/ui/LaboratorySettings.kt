package com.xiwei.sujian.feature.settings.ui

import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.res.stringResource
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.xiwei.sujian.R
import com.xiwei.sujian.core.designsystem.component.SujianSwitchRow

/**
 * #633 评论 5379618506：实验室设置 — 一个逻辑字段组 = 一张 High 内卡。
 */
@Composable
fun LaboratorySettingsContent(vm: SettingsViewModel) {
    val checked by vm.immersiveFullscreenRow.collectAsStateWithLifecycle()

    SettingsInnerCard {
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
