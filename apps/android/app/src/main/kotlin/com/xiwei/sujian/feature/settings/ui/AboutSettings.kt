package com.xiwei.sujian.feature.settings.ui

import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.height
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.xiwei.sujian.R

/**
 * #633 评论 5379618506：关于设置 — 一个逻辑字段组 = 一张 High 内卡。
 *
 * 关于分组: 应用名称 + 作者 + 许可 + 数据根路径 + 版本信息
 */
@Composable
fun AboutSettingsContent(vm: SettingsViewModel) {
    val state by vm.aboutState.collectAsStateWithLifecycle()

    SettingsInnerCard {
        Spacer(modifier = Modifier.height(12.dp))
        Text(
            text = stringResource(id = R.string.about_app_name),
            style = MaterialTheme.typography.headlineSmall,
            modifier = Modifier.height(32.dp),
        )
        Spacer(modifier = Modifier.height(4.dp))
        Text(
            text = stringResource(id = R.string.about_author),
            style = MaterialTheme.typography.bodyMedium,
        )
        Spacer(modifier = Modifier.height(4.dp))
        Text(
            text = stringResource(id = R.string.about_license),
            style = MaterialTheme.typography.bodyMedium,
        )
        Spacer(modifier = Modifier.height(4.dp))
        Text(
            text = stringResource(id = R.string.pref_data_root_path, state.dataRootPath),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        if (state.versionInfo.isNotEmpty()) {
            Spacer(modifier = Modifier.height(4.dp))
            Text(
                text = state.versionInfo,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        Spacer(modifier = Modifier.height(12.dp))
    }
}
