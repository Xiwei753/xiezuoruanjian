package com.xiwei.sujian.ui.compose.starmap

import com.xiwei.sujian.model.StarMapLayoutData
import com.xiwei.sujian.model.StarMapNodeKind
import com.xiwei.sujian.model.StarMapViewportData

internal sealed interface StarMapAction {
    data class AddNode(val title: String, val kind: StarMapNodeKind) : StarMapAction
    data class DeleteNode(val nodeId: String) : StarMapAction
    data class UpdateNode(val nodeId: String, val title: String, val kind: StarMapNodeKind?) : StarMapAction
    data class AddEdge(val fromNodeId: String, val toNodeId: String) : StarMapAction
    data class DeleteEdge(val edgeId: String) : StarMapAction
    data class SaveLayout(val layout: StarMapLayoutData) : StarMapAction
    data class SaveViewport(val viewport: StarMapViewportData) : StarMapAction
    data class SelectNode(val nodeId: String?) : StarMapAction
    data class StartEditingNode(val nodeId: String) : StarMapAction
    data class StopEditingNode(val nodeId: String?) : StarMapAction
}
