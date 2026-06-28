package com.xiwei.sujian.model

/**
 * Android typed model for editor animation events from Core.
 *
 * Desktop QML may keep consuming animation_events_json for its overlay path; Android must stay on
 * typed models and must not route typed DTO -> JSON -> handwritten parser.
 */
enum class EditorAnimationKindData {
    Insert,
    Delete,
    Cursor
}

data class EditorAnimationEventData(
    val id: ULong,
    val kind: EditorAnimationKindData,
    val rangeStart: Int,
    val rangeLen: Int,
    val text: String,
    val oldCursorIndex: Int,
    val newCursorIndex: Int,
    val durationMs: Long
)
