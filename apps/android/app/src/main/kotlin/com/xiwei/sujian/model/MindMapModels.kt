package com.xiwei.sujian.model

import com.google.gson.annotations.SerializedName
import com.google.gson.JsonDeserializer
import com.google.gson.JsonElement
import com.google.gson.JsonDeserializationContext
import java.lang.reflect.Type

/**
 * MindMapModels — 思维导图数据模型
 *
 * **LEGACY: 已废弃，仅保留用于旧数据迁移兼容。正式图谱路线为 StarMap。**
 *
 * 定义思维导图相关的数据类和枚举，与 Rust Core 的思维导图数据结构一一对应。
 *
 * ## 架构定位
 * - 这些模型是 Rust Core JSON 响应的 Kotlin 映射
 * - 所有字段名使用 @SerializedName 映射 Rust 的 snake_case
 *
 * ## 包含模型
 * - MindMapNodeKind：节点类型枚举（项目、卷、章节、角色等）
 * - MindMapSnapshot：思维导图快照数据
 * - MindMapGraph/MindMapGraphNode/MindMapGraphEdge：图数据结构
 */

enum class MindMapNodeKind {
    @SerializedName("project") Project,
    @SerializedName("volume") Volume,
    @SerializedName("chapter") Chapter,
    @SerializedName("textAnchor") TextAnchor,
    @SerializedName("character") Character,
    @SerializedName("event") Event,
    @SerializedName("location") Location,
    @SerializedName("item") Item,
    @SerializedName("concept") Concept,
    @SerializedName("theme") Theme,
    @SerializedName("note") Note,
    @SerializedName("organization") Organization,
    @SerializedName("timeline") Timeline,
    @SerializedName("plot") Plot,
    @SerializedName("foreshadowing") Foreshadowing,
    @SerializedName("custom") Custom
}

class MindMapNodeKindDeserializer : JsonDeserializer<MindMapNodeKind> {
    override fun deserialize(
        json: JsonElement,
        typeOfT: Type,
        context: JsonDeserializationContext
    ): MindMapNodeKind {
        val kindString = json.asString.lowercase()
        return when (kindString) {
            "project" -> MindMapNodeKind.Project
            "volume" -> MindMapNodeKind.Volume
            "chapter" -> MindMapNodeKind.Chapter
            "textanchor" -> MindMapNodeKind.TextAnchor
            "character" -> MindMapNodeKind.Character
            "event" -> MindMapNodeKind.Event
            "location" -> MindMapNodeKind.Location
            "item" -> MindMapNodeKind.Item
            "concept" -> MindMapNodeKind.Concept
            "theme" -> MindMapNodeKind.Theme
            "note" -> MindMapNodeKind.Note
            "organization" -> MindMapNodeKind.Organization
            "timeline" -> MindMapNodeKind.Timeline
            "plot" -> MindMapNodeKind.Plot
            "foreshadowing" -> MindMapNodeKind.Foreshadowing
            else -> MindMapNodeKind.Custom
        }
    }
}

data class MindMapNode(
    val id: String,
    val title: String,
    val kind: MindMapNodeKind,
    val x: Float,
    val y: Float,
    val width: Float,
    val height: Float,
    val radius: Float,
    val collapsed: Boolean,
    val anchorCount: Int,
    val brokenLink: Boolean,
    val tags: List<String> = emptyList()
)

data class MindMapEdge(
    val id: String,
    val from: String,
    val to: String,
    val kind: String,
    val label: String?
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

data class MindMapGraphNode(
    val id: String,
    val title: String,
    val kind: MindMapNodeKind,
    val payload: Map<String, Any>? = null,
    val tags: List<String> = emptyList(),
    val createdAt: Long = 0,
    val updatedAt: Long = 0
)

data class MindMapGraphEdge(
    val id: String,
    val from: String,
    val to: String,
    val kind: String,
    val label: String? = null,
    val payload: Map<String, Any>? = null,
    val createdAt: Long = 0,
    val updatedAt: Long = 0
)

data class MindMapGraphMetadata(
    val id: String,
    val title: String,
    val createdAt: Long,
    val updatedAt: Long
)

data class MindMapGraphsList(
    val defaultGraphId: String?,
    val graphs: List<MindMapGraphMetadata>
)

data class MindMapAnchor(
    val id: String,
    val projectId: String,
    val chapterId: String,
    val startOffset: Int,
    val endOffset: Int,
    val selectedText: String,
    val prefixText: String,
    val suffixText: String,
    val checksum: String = "",
    val createdAt: Long = 0,
    val updatedAt: Long = 0
)

data class MindMapLink(
    val id: String,
    val nodeId: String,
    val anchorId: String,
    val kind: String,
    val createdAt: Long,
    val updatedAt: Long
)

data class MindMapGraph(
    val schemaVersion: Int,
    val id: String,
    val projectId: String,
    val title: String,
    val nodes: List<MindMapGraphNode>,
    val edges: List<MindMapGraphEdge>,
    val anchors: List<MindMapAnchor>,
    val links: List<MindMapLink>,
    val createdAt: Long,
    val updatedAt: Long
)

data class MindMapLayoutNode(
    val nodeId: String,
    val x: Float,
    val y: Float,
    val width: Float,
    val height: Float,
    val radius: Float,
    val collapsed: Boolean,
    val zIndex: Int
)

data class MindMapLayout(
    val kind: String,
    val nodes: List<MindMapLayoutNode>
)

data class MindMapGraphNodePatch(
    val title: String? = null,
    val kind: MindMapNodeKind? = null,
    val payload: Map<String, Any>? = null,
    val tags: List<String>? = null
)

data class MindMapGraphEdgePatch(
    val kind: String? = null,
    val label: String? = null,
    val payload: Map<String, Any>? = null
)
