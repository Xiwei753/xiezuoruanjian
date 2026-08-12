package com.xiwei.sujian.feature.settings.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import com.xiwei.sujian.R
import com.xiwei.sujian.core.designsystem.component.SujianSection
import com.xiwei.sujian.core.designsystem.theme.LocalSujianDimensions

@Composable
fun AboutSettings(
    state: AboutSectionState,
    onIntent: (SettingsIntent) -> Unit,
    modifier: Modifier = Modifier,
) {
    val dims = LocalSujianDimensions.current

    Column(
        modifier = modifier.padding(dims.space16),
        verticalArrangement = Arrangement.spacedBy(dims.space16),
    ) {
        SujianSection(title = stringResource(id = R.string.pref_category_about)) {
            Text(
                text = stringResource(id = R.string.about_app_name),
                style = MaterialTheme.typography.headlineSmall,
            )
            Spacer(modifier = Modifier.height(dims.space8))
            Text(
                text = stringResource(id = R.string.about_author),
                style = MaterialTheme.typography.bodyMedium,
            )
            Spacer(modifier = Modifier.height(dims.space4))
            Text(
                text = stringResource(id = R.string.about_license),
                style = MaterialTheme.typography.bodyMedium,
            )
            Spacer(modifier = Modifier.height(dims.space4))
            Text(
                text = stringResource(id = R.string.pref_data_root_path, state.dataRootPath),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            if (state.versionInfo.isNotEmpty()) {
                Spacer(modifier = Modifier.height(dims.space4))
                Text(
                    text = state.versionInfo,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}
