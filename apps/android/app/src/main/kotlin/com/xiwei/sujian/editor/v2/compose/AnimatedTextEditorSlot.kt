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
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.TransformOrigin
import androidx.compose.ui.layout.boundsInWindow
import androidx.compose.ui.layout.onGloballyPositioned
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
    var slotBoundsInWindow by remember { mutableStateOf(Rect.Zero) }

    val activeTargetId = coordinator.activeTargetId
    val editingState = coordinator.editingState
    val isVisible = activeTargetId != null && (editingState == EditingState.BINDING || editingState == EditingState.EDITING)

    val targetGeometry = coordinator.activeTargetGeometry
    val targetTransform = coordinator.activeTargetTransform

    val density = LocalDensity.current

    val unscaledWidthPx = targetGeometry.width().toFloat()
    val unscaledHeightPx = targetGeometry.height().toFloat()

    val scaledWidthPx = unscaledWidthPx * targetTransform.scaleX
    val scaledHeightPx = unscaledHeightPx * targetTransform.scaleY

    val targetRectInWindow = Rect(
        left = targetGeometry.left.toFloat() + targetTransform.translateX,
        top = targetGeometry.top.toFloat() + targetTransform.translateY,
        right = targetGeometry.left.toFloat() + targetTransform.translateX + scaledWidthPx,
        bottom = targetGeometry.top.toFloat() + targetTransform.translateY + scaledHeightPx
    )

    val slotLocalLeft = targetRectInWindow.left - slotBoundsInWindow.left
    val slotLocalTop = targetRectInWindow.top - slotBoundsInWindow.top

    val slotWidthPx = if (isVisible && scaledWidthPx > 0f) scaledWidthPx else 1f
    val slotHeightPx = if (isVisible && scaledHeightPx > 0f) scaledHeightPx else 1f

    val slotWidthDp = with(density) { slotWidthPx.toDp() }
    val slotHeightDp = with(density) { slotHeightPx.toDp() }

    val themeColors = EditorThemeAdapter.extractColors()

    DisposableEffect(Unit) {
        onDispose {
            coordinator.releaseHost()
        }
    }

    Box(
        modifier = modifier
            .onGloballyPositioned { coordinates ->
                slotBoundsInWindow = coordinates.boundsInWindow()
            }
    ) {
        AndroidView(
            factory = { ctx ->
                val view = coordinator.getSharedEditorView()
                    ?: SujianEditorView(ctx).also { coordinator.setSharedEditorView(it) }
                EditorThemeAdapter.applyToView(view, themeColors)
                view
            },
            update = { view ->
                EditorThemeAdapter.applyToView(view, themeColors)
                if (isVisible && scaledWidthPx > 0f && scaledHeightPx > 0f) {
                    view.visibility = android.view.View.VISIBLE
                    coordinator.updateHostGeometry(unscaledWidthPx, unscaledHeightPx)
                } else {
                    view.visibility = android.view.View.GONE
                }
            },
            onReset = { view ->
                view.resetForReuse()
            },
            onRelease = { view ->
                view.visibility = android.view.View.GONE
            },
            modifier = Modifier
                .graphicsLayer {
                    if (isVisible && scaledWidthPx > 0f && scaledHeightPx > 0f) {
                        translationX = slotLocalLeft
                        translationY = slotLocalTop
                        scaleX = targetTransform.scaleX
                        scaleY = targetTransform.scaleY
                        transformOrigin = TransformOrigin(0f, 0f)
                        clip = true
                        alpha = 1f
                    } else {
                        translationX = 0f
                        translationY = 0f
                        scaleX = 1f
                        scaleY = 1f
                        alpha = 0f
                        clip = true
                    }
                }
                .requiredSize(
                    width = slotWidthDp.coerceAtLeast(1.dp),
                    height = slotHeightDp.coerceAtLeast(1.dp)
                )
        )
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
