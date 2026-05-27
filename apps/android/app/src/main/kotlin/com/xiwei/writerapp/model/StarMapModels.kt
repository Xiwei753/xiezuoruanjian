package com.xiwei.writerapp.model

import com.google.gson.annotations.SerializedName
import com.google.gson.JsonDeserializer
import com.google.gson.JsonElement
import com.google.gson.JsonDeserializationContext
import java.lang.reflect.Type

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

enum class StarMapNodeKind {
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
    @SerializedName("chapter") Chapter,
    @SerializedName("custom") Custom
}

class StarMapNodeKindDeserializer : JsonDeserializer<StarMapNodeKind> {
    override fun deserialize(
        json: JsonElement,
        typeOfT: Type,
        context: JsonDeserializationContext
    ): StarMapNodeKind {
        val kindString = json.asString.lowercase()
        return when (kindString) {
            "character" -> StarMapNodeKind.Character
            "event" -> StarMapNodeKind.Event
            "location" -> StarMapNodeKind.Location
            "item" -> StarMapNodeKind.Item
            "concept" -> StarMapNodeKind.Concept
            "theme" -> StarMapNodeKind.Theme
            "note" -> StarMapNodeKind.Note
            "organization" -> StarMapNodeKind.Organization
            "timeline" -> StarMapNodeKind.Timeline
            "plot" -> StarMapNodeKind.Plot
            "foreshadowing" -> StarMapNodeKind.Foreshadowing
            "chapter" -> StarMapNodeKind.Chapter
            else -> StarMapNodeKind.Custom
        }
    }
}

enum class StarMapEdgeKind {
    @SerializedName("contains") Contains,
    @SerializedName("references") References,
    @SerializedName("appearsIn") AppearsIn,
    @SerializedName("causes") Causes,
    @SerializedName("relatedTo") RelatedTo,
    @SerializedName("locatedAt") LocatedAt,
    @SerializedName("characterRelation") CharacterRelation,
    @SerializedName("timeline") Timeline,
    @SerializedName("foreshadows") Foreshadows,
    @SerializedName("resolves") Resolves,
    @SerializedName("dependsOn") DependsOn,
    @SerializedName("conflictsWith") ConflictsWith,
    @SerializedName("custom") Custom
}

class StarMapEdgeKindDeserializer : JsonDeserializer<StarMapEdgeKind> {
    override fun deserialize(
        json: JsonElement,
        typeOfT: Type,
        context: JsonDeserializationContext
    ): StarMapEdgeKind {
        val kindString = json.asString.lowercase()
        return when (kindString) {
            "contains" -> StarMapEdgeKind.Contains
            "references" -> StarMapEdgeKind.References
            "appearsin" -> StarMapEdgeKind.AppearsIn
            "causes" -> StarMapEdgeKind.Causes
            "relatedto" -> StarMapEdgeKind.RelatedTo
            "locatedat" -> StarMapEdgeKind.LocatedAt
            "characterrelation" -> StarMapEdgeKind.CharacterRelation
            "timeline" -> StarMapEdgeKind.Timeline
            "foreshadows" -> StarMapEdgeKind.Foreshadows
            "resolves" -> StarMapEdgeKind.Resolves
            "dependson" -> StarMapEdgeKind.DependsOn
            "conflictswith" -> StarMapEdgeKind.ConflictsWith
            else -> StarMapEdgeKind.Custom
        }
    }
}

data class StarMapGraphNode(
    val id: String,
    val title: String,
    val kind: StarMapNodeKind,
    val payload: Map<String, Any>? = null,
    val tags: List<String> = emptyList(),
    val createdAt: Long = 0,
    val updatedAt: Long = 0
)

data class StarMapGraphEdge(
    val id: String,
    val from: String,
    val to: String,
    val kind: StarMapEdgeKind,
    val label: String? = null,
    val payload: Map<String, Any>? = null,
    val createdAt: Long = 0,
    val updatedAt: Long = 0
)

data class StarMapGraphData(
    val schemaVersion: Int,
    val id: String,
    val starmapId: String,
    val title: String,
    val nodes: List<StarMapGraphNode>,
    val edges: List<StarMapGraphEdge>,
    val createdAt: Long,
    val updatedAt: Long
)

enum class StarMapLayoutKind {
    @SerializedName("freeform") Freeform,
    @SerializedName("autoRadial") AutoRadial,
    @SerializedName("custom") Custom
}

data class StarMapLayoutNodeData(
    val nodeId: String,
    val x: Float,
    val y: Float,
    val width: Float,
    val height: Float,
    val radius: Float,
    val collapsed: Boolean,
    val zIndex: Int
)

data class StarMapLayoutData(
    val kind: StarMapLayoutKind,
    val nodes: List<StarMapLayoutNodeData>
)

data class StarMapNodePatch(
    val title: String? = null,
    val kind: StarMapNodeKind? = null,
    val payload: Map<String, Any>? = null,
    val tags: List<String>? = null
)

data class StarMapEdgePatch(
    val kind: StarMapEdgeKind? = null,
    val label: String? = null,
    val payload: Map<String, Any>? = null
)

data class StarMapData(
    val graph: StarMapGraphData,
    val layout: StarMapLayoutData
)
