package com.xiwei.sujian.ui.compose.starmap

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.SideEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import com.xiwei.sujian.R
import com.xiwei.sujian.designsystem.icon.SujianIcons

/**
 * 星图目的地占位入口：星图功能尚未实现，仅显示占位提示。
 *
 * 保留 [StarMapScreen] 签名以兼容
 * [com.xiwei.sujian.ui.compose.navigation.SujianNavigationSuite] 的调用；
 * 占位体内清理星图顶栏状态，避免残留编辑态标题/返回/操作。
 */
@Composable
fun StarMapScreen(
    topBarState: com.xiwei.sujian.ui.compose.navigation.StarMapTopBarState,
    modifier: Modifier = Modifier,
) {
    SideEffect { topBarState.clear() }

    Column(
        modifier = modifier.fillMaxSize(),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Icon(
            imageVector = SujianIcons.Hub,
            contentDescription = null,
            tint = MaterialTheme.colorScheme.outline,
        )
        Text(
            text = stringResource(id = R.string.starmap_not_implemented),
            style = MaterialTheme.typography.bodyLarge,
            color = MaterialTheme.colorScheme.outline,
        )
    }
}
