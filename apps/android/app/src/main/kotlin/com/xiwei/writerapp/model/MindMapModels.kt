package com.xiwei.writerapp.model

import com.google.gson.annotations.SerializedName
import com.google.gson.JsonDeserializer
import com.google.gson.JsonElement
import com.google.gson.JsonDeserializationContext
import java.lang.reflect.Type

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
