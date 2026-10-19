package com.xiwei.sujian.feature.settings.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
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
    // #958：内置校对模型的第三方来源声明与许可全文（点击后弹窗展示）。
    var thirdPartyNotices by remember { mutableStateOf<String?>(null) }

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
        TextButton(onClick = { thirdPartyNotices = vm.bundledCorrectionNoticesText() }) {
            Text(text = stringResource(id = R.string.about_third_party_resources))
        }
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

    val notices = thirdPartyNotices
    if (notices != null) {
        AlertDialog(
            onDismissRequest = { thirdPartyNotices = null },
            title = { Text(text = stringResource(id = R.string.about_third_party_resources)) },
            text = {
                Box(
                    modifier =
                        Modifier
                            .fillMaxWidth()
                            .heightIn(max = 420.dp)
                            .verticalScroll(rememberScrollState()),
                ) {
                    Text(
                        text = notices,
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
            },
            confirmButton = {
                TextButton(onClick = { thirdPartyNotices = null }) {
                    Text(text = stringResource(id = R.string.about_third_party_close))
                }
            },
        )
    }
}
