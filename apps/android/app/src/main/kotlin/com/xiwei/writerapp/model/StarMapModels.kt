package com.xiwei.writerapp.model

data class StarMapMeta(
    val starmapId: String,
    val title: String,
    val description: String,
    val projectId: String?,
    val parentStarmapId: String?,
    val isMainForProject: Boolean,
    val accentColor: String,
    val createdAt: Long,
    val updatedAt: Long,
    val nodeCount: Int,
    val edgeCount: Int,
    val linkedChapterCount: Int,
    val childStarmapCount: Int
)

data class StarMapData(
    val graph: MindMapGraph,
    val layout: MindMapLayout
)
