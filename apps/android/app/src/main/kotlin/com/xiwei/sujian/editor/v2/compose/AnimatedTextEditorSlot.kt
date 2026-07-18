package com.xiwei.sujian.editor.v2.compose

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.viewinterop.AndroidView
import com.xiwei.sujian.data.BridgeProvider
import com.xiwei.sujian.editor.v2.coordinator.AnimatedTextEditorCoordinator
import com.xiwei.sujian.editor.v2.coordinator.EditingState
import com.xiwei.sujian.editor.v2.host.SujianEditorView

@Composable
fun AnimatedTextEditorSlot(
    coordinator: AnimatedTextEditorCoordinator,
    modifier: Modifier = Modifier
) {
    var editorView by remember { mutableStateOf<SujianEditorView?>(null) }

    val activeTargetId = coordinator.getActiveTargetId()
    val editingState = coordinator.getEditingState()
    val isVisible = activeTargetId != null && editingState == EditingState.EDITING

    Box(modifier = modifier.fillMaxSize()) {
        if (isVisible) {
            AndroidView(
                factory = { ctx ->
                    val view = coordinator.getSharedEditorView()
                        ?: SujianEditorView(ctx).also { coordinator.setSharedEditorView(it) }
                    editorView = view
                    view
                },
                update = { view ->
                    view.visibility = android.view.View.VISIBLE
                    coordinator.positionActiveTargetView(view)
                },
                onReset = { view ->
                    view.resetForReuse()
                },
                onRelease = { view ->
                    view.visibility = android.view.View.GONE
                },
                modifier = Modifier.fillMaxSize()
            )
        }
    }
}

val LocalAnimatedTextEditorCoordinator = androidx.compose.runtime.compositionLocalOf<AnimatedTextEditorCoordinator?> {
    null
}

@Composable
fun rememberAnimatedTextEditorCoordinator(): AnimatedTextEditorCoordinator {
    val context = LocalContext.current
    val existing = LocalAnimatedTextEditorCoordinator.current
    return remember {
        existing ?: run {
            val bridge = BridgeProvider.getAppServiceBridge(context)
            AnimatedTextEditorCoordinator(context, bridge)
        }
    }
}
