package com.xiwei.writerapp.model

import com.google.gson.annotations.SerializedName

enum class MindMapNodeKind {
    @SerializedName("project") Project,
    @SerializedName("volume") Volume,
    @SerializedName("chapter") Chapter
}

data class MindMapNode(
    val id: String,
    val title: String,
    val kind: MindMapNodeKind,
    val parentId: String?,
    val depth: Int,
    val x: Float,
    val y: Float,
    val radius: Float,
    val width: Float,
    val height: Float,
    val collapsed: Boolean
)

data class MindMapEdge(
    val from: String,
    val to: String,
    val kind: String
)

data class MindMapBounds(
    val minX: Float,
    val minY: Float,
    val maxX: Float,
    val maxY: Float
)

data class MindMapSnapshot(
    val projectId: String,
    val layoutKind: String,
    val nodes: List<MindMapNode>,
    val edges: List<MindMapEdge>,
    val bounds: MindMapBounds,
    val generatedAt: Long
) {
    @Transient var parseTimeMs: Long = 0
    @Transient var jsonBytes: Int = 0
}

data class MindMapViewport(
    var scale: Float = 1.0f,
    var translateX: Float = 0.0f,
    var translateY: Float = 0.0f
)
