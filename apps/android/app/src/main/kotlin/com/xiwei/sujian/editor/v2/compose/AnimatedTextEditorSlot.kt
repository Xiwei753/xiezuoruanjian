package com.xiwei.sujian.editor.v2.compose

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInWindow
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import com.xiwei.sujian.data.BridgeProvider
import com.xiwei.sujian.editor.v2.coordinator.AnimatedTextEditorCoordinator
import com.xiwei.sujian.editor.v2.coordinator.EditingState
import com.xiwei.sujian.editor.v2.host.SujianEditorView
import com.xiwei.sujian.ui.compose.theme.EditorThemeAdapter

@Composable
fun AnimatedTextEditorSlot(
    coordinator: AnimatedTextEditorCoordinator,
    modifier: Modifier = Modifier
) {
    var editorView by remember { mutableStateOf<SujianEditorView?>(null) }
    var slotPositionInWindow by remember { mutableStateOf(Offset.Zero) }

    val activeTargetId = coordinator.activeTargetId
    val editingState = coordinator.editingState
    val isVisible = activeTargetId != null && (editingState == EditingState.BINDING || editingState == EditingState.EDITING)

    val targetGeometry = coordinator.activeTargetGeometry
    val targetTransform = coordinator.activeTargetTransform

    val density = LocalDensity.current

    val slotLocalLeft = (targetGeometry.left.toFloat() - slotPositionInWindow.x) * targetTransform.scaleX + targetTransform.translateX
    val slotLocalTop = (targetGeometry.top.toFloat() - slotPositionInWindow.y) * targetTransform.scaleY + targetTransform.translateY
    val slotWidthPx = targetGeometry.width().toFloat() * targetTransform.scaleX
    val slotHeightPx = targetGeometry.height().toFloat() * targetTransform.scaleY

    val slotWidthDp = with(density) { slotWidthPx.toDp() }
    val slotHeightDp = with(density) { slotHeightPx.toDp() }

    val themeColors = EditorThemeAdapter.extractColors()

    Box(
        modifier = modifier
            .onGloballyPositioned { coordinates ->
                slotPositionInWindow = coordinates.positionInWindow()
            }
    ) {
        if (isVisible && slotWidthPx > 0f && slotHeightPx > 0f) {
            AndroidView(
                factory = { ctx ->
                    val view = coordinator.getSharedEditorView()
                        ?: SujianEditorView(ctx).also { coordinator.setSharedEditorView(it) }
                    editorView = view
                    EditorThemeAdapter.applyToView(view, themeColors)
                    view
                },
                update = { view ->
                    view.visibility = android.view.View.VISIBLE
                    coordinator.updateHostGeometry(
                        slotWidthPx,
                        slotHeightPx
                    )
                },
                onReset = { view ->
                    view.resetForReuse()
                },
                onRelease = { view ->
                    view.visibility = android.view.View.GONE
                },
                modifier = Modifier
                    .graphicsLayer {
                        translationX = slotLocalLeft
                        translationY = slotLocalTop
                        scaleX = targetTransform.scaleX
                        scaleY = targetTransform.scaleY
                        clip = true
                    }
                    .requiredSize(
                        width = slotWidthDp.coerceAtLeast(1.dp),
                        height = slotHeightDp.coerceAtLeast(1.dp)
                    )
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
