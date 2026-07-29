package com.xiwei.sujian.ui.compose.workbench.component

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.width
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.ui.compose.workbench.model.DragDropTarget
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchDragState

@Composable
fun WorkbenchDragOverlay(
    dragState: WorkbenchDragState,
    maxWidthDp: Float,
    maxHeightDp: Float,
    modifier: Modifier = Modifier,
) {
    if (!dragState.isDragging) return

    val hintColor = MaterialTheme.colorScheme.primary.copy(alpha = 0.12f)
    val hintWidth = 72.dp
    val hintHeight = 72.dp

    Box(modifier = modifier) {
        if (dragState.dropTarget == DragDropTarget.DockLeft) {
            Box(
                modifier = Modifier
                    .align(Alignment.CenterStart)
                    .width(hintWidth)
                    .fillMaxHeight()
                    .background(hintColor),
            )
        }

        if (dragState.dropTarget == DragDropTarget.DockRight) {
            Box(
                modifier = Modifier
                    .align(Alignment.CenterEnd)
                    .width(hintWidth)
                    .fillMaxHeight()
                    .background(hintColor),
            )
        }

        if (dragState.dropTarget == DragDropTarget.DockBottom) {
            Box(
                modifier = Modifier
                    .align(Alignment.BottomCenter)
                    .fillMaxWidth()
                    .height(hintHeight)
                    .background(hintColor),
            )
        }

        if (dragState.dropTarget == DragDropTarget.FloatArea) {
            Box(
                modifier = Modifier
                    .align(Alignment.Center)
                    .width(hintWidth)
                    .height(hintHeight)
                    .background(hintColor),
            )
        }
    }
}
