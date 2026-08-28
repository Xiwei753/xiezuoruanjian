package com.xiwei.sujian.core.designsystem.component

import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.nestedscroll.nestedScroll
import com.xiwei.sujian.core.designsystem.theme.LocalSujianDimensions

@OptIn(ExperimentalMaterial3Api::class)
@Composable
@Suppress("LongParameterList") // #641 评论 问题5：7 参数达 threshold，函数级 suppress（既有先例）
fun SujianTopAppBar(
    title: String,
    modifier: Modifier = Modifier,
    navigationIcon: ImageVector? = null,
    onNavigationClick: (() -> Unit)? = null,
    actions: @Composable () -> Unit = {},
    /** 顶栏容器背景色。写作区（#597）传 [Color.Transparent]，背景透出正文层。 */
    containerColor: Color = MaterialTheme.colorScheme.surface,
    /**
     * 顶栏 window insets（#640 评论 5443789509）。
     *
     * - compact 单栏：用默认 [TopAppBarDefaults.windowInsets]，让 Material3 自己处理 top system inset；
     * - wide SinglePane：顶部已由 safe workbench frame（systemBars + displayCutout）处理，
     *   传 `WindowInsets(0, 0, 0, 0)` 避免二次避让（plan 已把 toolbar rect 放到安全区下方）。
     */
    windowInsets: WindowInsets = TopAppBarDefaults.windowInsets,
) {
    val scrollBehavior = TopAppBarDefaults.pinnedScrollBehavior()
    TopAppBar(
        title = {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(text = title, style = MaterialTheme.typography.titleLarge)
            }
        },
        modifier = modifier.nestedScroll(scrollBehavior.nestedScrollConnection),
        navigationIcon = {
            if (navigationIcon != null && onNavigationClick != null) {
                SujianIconButton(
                    onClick = onNavigationClick,
                    icon = navigationIcon,
                )
            }
        },
        actions = { actions() },
        colors = TopAppBarDefaults.topAppBarColors(
            containerColor = containerColor,
            scrolledContainerColor = containerColor,
            titleContentColor = MaterialTheme.colorScheme.onSurface,
            navigationIconContentColor = MaterialTheme.colorScheme.onSurface,
            actionIconContentColor = MaterialTheme.colorScheme.onSurface,
        ),
        windowInsets = windowInsets,
    )
}
