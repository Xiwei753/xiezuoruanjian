package com.xiwei.sujian.ui.phone.portrait

import androidx.compose.animation.animateContentSize
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.lifecycle.viewmodel.compose.viewModel
import com.xiwei.sujian.R
import com.xiwei.sujian.designsystem.component.SujianIconButton
import com.xiwei.sujian.designsystem.icon.SujianIcons
import com.xiwei.sujian.designsystem.theme.LocalSujianDimensions
import com.xiwei.sujian.runtime.LocalSujianAppDependencies
import com.xiwei.sujian.ui.compose.navigation.SettingsSection
import com.xiwei.sujian.ui.compose.settings.SettingsDetailPane
import com.xiwei.sujian.ui.compose.settings.settingsCategories
import com.xiwei.sujian.ui.compose.settings.SettingsIntent
import com.xiwei.sujian.ui.compose.settings.SettingsUiState
import com.xiwei.sujian.ui.compose.settings.SettingsViewModel

@Composable
fun PhoneSettingsScreen(
    expandedSections: Set<SettingsSection>,
    onToggleSection: (SettingsSection) -> Unit,
    modifier: Modifier = Modifier,
) {
    val deps = LocalSujianAppDependencies.current
    val vm: SettingsViewModel = viewModel(factory = SettingsViewModel.Factory(deps.settingsRepository, deps.syncCoordinator))
    val uiState by vm.uiState.collectAsState()
    val dims = LocalSujianDimensions.current

    LazyColumn(
        modifier = modifier.fillMaxSize(),
        contentPadding = PaddingValues(vertical = dims.space8),
    ) {
        item(key = "global_search") {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .clickable { }
                    .padding(horizontal = dims.space16, vertical = dims.space12),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Icon(
                    imageVector = SujianIcons.Search,
                    contentDescription = null,
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Text(
                    text = stringResource(id = R.string.search_hint),
                    modifier = Modifier.padding(start = dims.space12),
                    style = MaterialTheme.typography.bodyLarge,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }

        items(settingsCategories, key = { it.section.name }) { category ->
            SettingsExpandableSection(
                section = category.section,
                isExpanded = expandedSections.contains(category.section),
                onToggle = { onToggleSection(category.section) },
                state = uiState,
                onIntent = vm::handleIntent,
            )
        }
    }
}

@Composable
private fun SettingsExpandableSection(
    section: SettingsSection,
    isExpanded: Boolean,
    onToggle: () -> Unit,
    state: SettingsUiState,
    onIntent: (SettingsIntent) -> Unit,
    modifier: Modifier = Modifier,
) {
    val dims = LocalSujianDimensions.current
    val (titleResId, icon) = settingsCategories.find { it.section == section }?.let { it.titleResId to it.icon } ?: (0 to SujianIcons.Info)

    Column(modifier = modifier.fillMaxWidth().animateContentSize()) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .clickable { onToggle() }
                .padding(horizontal = dims.space16, vertical = dims.space12),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(
                imageVector = icon,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.primary,
            )
            Text(
                text = stringResource(id = titleResId),
                modifier = Modifier
                    .weight(1f)
                    .padding(start = dims.space12),
                style = MaterialTheme.typography.titleSmall,
                color = MaterialTheme.colorScheme.onSurface,
            )
            Icon(
                imageVector = if (isExpanded) SujianIcons.ExpandLess else SujianIcons.ExpandMore,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        if (isExpanded) {
            Column(modifier = Modifier.padding(horizontal = dims.space16)) {
                SettingsDetailPane(
                    section = section,
                    state = state,
                    onIntent = onIntent,
                )
            }
        }
    }
}


