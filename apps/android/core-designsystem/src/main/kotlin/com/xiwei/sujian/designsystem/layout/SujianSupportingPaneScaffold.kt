package com.xiwei.sujian.designsystem.layout

import androidx.compose.material3.adaptive.layout.AnimatedPane
import androidx.compose.material3.adaptive.layout.SupportingPaneScaffoldRole
import androidx.compose.material3.adaptive.navigation.BackNavigationBehavior
import androidx.compose.material3.adaptive.navigation.NavigableSupportingPaneScaffold
import androidx.compose.material3.adaptive.navigation.rememberSupportingPaneScaffoldNavigator
import androidx.compose.runtime.Composable
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Modifier
import kotlinx.coroutines.launch

@OptIn(androidx.compose.material3.adaptive.ExperimentalMaterial3AdaptiveApi::class)
@Composable
fun <T : Any> SujianSupportingPaneScaffold(
    mainPane: @Composable SujianSupportingScope<T>.() -> Unit,
    supportingPane: @Composable SujianSupportingScope<T>.() -> Unit,
    modifier: Modifier = Modifier,
    extraPane: (@Composable SujianSupportingScope<T>.() -> Unit)? = null,
) {
    val navigator = rememberSupportingPaneScaffoldNavigator<T>()
    val coroutineScope = rememberCoroutineScope()

    NavigableSupportingPaneScaffold(
        navigator = navigator,
        modifier = modifier,
        mainPane = {
            AnimatedPane {
                SujianSupportingScope(
                    navigator = navigator,
                    coroutineScope = coroutineScope,
                ).mainPane()
            }
        },
        supportingPane = {
            AnimatedPane {
                SujianSupportingScope(
                    navigator = navigator,
                    coroutineScope = coroutineScope,
                ).supportingPane()
            }
        },
        extraPane = if (extraPane != null) {
            {
                AnimatedPane {
                    SujianSupportingScope(
                        navigator = navigator,
                        coroutineScope = coroutineScope,
                    ).extraPane()
                }
            }
        } else null,
    )
}

@OptIn(androidx.compose.material3.adaptive.ExperimentalMaterial3AdaptiveApi::class)
class SujianSupportingScope<T : Any>(
    private val navigator: androidx.compose.material3.adaptive.navigation.ThreePaneScaffoldNavigator<T>,
    private val coroutineScope: kotlinx.coroutines.CoroutineScope,
) {
    val currentContentKey: T?
        get() = navigator.currentDestination?.contentKey

    fun navigateToSupporting(key: T) {
        coroutineScope.launch {
            navigator.navigateTo(SupportingPaneScaffoldRole.Supporting, key)
        }
    }

    fun navigateBack() {
        coroutineScope.launch {
            navigator.navigateBack(BackNavigationBehavior.PopUntilScaffoldValueChange)
        }
    }
}
