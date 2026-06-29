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

// ── Visual Transaction 数据模型 (Phase 2) ──

/**
 * 编辑原因枚举，与 Core EditorTransactionCauseDto 对齐。
 *
 * 包含 Undo/Redo，用于视觉事务的 cause 映射。
 */
enum class SujianEditCauseData {
    Typing,
    Delete,
    ImeComposition,
    TypingCommit,
    Paste,
    Undo,
    Redo,
    Load,
    Format,
    Programmatic
}

/**
 * 坐标模式枚举，与 Core VisualCoordinateModeDto 对齐。
 */
enum class VisualCoordinateModeData {
    Baseline
}

/**
 * 视觉事务数据类，与 Core EditorVisualTransactionDto 对齐。
 *
 * Core 层只裁判事件语义和范围（UTF-8 byte offset），
 * Android 层负责 layout 坐标转换和绘制。
 *
 * 坐标字段（oldCursorRect, newCursorRect, deletedGlyphRects, insertGlyphRects）
 * 由 Android 层自行填充——Core 不传坐标。
 */
data class EditorVisualTransactionData(
    val id: ULong,
    val kind: EditorAnimationKindData,
    val cause: SujianEditCauseData,
    val oldText: String,
    val newText: String,
    /** 旧选区 anchor（UTF-8 byte offset） */
    val oldSelectionAnchor: Int,
    /** 旧选区 head（UTF-8 byte offset） */
    val oldSelectionHead: Int,
    /** 新选区 anchor（UTF-8 byte offset） */
    val newSelectionAnchor: Int,
    /** 新选区 head（UTF-8 byte offset） */
    val newSelectionHead: Int,
    /** 插入范围起始（UTF-8 byte offset），无插入时为 0 */
    val insertedRangeStart: Int,
    /** 插入范围结束（UTF-8 byte offset），无插入时为 0 */
    val insertedRangeEnd: Int,
    val durationMs: Long,
    val coordinateMode: VisualCoordinateModeData,

    // ── 可变坐标字段（由 Android 层填充） ──
    /** 插入前光标矩形，由 runVisualEdit 在 edit 前捕获 */
    var oldCursorRect: SujianCursorRectData? = null,
    /** 插入后光标矩形，由 runVisualEdit 在 edit 后捕获 */
    var newCursorRect: SujianCursorRectData? = null,
    /** 被删除的 glyph 矩形列表，由 onBeforeDelete 捕获 */
    var deletedGlyphRects: List<SujianGlyphRectData> = emptyList(),
    /** 被插入的 glyph 矩形列表，由 handleInsertTransaction 填充 */
    var insertGlyphRects: List<SujianGlyphRectData> = emptyList()
)

/**
 * 光标矩形数据类，与 Core CursorRectDto 对齐。
 */
data class SujianCursorRectData(
    val x: Double,
    val top: Double,
    val bottom: Double,
    val baselineY: Double
)

/**
 * Glyph 矩形数据类，与 Core GlyphRectDto 对齐。
 */
data class SujianGlyphRectData(
    val x: Double,
    val y: Double,
    val w: Double,
    val h: Double,
    val char: String,
    val baselineY: Double
)

/**
 * 视觉编辑上下文，由 runVisualEdit 生成并传给 AnimationController。
 */
data class SujianVisualEditContext(
    val oldCursorRect: SujianCursorRectData?,
    val newCursorRect: SujianCursorRectData?,
    val cause: SujianEditCauseData
)
