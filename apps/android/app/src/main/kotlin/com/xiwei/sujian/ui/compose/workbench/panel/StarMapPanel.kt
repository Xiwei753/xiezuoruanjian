package com.xiwei.sujian.ui.compose.workbench.panel

import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.viewmodel.compose.viewModel
import com.xiwei.sujian.ui.compose.navigation.StarMapTopBarState
import com.xiwei.sujian.ui.compose.starmap.StarMapScreen
import com.xiwei.sujian.ui.compose.starmap.StarMapViewModel

@Composable
fun StarMapPanel(
    modifier: Modifier = Modifier,
) {
    // 面板内嵌星图：标题/返回/操作不上抛根壳 TopAppBar（面板自带小型标题区）。
    val panelTopBarState = remember { StarMapTopBarState() }
    val starMapVm: StarMapViewModel = viewModel(factory = StarMapViewModel.Factory(LocalContext.current))
    StarMapScreen(
        topBarState = panelTopBarState,
        viewModel = starMapVm,
        modifier = modifier,
    )
}
