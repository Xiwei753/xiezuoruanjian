package com.xiwei.sujian.designsystem.layout

import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FabPosition
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import com.xiwei.sujian.designsystem.component.SujianFab
import com.xiwei.sujian.designsystem.component.SujianSnackbar
import com.xiwei.sujian.designsystem.component.SujianTopAppBar

@Composable
fun SujianAppScaffold(
    modifier: Modifier = Modifier,
    snackbarHostState: SnackbarHostState? = null,
    content: @Composable (PaddingValues) -> Unit,
) {
    Scaffold(
        modifier = modifier.fillMaxSize(),
        containerColor = MaterialTheme.colorScheme.surface,
        snackbarHost = {
            if (snackbarHostState != null) {
                SnackbarHost(hostState = snackbarHostState) { data ->
                    SujianSnackbar(data = data)
                }
            }
        },
        content = content,
    )
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SujianScreenScaffold(
    title: String,
    modifier: Modifier = Modifier,
    navigationIcon: androidx.compose.ui.graphics.vector.ImageVector? = null,
    navigationIconContentDescription: String? = null,
    onNavigationClick: (() -> Unit)? = null,
    actions: @Composable () -> Unit = {},
    snackbarHostState: SnackbarHostState? = null,
    fabIcon: androidx.compose.ui.graphics.vector.ImageVector? = null,
    fabText: String? = null,
    fabExtended: Boolean = false,
    fabContentDescription: String? = null,
    onFabClick: (() -> Unit)? = null,
    content: @Composable (PaddingValues) -> Unit,
) {
    Scaffold(
        modifier = modifier.fillMaxSize(),
        topBar = {
            SujianTopAppBar(
                title = title,
                navigationIcon = navigationIcon,
                navigationIconContentDescription = navigationIconContentDescription,
                onNavigationClick = onNavigationClick,
                actions = actions,
            )
        },
        snackbarHost = {
            if (snackbarHostState != null) {
                SnackbarHost(hostState = snackbarHostState) { data ->
                    SujianSnackbar(data = data)
                }
            }
        },
        floatingActionButton = {
            if (onFabClick != null && fabIcon != null) {
                SujianFab(
                    onClick = onFabClick,
                    icon = fabIcon,
                    text = fabText,
                    extended = fabExtended,
                    contentDescription = fabContentDescription,
                )
            }
        },
        floatingActionButtonPosition = FabPosition.End,
        containerColor = MaterialTheme.colorScheme.surface,
        content = content,
    )
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SujianSettingsScaffold(
    title: String,
    modifier: Modifier = Modifier,
    navigationIcon: androidx.compose.ui.graphics.vector.ImageVector? = null,
    onNavigationClick: (() -> Unit)? = null,
    content: @Composable (PaddingValues) -> Unit,
) {
    SujianScreenScaffold(
        title = title,
        modifier = modifier,
        navigationIcon = navigationIcon,
        navigationIconContentDescription = "返回",
        onNavigationClick = onNavigationClick,
        content = content,
    )
}
