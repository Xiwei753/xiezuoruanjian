package com.xiwei.sujian.designsystem.layout

import androidx.compose.material3.adaptive.layout.AnimatedPane
import androidx.compose.material3.adaptive.layout.ListDetailPaneScaffoldRole
import androidx.compose.material3.adaptive.layout.PaneScaffoldDirective
import androidx.compose.material3.adaptive.navigation.BackNavigationBehavior
import androidx.compose.material3.adaptive.navigation.NavigableListDetailPaneScaffold
import androidx.compose.material3.adaptive.navigation.ThreePaneScaffoldNavigator
import androidx.compose.material3.adaptive.navigation.rememberListDetailPaneScaffoldNavigator
import androidx.compose.runtime.Composable
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Modifier
import kotlinx.coroutines.launch

@OptIn(androidx.compose.material3.adaptive.ExperimentalMaterial3AdaptiveApi::class)
@Composable
fun <T : Any> SujianListDetailScaffold(
    listPane: @Composable SujianListDetailScope<T>.() -> Unit,
    detailPane: @Composable SujianListDetailScope<T>.() -> Unit,
    modifier: Modifier = Modifier,
    extraPane: (@Composable SujianListDetailScope<T>.() -> Unit)? = null,
    scaffoldDirective: PaneScaffoldDirective? = null,
) {
    val navigator = if (scaffoldDirective != null) {
        rememberListDetailPaneScaffoldNavigator<T>(scaffoldDirective = scaffoldDirective)
    } else {
        rememberListDetailPaneScaffoldNavigator<T>()
    }
    SujianListDetailScaffoldWithNavigator(
        navigator = navigator,
        modifier = modifier,
        listPane = listPane,
        detailPane = detailPane,
        extraPane = extraPane,
    )
}

@OptIn(androidx.compose.material3.adaptive.ExperimentalMaterial3AdaptiveApi::class)
@Composable
fun <T : Any> SujianListDetailScaffoldWithNavigator(
    navigator: ThreePaneScaffoldNavigator<T>,
    modifier: Modifier = Modifier,
    listPane: @Composable SujianListDetailScope<T>.() -> Unit,
    detailPane: @Composable SujianListDetailScope<T>.() -> Unit,
    extraPane: (@Composable SujianListDetailScope<T>.() -> Unit)? = null,
) {
    val coroutineScope = rememberCoroutineScope()

    NavigableListDetailPaneScaffold(
        navigator = navigator,
        modifier = modifier,
        listPane = {
            AnimatedPane {
                SujianListDetailScope(
                    navigator = navigator,
                    coroutineScope = coroutineScope,
                ).listPane()
            }
        },
        detailPane = {
            AnimatedPane {
                SujianListDetailScope(
                    navigator = navigator,
                    coroutineScope = coroutineScope,
                ).detailPane()
            }
        },
        extraPane = if (extraPane != null) {
            {
                AnimatedPane {
                    SujianListDetailScope(
                        navigator = navigator,
                        coroutineScope = coroutineScope,
                    ).extraPane()
                }
            }
        } else null,
    )
}

@OptIn(androidx.compose.material3.adaptive.ExperimentalMaterial3AdaptiveApi::class)
class SujianListDetailScope<T : Any>(
    private val navigator: androidx.compose.material3.adaptive.navigation.ThreePaneScaffoldNavigator<T>,
    private val coroutineScope: kotlinx.coroutines.CoroutineScope,
) {
    val currentContentKey: T?
        get() = navigator.currentDestination?.contentKey

    fun navigateToDetail(key: T) {
        coroutineScope.launch {
            navigator.navigateTo(ListDetailPaneScaffoldRole.Detail, key)
        }
    }

    fun navigateBack() {
        coroutineScope.launch {
            navigator.navigateBack(BackNavigationBehavior.PopUntilScaffoldValueChange)
        }
    }
}
