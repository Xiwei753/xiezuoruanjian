package com.xiwei.sujian.editor.v2.compose

import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.remember
import androidx.compose.ui.platform.LocalContext
import androidx.compose.runtime.Composable
import com.xiwei.sujian.data.BridgeProvider
import com.xiwei.sujian.editor.v2.coordinator.EditorSessionCoordinator
import com.xiwei.sujian.editor.v2.coordinator.EditorWindowHost

/**
 * #595 一：CompositionLocal 提供 [EditorWindowHost] — 从 [AnimatedTextEditorSlot]
 * 提取到独立文件，因为 AnimatedTextEditorSlot 根部覆盖层已删除。
 */
val LocalEditorWindowHost = compositionLocalOf<EditorWindowHost?> {
    null
}

@Composable
fun rememberEditorWindowHost(
    animationTimeSource: com.xiwei.sujian.editor.v2.visual.AnimationTimeSource = com.xiwei.sujian.editor.v2.visual.ChoreographerAnimationTimeSource(),
    transactionIdSource: com.xiwei.sujian.editor.v2.visual.TransactionIdSource = com.xiwei.sujian.editor.v2.visual.TransactionIdSource()
): EditorWindowHost {
    val context = LocalContext.current
    val existing = LocalEditorWindowHost.current
    return remember {
        existing ?: run {
            val bridge = BridgeProvider.getAppServiceBridge(context)
            val session = EditorSessionCoordinator(bridge)
            EditorWindowHost(context, session, bridge, animationTimeSource, transactionIdSource)
        }
    }
}
