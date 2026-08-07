package com.xiwei.sujian.data.starmap

import com.xiwei.sujian.model.StarMapEdgeKind
import com.xiwei.sujian.model.StarMapNodeKind
import uniffi.writer_core.StarMapEdgeKindDto
import uniffi.writer_core.StarMapNodeKindDto

internal fun StarMapNodeKindDto.toModel(): StarMapNodeKind =
    when (this) {
        StarMapNodeKindDto.CHARACTER -> StarMapNodeKind.Character
        StarMapNodeKindDto.EVENT -> StarMapNodeKind.Event
        StarMapNodeKindDto.LOCATION -> StarMapNodeKind.Location
        StarMapNodeKindDto.ITEM -> StarMapNodeKind.Item
        StarMapNodeKindDto.CONCEPT -> StarMapNodeKind.Concept
        StarMapNodeKindDto.THEME -> StarMapNodeKind.Theme
        StarMapNodeKindDto.NOTE -> StarMapNodeKind.Note
        StarMapNodeKindDto.ORGANIZATION -> StarMapNodeKind.Organization
        StarMapNodeKindDto.TIMELINE -> StarMapNodeKind.Timeline
        StarMapNodeKindDto.PLOT -> StarMapNodeKind.Plot
        StarMapNodeKindDto.FORESHADOWING -> StarMapNodeKind.Foreshadowing
        StarMapNodeKindDto.CHAPTER -> StarMapNodeKind.Chapter
        StarMapNodeKindDto.CUSTOM -> StarMapNodeKind.Custom
    }

internal fun StarMapNodeKind.toDto(): StarMapNodeKindDto =
    when (this) {
        StarMapNodeKind.Character -> StarMapNodeKindDto.CHARACTER
        StarMapNodeKind.Event -> StarMapNodeKindDto.EVENT
        StarMapNodeKind.Location -> StarMapNodeKindDto.LOCATION
        StarMapNodeKind.Item -> StarMapNodeKindDto.ITEM
        StarMapNodeKind.Concept -> StarMapNodeKindDto.CONCEPT
        StarMapNodeKind.Theme -> StarMapNodeKindDto.THEME
        StarMapNodeKind.Note -> StarMapNodeKindDto.NOTE
        StarMapNodeKind.Organization -> StarMapNodeKindDto.ORGANIZATION
        StarMapNodeKind.Timeline -> StarMapNodeKindDto.TIMELINE
        StarMapNodeKind.Plot -> StarMapNodeKindDto.PLOT
        StarMapNodeKind.Foreshadowing -> StarMapNodeKindDto.FORESHADOWING
        StarMapNodeKind.Chapter -> StarMapNodeKindDto.CHAPTER
        StarMapNodeKind.Custom -> StarMapNodeKindDto.CUSTOM
    }

internal fun StarMapEdgeKindDto.toModel(): StarMapEdgeKind =
    when (this) {
        StarMapEdgeKindDto.CONTAINS -> StarMapEdgeKind.Contains
        StarMapEdgeKindDto.REFERENCES -> StarMapEdgeKind.References
        StarMapEdgeKindDto.APPEARS_IN -> StarMapEdgeKind.AppearsIn
        StarMapEdgeKindDto.CAUSES -> StarMapEdgeKind.Causes
        StarMapEdgeKindDto.RELATED_TO -> StarMapEdgeKind.RelatedTo
        StarMapEdgeKindDto.LOCATED_AT -> StarMapEdgeKind.LocatedAt
        StarMapEdgeKindDto.CHARACTER_RELATION -> StarMapEdgeKind.CharacterRelation
        StarMapEdgeKindDto.TIMELINE -> StarMapEdgeKind.Timeline
        StarMapEdgeKindDto.FORESHADOWS -> StarMapEdgeKind.Foreshadows
        StarMapEdgeKindDto.RESOLVES -> StarMapEdgeKind.Resolves
        StarMapEdgeKindDto.DEPENDS_ON -> StarMapEdgeKind.DependsOn
        StarMapEdgeKindDto.CONFLICTS_WITH -> StarMapEdgeKind.ConflictsWith
        StarMapEdgeKindDto.CUSTOM -> StarMapEdgeKind.Custom
    }

internal fun StarMapEdgeKind.toDto(): StarMapEdgeKindDto =
    when (this) {
        StarMapEdgeKind.Contains -> StarMapEdgeKindDto.CONTAINS
        StarMapEdgeKind.References -> StarMapEdgeKindDto.REFERENCES
        StarMapEdgeKind.AppearsIn -> StarMapEdgeKindDto.APPEARS_IN
        StarMapEdgeKind.Causes -> StarMapEdgeKindDto.CAUSES
        StarMapEdgeKind.RelatedTo -> StarMapEdgeKindDto.RELATED_TO
        StarMapEdgeKind.LocatedAt -> StarMapEdgeKindDto.LOCATED_AT
        StarMapEdgeKind.CharacterRelation -> StarMapEdgeKindDto.CHARACTER_RELATION
        StarMapEdgeKind.Timeline -> StarMapEdgeKindDto.TIMELINE
        StarMapEdgeKind.Foreshadows -> StarMapEdgeKindDto.FORESHADOWS
        StarMapEdgeKind.Resolves -> StarMapEdgeKindDto.RESOLVES
        StarMapEdgeKind.DependsOn -> StarMapEdgeKindDto.DEPENDS_ON
        StarMapEdgeKind.ConflictsWith -> StarMapEdgeKindDto.CONFLICTS_WITH
        StarMapEdgeKind.Custom -> StarMapEdgeKindDto.CUSTOM
    }
